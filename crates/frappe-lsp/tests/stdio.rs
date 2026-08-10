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

    /// Wait for the response to `id`, skipping anything else.
    fn await_id(&mut self, id: i64) -> Value {
        loop {
            let message = self.receive();
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

fn fixture_bench() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("tests/fixtures/bench")
}

#[test]
fn the_index_builds_in_the_background_and_reports_itself() {
    let bench = fixture_bench();
    let mut session = Session::start();

    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    let response = session.request(1, "initialize", params);
    assert!(response.get("error").is_none(), "{response}");

    // Before `initialized`, nothing has been built and the server says so
    // rather than blocking — specs/02.
    let before = session.request(2, "frappe-lsp/indexStatus", json!(null))["result"].clone();
    assert_eq!(before["confidence"], "partial");
    assert_eq!(before["doctypes"], 0);

    session.notify("initialized", json!({}));

    // The build runs off the request path, so poll rather than assume.
    let mut status = Value::Null;
    for attempt in 0..100 {
        status =
            session.request(10 + attempt, "frappe-lsp/indexStatus", json!(null))["result"].clone();
        if status["confidence"] == "static" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert_eq!(
        status["confidence"], "static",
        "index never finished: {status}"
    );
    assert_eq!(status["apps"], 3);
    assert_eq!(status["doctypes"], 8, "{status}");
    assert_eq!(status["controllers"], 5);
    assert_eq!(status["whitelistedMethods"], 5);
    assert_eq!(status["frappeVersion"], "17.0.0-dev");
    // Library Availability twice, and the colliding custom field.
    assert_eq!(status["conflicts"], 2);
    // The deliberately malformed DocType JSON, and nothing else.
    assert_eq!(status["problems"], 1);

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_forced_reindex_starts_over() {
    let bench = fixture_bench();
    let mut session = Session::start();

    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));

    let response = session.request(2, "frappe-lsp/reindex", json!(null));
    assert!(response.get("error").is_none(), "{response}");
    assert_eq!(response["result"]["started"], true);

    let mut status = Value::Null;
    for attempt in 0..100 {
        status =
            session.request(10 + attempt, "frappe-lsp/indexStatus", json!(null))["result"].clone();
        if status["confidence"] == "static" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(status["doctypes"], 8, "{status}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn the_bridge_says_it_is_off_rather_than_missing() {
    let mut session = Session::start();
    session.request(1, "initialize", initialize_params());
    session.notify("initialized", json!({}));

    let response = session.request(2, "frappe-lsp/refreshBridge", json!(null));
    let error = response.get("error").expect("an error, not a result");
    // -32601 is method_not_found, which would read as "this server is broken".
    assert_ne!(error["code"], -32601, "{error}");
    assert!(
        error["message"]
            .as_str()
            .is_some_and(|message| message.contains("bridge")),
        "{error}"
    );

    session.request(3, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).expect("destination");
    for entry in std::fs::read_dir(from).expect("readable source").flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target);
        } else {
            std::fs::copy(&source, &target).expect("copy");
        }
    }
}

