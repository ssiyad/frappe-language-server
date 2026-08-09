//! Protocol conformance over a real server process — spec 32.
//!
//! The session below is the one an editor drives: negotiate, open a buffer,
//! edit it, ask for every advertised feature, shut down. Two things are being
//! checked at once — that the server answers without a protocol error, and that
//! stdout carries framed JSON-RPC and nothing else (spec 30). A stray
//! `println!` anywhere in the tree corrupts the stream and shows up here.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

struct Session {
    process: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

impl Session {
    fn start() -> Self {
        let mut process = Command::new(env!("CARGO_BIN_EXE_frappe-lsp"))
            .arg("--log-level")
            .arg("debug")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("server binary starts");

        let stdin = process.stdin.take().expect("piped stdin");
        let stdout = BufReader::new(process.stdout.take().expect("piped stdout"));
        Self {
            process,
            stdin,
            stdout,
        }
    }

    fn send(&mut self, message: Value) {
        let body = serde_json::to_string(&message).expect("serializable");
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("write");
        self.stdin.flush().expect("flush");
    }

    fn request(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
        loop {
            let message = self.receive();
            // Skip notifications the server sends on its own initiative.
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}));
    }

    /// Strict framing: anything on stdout that is not a `Content-Length` header
    /// followed by exactly that many bytes of JSON fails the test.
    fn receive(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            let read = self.stdout.read_line(&mut line).expect("read header");
            assert_ne!(read, 0, "server closed stdout mid-message");

            match line.as_str() {
                "\r\n" => break,
                _ => {
                    let (name, value) = line
                        .trim_end()
                        .split_once(": ")
                        .unwrap_or_else(|| panic!("not a header: {line:?}"));
                    if name.eq_ignore_ascii_case("Content-Length") {
                        length = Some(value.parse::<usize>().expect("numeric length"));
                    }
                }
            }
        }

        let mut body = vec![0; length.expect("Content-Length header")];
        self.stdout.read_exact(&mut body).expect("read body");
        serde_json::from_slice(&body).expect("body is JSON")
    }

    fn finish(mut self) -> String {
        let status = self.process.wait().expect("server exits");
        assert!(status.success(), "server exited with {status}");

        let mut stderr = String::new();
        self.process
            .stderr
            .take()
            .expect("piped stderr")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        stderr
    }
}

fn initialize_params() -> Value {
    json!({
        "processId": null,
        "clientInfo": {"name": "integration-test"},
        "rootUri": null,
        "capabilities": {
            "general": {"positionEncodings": ["utf-8", "utf-16"]},
            "textDocument": {
                "synchronization": {"didSave": true},
                "definition": {"linkSupport": true},
                "hover": {"contentFormat": ["markdown", "plaintext"]},
                "diagnostic": {},
                "completion": {"completionItem": {"labelDetailsSupport": true}}
            }
        }
    })
}

