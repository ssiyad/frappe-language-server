//! `cargo xtask stress` — the shipped binary, driven like an editor over a
//! real bench, looking for the three ways a language server fails its user:
//! it exits, it stops answering, or it answers with an error.
//!
//! Every file in the corpus is opened, asked about at sampled positions, and
//! then typed into: the half-finished states an editor sends on every
//! keystroke are the ones a corpus of finished code never shows a server.

mod client;
mod hostile;
mod text;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use frappe_index::Index;
use serde_json::{Value, json};

use client::{Client, Failure};
use text::Rng;

pub struct Options {
    pub bench: PathBuf,
    pub binary: PathBuf,
    pub log: PathBuf,
    /// Positions asked about per file.
    pub positions: usize,
    /// Edits typed into each file.
    pub edits: usize,
    /// Files driven, spread evenly over the corpus; 0 for all of them.
    pub limit: usize,
    pub seed: u64,
    /// Every installed app rather than `frappe` and `erpnext` alone, as smoke.
    pub all: bool,
}

/// Run the drive. Returns whether it was clean.
pub fn run(options: &Options) -> std::io::Result<bool> {
    let index = Index::build(&options.bench);
    let mut files = crate::smoke::files(&index, options.all, true);
    if options.limit > 0 && files.len() > options.limit {
        let stride = files.len() / options.limit;
        files = files.into_iter().step_by(stride).collect();
    }
    drop(index);

    let mut client = Client::spawn(&options.binary, &options.log, Duration::from_secs(20))?;
    let mut drive = Drive {
        rng: Rng::new(options.seed),
        latencies: BTreeMap::new(),
        slowest: BTreeMap::new(),
        errors: BTreeMap::new(),
    };
    let started = Instant::now();
    let outcome = drive.session(&mut client, options, &files);
    let alive = client.alive();
    if outcome.is_ok() {
        client.shutdown();
    }

    let log = std::fs::read_to_string(&options.log).unwrap_or_default();
    let panics: Vec<&str> = log
        .lines()
        .filter(|line| line.contains("panicked"))
        .collect();
    drive.report(&files, started.elapsed(), &panics);

    match outcome {
        Ok(()) => Ok(drive.errors.is_empty() && panics.is_empty()),
        Err((failure, context)) => {
            println!(
                "\nFAILED: {failure} {}during {context}",
                if alive { "" } else { "(process gone) " }
            );
            let tail: Vec<&str> = log.lines().rev().take(30).collect();
            for line in tail.into_iter().rev() {
                println!("  | {line}");
            }
            Ok(false)
        }
    }
}

struct Drive {
    rng: Rng,
    latencies: BTreeMap<String, Vec<Duration>>,
    /// The slowest request of each method, and where it was.
    slowest: BTreeMap<String, (Duration, String)>,
    /// `method: message` → how often.
    errors: BTreeMap<String, usize>,
}

type Stop = (Failure, String);

impl Drive {
    fn session(
        &mut self,
        client: &mut Client,
        options: &Options,
        files: &[PathBuf],
    ) -> Result<(), Stop> {
        let root = format!("file://{}", options.bench.display());
        self.call(client, "initialize", initialize(&root), "initialize")?;
        notify(client, "initialized", json!({}), "initialized")?;
        self.settle(client)?;
        if let Some(package) = files.first().and_then(|file| package_of(file)) {
            self.hostile(client, &package)?;
        }

        for (number, path) in files.iter().enumerate() {
            if number % 250 == 0 {
                println!(
                    "  {number:5}/{}  {}",
                    files.len(),
                    path.strip_prefix(&options.bench).unwrap_or(path).display()
                );
            }
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            self.file(client, options, path, &source)?;
        }

        self.call(
            client,
            "workspace/symbol",
            json!({"query": "Sales Invoice"}),
            "workspace/symbol",
        )?;
        self.call(
            client,
            "workspace/symbol",
            json!({"query": ""}),
            "workspace/symbol",
        )?;
        self.call(client, "frappe-lsp/reindex", Value::Null, "reindex")?;
        self.settle(client)
    }