/// Poll `indexStatus` until the background build settles.
fn settled(session: &mut Session, first_id: i64) -> Value {
    let mut status = Value::Null;
    for attempt in 0..100 {
        status =
            session.request(first_id + attempt, "frappe-lsp/indexStatus", json!(null))["result"]
                .clone();
        if status["confidence"] == "static" {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    status
}

#[test]
fn saving_a_doctype_patches_the_graph_without_a_rebuild() {
    let bench = std::env::temp_dir().join(format!("frappe-lsp-save-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&bench);
    copy_tree(&fixture_bench(), &bench);

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));

    let before = settled(&mut session, 10);
    assert_eq!(before["doctypes"], 8, "{before}");
    let fields_before = before["fields"].as_u64().expect("a field count");

    // Add a DocType the way `bench new-doctype` would, then tell the server the
    // file was saved. Adding one is the observable case: a count that moves
    // cannot be confused with a save that was quietly ignored.
    let directory = bench.join("apps/library/library/library/doctype/library_shelf");
    std::fs::create_dir_all(&directory).expect("doctype directory");
    let path = directory.join("library_shelf.json");
    std::fs::write(
        &path,
        r#"{
 "actions": [],
 "creation": "2026-01-14 09:12:44.201375",
 "doctype": "DocType",
 "engine": "InnoDB",
 "field_order": [
  "shelf_code"
 ],
 "fields": [
  {
   "fieldname": "shelf_code",
   "fieldtype": "Data",
   "label": "Shelf Code"
  }
 ],
 "links": [],
 "modified": "2026-02-03 11:47:02.884913",
 "modified_by": "Administrator",
 "module": "Library",
 "name": "Library Shelf",
 "owner": "Administrator",
 "permissions": [],
 "sort_field": "creation",
 "sort_order": "DESC",
 "states": []
}
"#,
    )
    .expect("write the new DocType");

    session.notify(
        "textDocument/didSave",
        json!({"textDocument": {"uri": format!("file://{}", path.display())}}),
    );

    // The re-index runs off the request path too, so poll for its effect.
    let mut after = Value::Null;
    for attempt in 0..100 {
        after =
            session.request(300 + attempt, "frappe-lsp/indexStatus", json!(null))["result"].clone();
        if after["doctypes"] == 9 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }

    assert_eq!(
        after["doctypes"], 9,
        "the save was never picked up: {after}"
    );
    assert_eq!(
        after["fields"].as_u64().expect("a field count"),
        fields_before + 1
    );
    // A patch, not a rebuild: nothing else moved.
    assert_eq!(after["apps"], before["apps"]);
    assert_eq!(after["problems"], before["problems"]);
    assert_eq!(after["conflicts"], before["conflicts"]);

    session.request(500, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();

    std::fs::remove_dir_all(&bench).expect("cleanup");
}

/// Open a buffer in a bench and return the session, ready for requests.
fn session_on(bench: &std::path::Path, relative: &str, text: &str) -> (Session, String) {
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let path = bench.join(relative);
    let uri = format!("file://{}", path.display());
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "python", "version": 1, "text": text
        }}),
    );
    (session, uri)
}

/// The position of `|` in a source, with the marker removed — the cursor
/// convention of specs/32's golden tests.
fn cursor(source: &str) -> (String, Value) {
    let offset = source.find('|').expect("a cursor marker");
    let before = &source[..offset];
    let line = before.matches('\n').count();
    let character = before.len() - before.rfind('\n').map_or(0, |at| at + 1);
    (
        source.replace('|', ""),
        json!({"line": line, "character": character}),
    )
}