#[test]
fn a_full_edit_session_stays_on_protocol() {
    let mut session = Session::start();

    let result = session.request(1, "initialize", initialize_params())["result"].clone();
    let capabilities = &result["capabilities"];

    assert_eq!(result["serverInfo"]["name"], "frappe-lsp");
    assert_eq!(capabilities["positionEncoding"], "utf-8");
    assert_eq!(capabilities["textDocumentSync"]["change"], 2);
    assert_eq!(capabilities["hoverProvider"], true);
    assert_eq!(capabilities["definitionProvider"], true);
    assert_eq!(capabilities["typeDefinitionProvider"], true);
    assert_eq!(capabilities["implementationProvider"], true);
    assert_eq!(capabilities["referencesProvider"], true);
    assert_eq!(capabilities["documentSymbolProvider"], true);
    assert_eq!(capabilities["workspaceSymbolProvider"], true);
    assert_eq!(capabilities["codeActionProvider"], true);
    assert_eq!(capabilities["completionProvider"]["resolveProvider"], true);
    assert_eq!(
        capabilities["completionProvider"]["triggerCharacters"],
        json!(["\"", "'", ".", "["])
    );
    assert!(capabilities["diagnosticProvider"].is_object());
    // Spec 30 names these as deliberately not advertised.
    for absent in [
        "documentFormattingProvider",
        "renameProvider",
        "signatureHelpProvider",
        "semanticTokensProvider",
        "inlayHintProvider",
    ] {
        assert!(
            capabilities[absent].is_null(),
            "{absent} must not be advertised"
        );
    }

    session.notify("initialized", json!({}));

    let uri = "file:///tmp/frappe-lsp-test/app/app/doctype/task/task.py";
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "python", "version": 1,
            "text": "import frappe\n\ndoc = frappe.get_doc(\"Task\")\n"
        }}),
    );
    session.notify(
        "textDocument/didChange",
        json!({
            "textDocument": {"uri": uri, "version": 2},
            "contentChanges": [{
                "range": {"start": {"line": 2, "character": 22},
                          "end": {"line": 2, "character": 26}},
                "text": "Sales Invoice"
            }]
        }),
    );
    session.notify(
        "workspace/didChangeConfiguration",
        json!({"settings": {"frappe-lsp": {"hover": {"enabled": false}}}}),
    );

    let position = json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 24}});
    for (id, method, params) in [
        (2, "textDocument/hover", position.clone()),
        (3, "textDocument/definition", position.clone()),
        (4, "textDocument/typeDefinition", position.clone()),
        (5, "textDocument/implementation", position.clone()),
        (
            6,
            "textDocument/completion",
            json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 24},
                   "context": {"triggerKind": 1}}),
        ),
        (
            7,
            "textDocument/references",
            json!({"textDocument": {"uri": uri}, "position": {"line": 2, "character": 24},
                   "context": {"includeDeclaration": true}}),
        ),
        (
            8,
            "textDocument/documentSymbol",
            json!({"textDocument": {"uri": uri}}),
        ),
        (9, "workspace/symbol", json!({"query": "Task"})),
        (
            10,
            "textDocument/codeAction",
            json!({"textDocument": {"uri": uri},
                   "range": {"start": {"line": 2, "character": 0},
                             "end": {"line": 2, "character": 0}},
                   "context": {"diagnostics": []}}),
        ),
        (
            11,
            "textDocument/diagnostic",
            json!({"textDocument": {"uri": uri}}),
        ),
    ] {
        let response = session.request(id, method, params);
        assert!(
            response.get("error").is_none(),
            "{method} returned {response}"
        );
    }

    session.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": uri}}),
    );

    let shutdown = session.request(12, "shutdown", json!(null));
    assert!(shutdown.get("error").is_none(), "{shutdown}");
    session.notify("exit", json!(null));

    let stderr = session.finish();
    assert!(stderr.contains("initializing"), "logs missing: {stderr}");
    assert!(stderr.contains("shutting down"), "logs missing: {stderr}");
}

#[test]
fn a_config_file_carrying_a_credential_is_reported_and_ignored() {
    let root = std::env::temp_dir().join("frappe-lsp-credential-test");
    std::fs::create_dir_all(&root).expect("temp dir");
    std::fs::write(
        root.join(".frappe-lsp.toml"),
        "[bridge]\nenabled = true\napi_secret = \"ff5c1a9e0b7d43a6\"\n",
    )
    .expect("write config");

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", root.display()));
    let response = session.request(1, "initialize", params);

    // The bad file costs the user the file, never the session.
    assert!(response.get("error").is_none(), "{response}");
    session.notify("initialized", json!({}));

    let logged = loop {
        let message = session.receive();
        if message["method"] == "window/logMessage" {
            break message;
        }
    };
    assert_eq!(logged["params"]["type"], 1, "logged as an error");
    let text = logged["params"]["message"].as_str().expect("message text");
    assert!(text.contains("bridge.api_secret"), "{text}");
    assert!(text.contains(".frappe-lsp.toml"), "{text}");

    session.request(2, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();

    std::fs::remove_dir_all(&root).expect("cleanup");
}
