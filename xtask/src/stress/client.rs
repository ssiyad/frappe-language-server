//! A minimal LSP client over the server's stdio, strict about framing.
//!
//! It answers the requests a server sends on its own initiative the way an
//! editor would. A client that stays silent there can make a correct server
//! look hung.

use std::fmt;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Why a request produced no answer.
#[derive(Debug)]
pub enum Failure {
    /// The server closed stdout: it exited, most likely by panicking.
    Exited,
    /// No answer inside the timeout.
    Hung,
    /// Anything on stdout that is not a framed JSON message.
    Framing(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited => f.write_str("the server exited"),
            Self::Hung => f.write_str("no answer inside the timeout"),
            Self::Framing(error) => write!(f, "broken framing: {error}"),
        }
    }
}

pub struct Client {
    process: Child,
    stdin: ChildStdin,
    inbox: Receiver<Result<Value, String>>,
    next_id: i64,
    timeout: Duration,
}

impl Client {
    pub fn spawn(binary: &Path, log: &Path, timeout: Duration) -> std::io::Result<Self> {
        let mut process = Command::new(binary)
            .args(["--log-level", "warn"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(std::fs::File::create(log)?)
            .spawn()?;
        let stdin = process.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(process.stdout.take().expect("piped stdout"));

        let (sender, inbox) = mpsc::channel();
        thread::spawn(move || read_messages(stdout, &sender));
        Ok(Self {
            process,
            stdin,
            inbox,
            next_id: 1,
            timeout,
        })
    }

    /// Send a request and wait for its response; the elapsed time is the
    /// server's latency as an editor sees it.
    pub fn request(&mut self, method: &str, params: Value) -> Result<(Value, Duration), Failure> {
        let id = self.next_id;
        self.next_id += 1;
        let started = Instant::now();
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;

        loop {
            let left = self.timeout.saturating_sub(started.elapsed());
            let message = match self.inbox.recv_timeout(left) {
                Ok(Ok(message)) => message,
                Ok(Err(error)) => return Err(Failure::Framing(error)),
                Err(RecvTimeoutError::Timeout) => return Err(Failure::Hung),
                Err(RecvTimeoutError::Disconnected) => return Err(Failure::Exited),
            };
            if message.get("method").is_some() {
                self.answer(&message)?;
                continue;
            }
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return Ok((message, started.elapsed()));
            }
        }
    }

    pub fn notify(&mut self, method: &str, params: Value) -> Result<(), Failure> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// Whether the process is still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.process.try_wait(), Ok(None))
    }

    pub fn shutdown(mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline && self.alive() {
            thread::sleep(Duration::from_millis(20));
        }
        let _ = self.process.kill();
    }

    /// Reply to a server-initiated request. Notifications need nothing.
    fn answer(&mut self, message: &Value) -> Result<(), Failure> {
        let Some(id) = message.get("id").cloned() else {
            return Ok(());
        };
        let result = match message["method"].as_str() {
            // One `null` per item: "no settings", which the server must accept.
            Some("workspace/configuration") => {
                let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                Value::Array(vec![Value::Null; items])
            }
            _ => Value::Null,
        };
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    fn send(&mut self, message: &Value) -> Result<(), Failure> {
        let body = message.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
            .and_then(|()| self.stdin.flush())
            .map_err(|_| Failure::Exited)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

fn read_messages(mut stdout: impl BufRead, sender: &mpsc::Sender<Result<Value, String>>) {
    loop {
        match read_message(&mut stdout) {
            Ok(Some(message)) => {
                if sender.send(Ok(message)).is_err() {
                    return;
                }
            }
            Ok(None) => return,
            Err(error) => {
                let _ = sender.send(Err(error));
                return;
            }
        }
    }
}

/// One framed message; `None` at a clean end of stream.
fn read_message(stdout: &mut impl BufRead) -> Result<Option<Value>, String> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if stdout.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Ok(None);
        }
        if line == "\r\n" {
            break;
        }
        let (name, value) = line
            .trim_end()
            .split_once(": ")
            .ok_or_else(|| format!("not a header: {line:?}"))?;
        if name.eq_ignore_ascii_case("Content-Length") {
            length = Some(value.parse::<usize>().map_err(|e| e.to_string())?);
        }
    }
    let length = length.ok_or("a message without Content-Length")?;
    let mut body = vec![0; length];
    stdout.read_exact(&mut body).map_err(|e| e.to_string())?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| format!("not JSON: {error}"))
}