    /// One file: open it, ask about it, type into it, put it back, close it.
    fn file(
        &mut self,
        client: &mut Client,
        options: &Options,
        path: &Path,
        source: &str,
    ) -> Result<(), Stop> {
        let uri = format!("file://{}", path.display());
        let document = json!({"uri": uri});
        let context = |what: &str, at: Option<usize>| match at {
            Some(at) => format!("{what} at byte {at} of {}", path.display()),
            None => format!("{what} on {}", path.display()),
        };

        notify(
            client,
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": language(path), "version": 1, "text": source,
            }}),
            &context("didOpen", None),
        )?;
        for method in [
            "textDocument/documentSymbol",
            "textDocument/codeLens",
            "textDocument/diagnostic",
        ] {
            self.call(
                client,
                method,
                json!({"textDocument": document}),
                &context(method, None),
            )?;
        }

        let all = text::candidates(source);
        for (number, at) in text::sample(&all, options.positions, &mut self.rng)
            .into_iter()
            .enumerate()
        {
            self.ask(
                client,
                &document,
                source,
                at,
                number % 4 == 0,
                &context("a position", Some(at)),
            )?;
        }

        let mut current = source.to_string();
        for version in 2..2 + options.edits {
            let candidates = text::candidates(&current);
            let at = candidates[self.rng.below(candidates.len())];
            let edit = text::edit(&current, at, &mut self.rng);
            notify(
                client,
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": version},
                    "contentChanges": [edit.change],
                }),
                &context("didChange", Some(at)),
            )?;
            current = edit.text;
            self.ask(
                client,
                &document,
                &current,
                edit.cursor,
                false,
                &context("an edit", Some(at)),
            )?;
            self.call(
                client,
                "textDocument/diagnostic",
                json!({"textDocument": document}),
                &context("diagnostic after an edit", Some(at)),
            )?;
        }

        notify(
            client,
            "textDocument/didChange",
            json!({
                "textDocument": {"uri": uri, "version": 2 + options.edits},
                "contentChanges": [{"text": source}],
            }),
            &context("a full replace", None),
        )?;
        notify(
            client,
            "textDocument/didClose",
            json!({"textDocument": document}),
            &context("didClose", None),
        )
    }

    /// Everything an editor asks about one cursor position.
    fn ask(
        &mut self,
        client: &mut Client,
        document: &Value,
        source: &str,
        at: usize,
        references: bool,
        context: &str,
    ) -> Result<(), Stop> {
        let position = text::position(source, at);
        let params = json!({"textDocument": document, "position": position});
        for method in [
            "textDocument/hover",
            "textDocument/definition",
            "textDocument/completion",
        ] {
            self.call(client, method, params.clone(), context)?;
        }
        if references {
            let mut params = params;
            params["context"] = json!({"includeDeclaration": true});
            self.call(client, "textDocument/references", params, context)?;
        }
        Ok(())
    }

    fn call(
        &mut self,
        client: &mut Client,
        method: &str,
        params: Value,
        context: &str,
    ) -> Result<Value, Stop> {
        let (response, elapsed) = client
            .request(method, params)
            .map_err(|failure| (failure, format!("{method} — {context}")))?;
        self.latencies
            .entry(method.to_string())
            .or_default()
            .push(elapsed);
        let slowest = self
            .slowest
            .entry(method.to_string())
            .or_insert((Duration::ZERO, String::new()));
        if elapsed > slowest.0 {
            *slowest = (elapsed, context.to_string());
        }
        if let Some(error) = response.get("error") {
            let message = error["message"].as_str().unwrap_or("?");
            *self
                .errors
                .entry(format!("{method}: {message}"))
                .or_default() += 1;
            if self.errors.values().sum::<usize>() <= 5 {
                println!("  error: {method}: {message} — {context}");
            }
        }
        Ok(response)
    }

    /// Wait for the index to publish, as a user would before trusting it.
    fn settle(&mut self, client: &mut Client) -> Result<(), Stop> {
        for _ in 0..600 {
            let status = self.call(
                client,
                "frappe-lsp/indexStatus",
                Value::Null,
                "waiting for the index",
            )?;
            if status["result"]["confidence"] != "partial" {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Err((Failure::Hung, "the index never published".to_string()))
    }

    fn report(&self, files: &[PathBuf], elapsed: Duration, panics: &[&str]) {
        let total: usize = self.latencies.values().map(Vec::len).sum();
        println!("\n{} files, {total} requests in {elapsed:.1?}", files.len());
        println!(
            "  {:32} {:>8} {:>10} {:>10} {:>10}",
            "method", "count", "p50", "p99", "max"
        );
        for (method, samples) in &self.latencies {
            let mut sorted = samples.clone();
            sorted.sort_unstable();
            let at = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            println!(
                "  {method:32} {:>8} {:>10.1?} {:>10.1?} {:>10.1?}",
                sorted.len(),
                at(0.5),
                at(0.99),
                at(1.0)
            );
        }
        println!("\nslowest, where over 100 ms:");
        for (method, (elapsed, context)) in &self.slowest {
            if *elapsed > Duration::from_millis(100) {
                println!("  {method:32} {elapsed:>10.1?}  {context}");
            }
        }
        if !self.errors.is_empty() {
            println!("\nerror responses:");
            for (error, count) in &self.errors {
                println!("  {count:6}  {error}");
            }
        }
        if !panics.is_empty() {
            println!(
                "\n{} panics in the server log, first: {}",
                panics.len(),
                panics[0]
            );
        }
    }
}

fn notify(client: &mut Client, method: &str, params: Value, context: &str) -> Result<(), Stop> {
    client
        .notify(method, params)
        .map_err(|failure| (failure, format!("{method} — {context}")))
}

/// The app package a file sits in: the directory holding its `hooks.py`.
fn package_of(file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .find(|directory| directory.join("hooks.py").is_file())
        .map(Path::to_path_buf)
}

fn language(path: &Path) -> &'static str {
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("py") => "python",
        Some("js") => "javascript",
        Some("json") => "json",
        Some("html") => "html",
        _ => "plaintext",
    }
}

/// What Neovim offers, near enough: UTF-16 only, pull diagnostics, no cache
/// so a stress run leaves nothing in the bench.
fn initialize(root: &str) -> Value {
    json!({
        "processId": null,
        "clientInfo": {"name": "xtask-stress"},
        "rootUri": root,
        "initializationOptions": {"index": {"cache": false}},
        "capabilities": {
            "general": {"positionEncodings": ["utf-16"]},
            "workspace": {"configuration": true, "workspaceFolders": true, "symbol": {}},
            "textDocument": {
                "synchronization": {"didSave": true},
                "definition": {"linkSupport": true},
                "hover": {"contentFormat": ["markdown", "plaintext"]},
                "diagnostic": {},
                "codeLens": {},
                "completion": {"completionItem": {"labelDetailsSupport": true, "snippetSupport": true}}
            }
        }
    })
}