fn definition_at(session: &mut Session, id: i64, uri: &str, position: &Value) -> Value {
    let response = session.request(
        id,
        "textDocument/definition",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    assert!(response.get("error").is_none(), "{response}");
    response["result"].clone()
}

#[test]
fn definition_on_a_doctype_literal_lands_on_its_controller() {
    let source = "import frappe\n\ndoc = frappe.get_doc(\"Library Me|mber\", name)\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let result = definition_at(&mut session, 100, &uri, &position);
    let links = result.as_array().expect("LocationLink[]");

    // Spec 21's order: controller, then the JSON, then the form script.
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("library_member.py"),
        "{links:?}"
    );
    assert!(
        links[1]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("library_member.json"),
        "{links:?}"
    );

    // The origin highlights the name, not the quotes: the literal starts at
    // character 21, so the interior starts at 22.
    let origin = &links[0]["originSelectionRange"];
    assert_eq!(origin["start"]["character"], 22);
    assert_eq!(origin["end"]["character"], 36);

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn definition_on_a_field_lands_inside_the_doctype_json() {
    let source = "import frappe\nfrappe.get_all(\"Library Member\", pluck=\"emai|l_id\")\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let result = definition_at(&mut session, 100, &uri, &position);
    let links = result.as_array().expect("LocationLink[]");
    assert_eq!(links.len(), 1);

    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("library_member.json"),
        "{links:?}"
    );
    // The field's own object, not the top of the file.
    assert!(
        links[0]["targetRange"]["start"]["line"]
            .as_u64()
            .expect("line")
            > 0
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn definition_says_nothing_on_a_dynamic_argument() {
    let source = "import frappe\nfrappe.get_doc(doct|ype, name)\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let result = definition_at(&mut session, 100, &uri, &position);
    assert!(result.is_null(), "{result}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn hover_describes_a_doctype() {
    let source = "import frappe\nfrappe.get_doc(\"Library Mem|bership\")\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    let contents = &response["result"]["contents"];
    assert_eq!(contents["kind"], "markdown");

    let value = contents["value"].as_str().expect("a body");
    assert!(value.contains("Library Membership"), "{value}");
    assert!(value.contains("Submittable"), "{value}");
    // Spec 22: never more than ~15 lines, because the popup is small.
    assert!(value.lines().count() <= 15, "{value}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn hover_on_a_field_states_where_a_custom_field_came_from() {
    // `loyalty_points` exists only in the fixtures file, so grep for it in the
    // schema and you find nothing — spec 22 makes hover say so.
    let source = "import frappe\nfrappe.get_all(\"Library Member\", pluck=\"loyalty_po|ints\")\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    let value = response["result"]["contents"]["value"]
        .as_str()
        .expect("a body");
    assert!(value.contains("Custom Field"), "{value}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn hover_says_nothing_where_nothing_resolved() {
    let source = "import frappe\nfrappe.get_doc(doct|ype, name)\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    assert!(response["result"].is_null(), "{response}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn completion_offers_doctypes_and_replaces_only_the_string_interior() {
    let source = "import frappe\nfrappe.get_doc(\"Library |\")\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/completion",
        json!({"textDocument": {"uri": uri}, "position": position,
               "context": {"triggerKind": 1}}),
    );
    let items = response["result"]["items"].as_array().expect("items");
    assert!(!items.is_empty(), "{response}");

    let labels: Vec<&str> = items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();
    assert!(labels.contains(&"Library Member"), "{labels:?}");
    assert!(labels[0].starts_with("Library "), "{labels:?}");

    // The edit covers the interior: the literal opens at character 15, so the
    // replacement starts at 16 and the quotes survive.
    let edit = &items[0]["textEdit"];
    assert_eq!(edit["range"]["start"]["character"], 16);
    assert_eq!(edit["newText"], labels[0]);

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn completion_offers_the_fields_of_the_resolved_doctype() {
    let source = "import frappe\nfrappe.get_all(\"Library Member\", fields=[\"|\"])\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/completion",
        json!({"textDocument": {"uri": uri}, "position": position,
               "context": {"triggerKind": 1}}),
    );
    let items = response["result"]["items"].as_array().expect("items");
    let labels: Vec<&str> = items
        .iter()
        .filter_map(|item| item["label"].as_str())
        .collect();

    assert!(labels.contains(&"member_name"), "{labels:?}");
    // Layer 2 is merged, so the Custom Field is offered too.
    assert!(labels.contains(&"loyalty_points"), "{labels:?}");

    // Implicit fields are valid and almost never what is being typed, so they
    // rank last (spec 20).
    let implicit = labels
        .iter()
        .position(|label| *label == "name")
        .expect("name");
    let declared = labels
        .iter()
        .position(|label| *label == "member_name")
        .expect("member_name");
    assert!(declared < implicit, "{labels:?}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn completion_resolve_fills_documentation_lazily() {
    let source = "import frappe\nfrappe.get_doc(\"Library |\")\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/completion",
        json!({"textDocument": {"uri": uri}, "position": position,
               "context": {"triggerKind": 1}}),
    );
    let item = response["result"]["items"][0].clone();
    // Building markdown for every candidate on every keystroke blows the
    // budget, so it is absent until asked for (spec 20).
    assert!(item["documentation"].is_null(), "{item}");

    let resolved = session.request(101, "completionItem/resolve", item);
    let documentation = &resolved["result"]["documentation"];
    assert_eq!(documentation["kind"], "markdown");
    assert!(
        documentation["value"]
            .as_str()
            .expect("body")
            .contains("fields"),
        "{documentation}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn type_definition_on_a_field_goes_to_the_doctype_json() {
    let source = "import frappe\nfrappe.get_all(\"Library Member\", pluck=\"membe|r_name\")\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    let response = session.request(
        100,
        "textDocument/typeDefinition",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    let links = response["result"].as_array().expect("LocationLink[]");
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("library_member.json"),
        "{links:?}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

fn reference_bench() -> Option<std::path::PathBuf> {
    let bench = std::env::var_os("FRAPPE_LSP_REFERENCE_BENCH")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join("sources/frappe-dev"))
        })?;
    bench.join("sites").is_dir().then_some(bench)
}

/// The "done when" of task 008, over stdio against a real bench.
///
/// Budgets are spec 31's ceilings — 30 ms for definition and hover, 50 ms for
/// completion — measured as the client sees them, request to response, with
/// the index already built.
#[test]
fn navigation_works_on_the_reference_bench_inside_its_budgets() {
    let Some(bench) = reference_bench() else {
        eprintln!("skipped: no reference bench (set FRAPPE_LSP_REFERENCE_BENCH)");
        return;
    };

    // A DocType from frappe, and one from a custom app in the same bench.
    let source = "import frappe\n\
                  a = frappe.get_doc(\"User\", name)\n\
                  b = frappe.get_all(\"Vault Node\", fields=[\"node_name\"])\n\
                  c = frappe.get_all(\"User\", pluck=\"full_name\")\n";
    let (mut session, uri) = session_on(&bench, "apps/frappe/frappe/probe.py", source);

    // The median of several samples, not one. A budget describes what the
    // server does; a single sample also describes whatever else the machine
    // was doing at that instant, and this suite runs beside a compiler.
    let timed = |session: &mut Session, id: i64, method: &str, params: Value| {
        let mut elapsed = Vec::new();
        let mut response = Value::Null;
        for attempt in 0..5 {
            let started = std::time::Instant::now();
            response = session.request(id + attempt, method, params.clone());
            elapsed.push(started.elapsed());
        }
        elapsed.sort_unstable();
        (response, elapsed[elapsed.len() / 2])
    };

    let position = |line: u32, character: u32| json!({"line": line, "character": character});
    let document = json!({"uri": uri});

    // Definition on a frappe DocType.
    let (response, definition_frappe) = timed(
        &mut session,
        100,
        "textDocument/definition",
        json!({"textDocument": document, "position": position(1, 22)}),
    );
    let links = response["result"].as_array().expect("LocationLink[]");
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("core/doctype/user/user.py"),
        "{links:?}"
    );

    // Definition on a custom app's DocType, in the same session.
    let (response, definition_custom) = timed(
        &mut session,
        110,
        "textDocument/definition",
        json!({"textDocument": document, "position": position(2, 22)}),
    );
    let links = response["result"].as_array().expect("LocationLink[]");
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("vault_node.py"),
        "{links:?}"
    );

    let (response, hover) = timed(
        &mut session,
        120,
        "textDocument/hover",
        json!({"textDocument": document, "position": position(3, 22)}),
    );
    assert!(
        response["result"]["contents"]["value"]
            .as_str()
            .expect("a hover body")
            .contains("User"),
        "{response}"
    );

    let (response, completion) = timed(
        &mut session,
        130,
        "textDocument/completion",
        json!({"textDocument": document, "position": position(3, 40),
               "context": {"triggerKind": 1}}),
    );
    assert!(
        !response["result"]["items"]
            .as_array()
            .expect("items")
            .is_empty(),
        "{response}"
    );

    eprintln!(
        "definition {definition_frappe:?} (frappe) / {definition_custom:?} (custom app), \
         hover {hover:?}, completion {completion:?}"
    );

    // Spec 31's ceilings. The targets are 10/10/20 ms; these are the numbers a
    // regression would have to cross before a user notices.
    assert!(definition_frappe.as_millis() < 30, "{definition_frappe:?}");
    assert!(definition_custom.as_millis() < 30, "{definition_custom:?}");
    assert!(hover.as_millis() < 30, "{hover:?}");
    assert!(completion.as_millis() < 50, "{completion:?}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_cancelled_request_is_answered_as_cancelled() {
    // Handled by the transport, per spec 31's cancellation rule — asserted
    // here because "the framework does it" is a claim, not a test.
    let mut session = Session::start();
    session.request(1, "initialize", initialize_params());
    session.notify("initialized", json!({}));

    session.send(json!({
        "jsonrpc": "2.0", "id": 50, "method": "textDocument/hover",
        "params": {"textDocument": {"uri": "file:///tmp/none.py"},
                   "position": {"line": 0, "character": 0}}
    }));
    session.notify("$/cancelRequest", json!({"id": 50}));

    let response = session.await_id(50);
    // Either the answer arrived first or it was cancelled; both are correct,
    // and a request that never gets a response is not.
    if let Some(error) = response.get("error") {
        assert_eq!(error["code"], -32800, "{error}");
    }

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}
