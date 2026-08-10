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

/// The "done when" of task 009's watcher half: a DocType edited outside the
/// editor is reflected without a restart.
///
/// The client here registers no watcher and sends no `didSave`, which is
/// exactly the case the `notify` fallback exists for — `git checkout`,
/// `bench migrate`, a second editor.
#[test]
fn a_doctype_created_outside_the_editor_is_picked_up() {
    let bench = std::env::temp_dir().join(format!("frappe-lsp-watch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&bench);
    copy_tree(&fixture_bench(), &bench);

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));

    let before = settled(&mut session, 10);
    assert_eq!(before["doctypes"], 8, "{before}");

    // Written straight to disk. The server is told nothing.
    let directory = bench.join("apps/library/library/library/doctype/library_shelf");
    std::fs::create_dir_all(&directory).expect("doctype directory");
    std::fs::write(
        directory.join("library_shelf.json"),
        concat!(
            "{\n \"actions\": [],\n \"creation\": \"2026-01-14 09:12:44.201375\",\n",
            " \"doctype\": \"DocType\",\n \"engine\": \"InnoDB\",\n",
            " \"field_order\": [\n  \"shelf_code\"\n ],\n",
            " \"fields\": [\n  {\n   \"fieldname\": \"shelf_code\",\n",
            "   \"fieldtype\": \"Data\",\n   \"label\": \"Shelf Code\"\n  }\n ],\n",
            " \"links\": [],\n \"modified\": \"2026-02-03 11:47:02.884913\",\n",
            " \"modified_by\": \"Administrator\",\n \"module\": \"Library\",\n",
            " \"name\": \"Library Shelf\",\n \"owner\": \"Administrator\",\n",
            " \"permissions\": [],\n \"sort_field\": \"creation\",\n",
            " \"sort_order\": \"DESC\",\n \"states\": []\n}\n"
        ),
    )
    .expect("write the new DocType");

    // 250 ms of debounce plus the platform's own latency.
    let mut after = Value::Null;
    for attempt in 0..80 {
        after =
            session.request(300 + attempt, "frappe-lsp/indexStatus", json!(null))["result"].clone();
        if after["doctypes"] == 9 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        after["doctypes"], 9,
        "the filesystem change never landed: {after}"
    );

    // And removing it takes the DocType back out.
    std::fs::remove_file(directory.join("library_shelf.json")).expect("remove");
    let mut removed = Value::Null;
    for attempt in 0..80 {
        removed =
            session.request(400 + attempt, "frappe-lsp/indexStatus", json!(null))["result"].clone();
        if removed["doctypes"] == 8 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert_eq!(
        removed["doctypes"], 8,
        "the deletion never landed: {removed}"
    );

    session.request(500, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();

    std::fs::remove_dir_all(&bench).expect("cleanup");
}

/// Installing an app while the editor is open.
///
/// `library-old` sits in the fixture bench uninstalled, defining two DocTypes
/// the installed apps also define. Adding it to `apps.txt` changes the app set
/// and the merge order behind it, which no per-file patch expresses — the
/// server has to notice and rebuild.
#[test]
fn installing_an_app_is_noticed_without_a_restart() {
    let bench = std::env::temp_dir().join(format!("frappe-lsp-install-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&bench);
    copy_tree(&fixture_bench(), &bench);

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));

    let before = settled(&mut session, 10);
    assert_eq!(before["apps"], 3, "{before}");
    assert_eq!(before["conflicts"], 2, "{before}");

    // `bench install-app` writes both, and apps.json is the authoritative one
    // (spec 03), so editing only apps.txt would correctly change nothing.
    let apps_txt = bench.join("sites/apps.txt");
    let text = std::fs::read_to_string(&apps_txt).expect("apps.txt");
    std::fs::write(&apps_txt, format!("{text}\nlibrary_archive")).expect("write");

    let apps_json = bench.join("sites/apps.json");
    let mut installed: Value =
        serde_json::from_str(&std::fs::read_to_string(&apps_json).expect("apps.json"))
            .expect("valid JSON");
    installed["library_archive"] = json!({
        "idx": 4, "version": "0.0.9", "required": [],
        "resolution": {"commit_hash": null, "branch": "main"}
    });
    std::fs::write(
        &apps_json,
        serde_json::to_string_pretty(&installed).expect("json"),
    )
    .expect("write");

    let mut after = Value::Null;
    for attempt in 0..80 {
        after =
            session.request(300 + attempt, "frappe-lsp/indexStatus", json!(null))["result"].clone();
        if after["apps"] == 4 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    assert_eq!(after["apps"], 4, "the install was never noticed: {after}");
    // Its two DocTypes now collide with the installed ones, which is exactly
    // what indexing a fourth app is supposed to mean.
    assert!(
        after["conflicts"].as_u64().expect("conflicts") > 2,
        "{after}"
    );

    session.request(500, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();

    std::fs::remove_dir_all(&bench).expect("cleanup");
}

/// Open an app's real `hooks.py` from the fixture bench.
fn hooks_session(app: &str) -> (Session, String, String) {
    let bench = fixture_bench();
    let relative = format!("apps/{app}/hooks.py");
    let path = bench.join(&relative);
    let text = std::fs::read_to_string(&path).expect("a hooks.py");

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let uri = format!("file://{}", path.display());
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "python", "version": 1, "text": text
        }}),
    );
    (session, uri, text)
}

/// The position of `needle` in `text`, offset by `within`.
fn position_of(text: &str, needle: &str, within: usize) -> Value {
    let at = text.find(needle).expect("the needle is in the file") + within;
    let before = &text[..at];
    json!({
        "line": before.matches('\n').count(),
        "character": before.len() - before.rfind('\n').map_or(0, |n| n + 1),
    })
}

#[test]
fn definition_on_a_doc_events_handler_lands_on_the_function() {
    let (mut session, uri, text) = hooks_session("library/library");
    let position = position_of(&text, "library.events.validate_member", 4);

    let result = definition_at(&mut session, 100, &uri, &position);
    let links = result.as_array().expect("LocationLink[]");
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("events.py"),
        "{links:?}"
    );
    // The function, not the top of the file.
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
fn definition_on_a_doc_events_doctype_key_lands_on_the_doctype() {
    let (mut session, uri, text) = hooks_session("library/library");
    // The DocType key of the doc_events map, not the DocType anywhere else.
    let at = text.find("doc_events").expect("doc_events");
    let key = text[at..].find("\"Library Member\"").expect("the key") + at + 2;
    let before = &text[..key];
    let position = json!({
        "line": before.matches('\n').count(),
        "character": before.len() - before.rfind('\n').map_or(0, |n| n + 1),
    });

    let result = definition_at(&mut session, 100, &uri, &position);
    let links = result.as_array().expect("LocationLink[]");
    assert!(
        links.iter().any(|link| link["targetUri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("library_member.json"))),
        "{links:?}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn hover_on_a_doc_event_says_when_it_fires() {
    let (mut session, uri, text) = hooks_session("library/library");
    let position = position_of(&text, "\"on_update\"", 2);

    let response = session.request(
        100,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    let value = response["result"]["contents"]["value"]
        .as_str()
        .expect("a body");
    assert!(value.contains("on_update"), "{value}");
    assert!(value.contains("After every write"), "{value}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn hover_on_a_hook_key_names_its_schema() {
    let (mut session, uri, text) = hooks_session("library/library");
    let position = position_of(&text, "override_doctype_class", 4);

    let response = session.request(
        100,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    let value = response["result"]["contents"]["value"]
        .as_str()
        .expect("a body");
    assert!(value.contains("override_doctype_class"), "{value}");
    assert!(value.contains("replaces its controller"), "{value}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_wrong_doc_event_and_a_dead_submit_event_are_named() {
    // circulation's hooks are deliberately wrong — `on_validate` is not an
    // event, and `on_submit` on a non-submittable DocType never fires.
    let (mut session, uri, text) = hooks_session("circulation-app/circulation");

    let response = session.request(
        100,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri},
               "position": position_of(&text, "\"on_validate\"", 2)}),
    );
    let value = response["result"]["contents"]["value"]
        .as_str()
        .expect("body");
    assert!(value.contains("never runs"), "{value}");

    let response = session.request(
        101,
        "textDocument/hover",
        json!({"textDocument": {"uri": uri},
               "position": position_of(&text, "\"on_submit\"", 2)}),
    );
    let value = response["result"]["contents"]["value"]
        .as_str()
        .expect("body");
    assert!(value.contains("not submittable"), "{value}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn implementation_on_a_doctype_lists_what_replaces_its_controller() {
    let source = "import frappe\nfrappe.get_doc(\"Library Mem|ber\", name)\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = session_on(&fixture_bench(), "apps/library/library/probe.py", &text);

    // Spec 21: `definition` goes to the DocType's own controller, and the
    // class that replaces it answers `implementation` instead.
    let response = session.request(
        100,
        "textDocument/implementation",
        json!({"textDocument": {"uri": uri}, "position": position}),
    );
    let links = response["result"].as_array().expect("LocationLink[]");
    assert_eq!(links.len(), 1, "{links:?}");
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("overrides.py"),
        "{links:?}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

/// Open a JavaScript buffer in a bench.
fn js_session(bench: &std::path::Path, relative: &str, text: &str) -> (Session, String) {
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let uri = format!("file://{}", bench.join(relative).display());
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "javascript", "version": 1, "text": text
        }}),
    );
    (session, uri)
}

#[test]
fn definition_on_a_doctype_in_a_form_script_works() {
    let source = "frappe.ui.form.on(\"Library Mem|ber\", {\n\trefresh(frm) {},\n});\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = js_session(
        &fixture_bench(),
        "apps/library/library/public/js/probe.js",
        &text,
    );

    let result = definition_at(&mut session, 100, &uri, &position);
    let links = result.as_array().expect("LocationLink[]");
    assert!(
        links.iter().any(|link| link["targetUri"]
            .as_str()
            .is_some_and(|uri| uri.ends_with("library_member.json"))),
        "{links:?}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn hover_tells_a_typod_handler_from_a_real_one() {
    // The classic silent failure spec 11 exists to catch.
    let source = "frappe.ui.form.on(\"Library Member\", {\n\trefresh(frm) {},\n\tmember_name(frm) {},\n\tloans_add(frm) {},\n\tmemebr_name(frm) {},\n});\n";
    let (mut session, uri) = js_session(
        &fixture_bench(),
        "apps/library/library/public/js/probe.js",
        source,
    );

    let hover_at = |session: &mut Session, id: i64, needle: &str| -> String {
        let at = source.find(needle).expect("needle") + 2;
        let before = &source[..at];
        let response = session.request(
            id,
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": {
                "line": before.matches('\n').count(),
                "character": before.len() - before.rfind('\n').map_or(0, |n| n + 1)
            }}),
        );
        response["result"]["contents"]["value"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };

    assert!(hover_at(&mut session, 100, "refresh(frm)").contains("lifecycle"));
    assert!(hover_at(&mut session, 101, "member_name(frm)").contains("when `member_name` changes"));
    assert!(hover_at(&mut session, 102, "loans_add(frm)").contains("table row"));
    // Neither an event nor a fieldname: valid JavaScript that never runs.
    assert!(hover_at(&mut session, 103, "memebr_name(frm)").contains("never fires"));

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn completion_inside_a_handler_map_offers_both_namespaces() {
    let source = "frappe.ui.form.on(\"Library Member\", {\n\t\"|\": function(frm) {},\n});\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = js_session(
        &fixture_bench(),
        "apps/library/library/public/js/probe.js",
        &text,
    );

    let response = session.request(
        100,
        "textDocument/completion",
        json!({"textDocument": {"uri": uri}, "position": position,
               "context": {"triggerKind": 1}}),
    );
    let items = response["result"]["items"].as_array().expect("items");
    let labels: Vec<&str> = items.iter().filter_map(|i| i["label"].as_str()).collect();

    // Spec 11's two namespaces share one keyspace, so completion must too.
    assert!(labels.contains(&"refresh"), "{labels:?}");
    assert!(labels.contains(&"member_name"), "{labels:?}");
    assert!(labels.contains(&"loans_add"), "{labels:?}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn frappe_call_in_javascript_navigates_to_its_python_definition() {
    let source = "frappe.ui.form.on(\"Library Member\", {\n\trefresh(frm) {\n\t\tfrappe.call({ method: \"library.api.get_member|_summary\" });\n\t},\n});\n";
    let (text, position) = cursor(source);
    let (mut session, uri) = js_session(
        &fixture_bench(),
        "apps/library/library/public/js/probe.js",
        &text,
    );

    let result = definition_at(&mut session, 100, &uri, &position);
    let links = result.as_array().expect("LocationLink[]");
    assert!(
        links[0]["targetUri"]
            .as_str()
            .expect("uri")
            .ends_with("api.py"),
        "{links:?}"
    );
    // `override_whitelisted_methods` remaps this one, and the call lands on
    // the replacement at runtime — so navigation goes there first (spec 21).
    let first = links[0]["targetRange"]["start"]["line"]
        .as_u64()
        .expect("line");
    let v2 = std::fs::read_to_string(fixture_bench().join("apps/library/library/api.py"))
        .expect("api.py");
    let expected = v2[..v2.find("def get_member_summary_v2").expect("v2")]
        .matches('\n')
        .count();
    assert!(
        first as usize >= expected - 1,
        "expected the remapped target, got line {first}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

fn diagnostics_for(session: &mut Session, uri: &str, text: &str) -> Vec<Value> {
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {
            "uri": uri, "languageId": "python", "version": 1, "text": text
        }}),
    );
    // didOpen publishes immediately. Match on the URI: a session may have
    // published for another buffer, and taking the first notification would
    // assert against the wrong file.
    loop {
        let message = session.receive();
        let is_publish = message["method"] == "publishDiagnostics"
            || message["method"] == "textDocument/publishDiagnostics";
        if is_publish && message["params"]["uri"] == uri {
            return message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    }
}

#[test]
fn the_deliberately_wrong_hooks_are_reported_with_their_codes() {
    let bench = fixture_bench();
    let path = bench.join("apps/circulation-app/circulation/hooks.py");
    let text = std::fs::read_to_string(&path).expect("hooks.py");

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let found = diagnostics_for(&mut session, &format!("file://{}", path.display()), &text);
    let codes: Vec<&str> = found.iter().filter_map(|d| d["code"].as_str()).collect();

    // Each of these is decided by a vendored table or by the DocType on disk,
    // so none can be wrong about code that works.
    for expected in [
        "frappe-lsp/unknown-hook",
        "frappe-lsp/unknown-doc-event",
        "frappe-lsp/non-submittable-event",
        "frappe-lsp/unknown-frequency",
        "frappe-lsp/invalid-cron",
        "frappe-lsp/duplicate-hook",
    ] {
        assert!(
            codes.contains(&expected),
            "{expected} missing from {codes:?}"
        );
    }

    // Every one carries a code and a place to look it up (spec 23).
    for diagnostic in &found {
        assert!(diagnostic["code"].is_string(), "{diagnostic}");
        assert!(
            diagnostic["codeDescription"]["href"].is_string(),
            "{diagnostic}"
        );
        assert_eq!(diagnostic["source"], "frappe-lsp");
    }

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn the_correct_app_produces_no_diagnostics_at_all() {
    // `library` is correct by construction; anything reported on it is a false
    // positive, which spec 23's governing rule forbids.
    let bench = fixture_bench();
    let path = bench.join("apps/library/library/hooks.py");
    let text = std::fs::read_to_string(&path).expect("hooks.py");

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let found = diagnostics_for(&mut session, &format!("file://{}", path.display()), &text);
    assert!(found.is_empty(), "false positives: {found:?}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn an_ignore_comment_silences_one_check_and_not_the_others() {
    let bench = fixture_bench();
    let path = bench.join("apps/circulation-app/circulation/hooks.py");
    let text = std::fs::read_to_string(&path).expect("hooks.py");

    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    // Suppress the unknown hook key only.
    let silenced = text.replace(
        "doctype_javascript = {\"Loan\": \"public/js/loan.js\"}",
        "# frappe-lsp: ignore unknown-hook\ndoctype_javascript = {\"Loan\": \"public/js/loan.js\"}",
    );
    let found = diagnostics_for(
        &mut session,
        &format!("file://{}", path.display()),
        &silenced,
    );
    let codes: Vec<&str> = found.iter().filter_map(|d| d["code"].as_str()).collect();

    assert!(!codes.contains(&"frappe-lsp/unknown-hook"), "{codes:?}");
    // The others are untouched: suppression is per code, per line.
    assert!(codes.contains(&"frappe-lsp/invalid-cron"), "{codes:?}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn the_absence_checks_work_when_a_user_turns_them_on() {
    // They ship off because they fire on correct frappe code (spec 23), but
    // off must mean "quiet", not "broken".
    let bench = fixture_bench();
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    params.as_object_mut().expect("params is an object").insert(
        "initializationOptions".to_string(),
        json!({"diagnostics": {"unknown-doctype": "warning", "unknown-field": "warning"}}),
    );
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let path = bench.join("apps/library/library/probe.py");
    let found = diagnostics_for(
        &mut session,
        &format!("file://{}", path.display()),
        "import frappe\nfrappe.get_doc(\"Libary Member\")\nfrappe.get_all(\"Library Member\", pluck=\"emial_id\")\nfrappe.get_doc(\"library member\")\n",
    );
    let codes: Vec<&str> = found.iter().filter_map(|d| d["code"].as_str()).collect();

    assert!(codes.contains(&"frappe-lsp/unknown-doctype"), "{codes:?}");
    assert!(codes.contains(&"frappe-lsp/unknown-field"), "{codes:?}");

    // A case mismatch carries its fix; a typo does not. Frappe is
    // case-sensitive here, so the near miss is a hint and never a silent
    // resolution ([04](../specs/04-metadata-index.md)).
    let messages: Vec<&str> = found.iter().filter_map(|d| d["message"].as_str()).collect();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`library member`") && m.contains("did you mean `Library Member`")),
        "{messages:?}"
    );
    assert!(
        messages
            .iter()
            .any(|m| m.contains("`Libary Member`") && !m.contains("did you mean")),
        "{messages:?}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_translation_that_cannot_be_extracted_is_reported_by_default() {
    let bench = fixture_bench();
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let path = bench.join("apps/library/library/probe.py");
    let found = diagnostics_for(
        &mut session,
        &format!("file://{}", path.display()),
        "from frappe import _\n\nname = \"x\"\na = _(f\"Loan for {name}\")\nb = _(\"Loan for {0}\").format(name)\n",
    );
    let codes: Vec<&str> = found.iter().filter_map(|d| d["code"].as_str()).collect();

    // The f-string is broken; `.format()` applied outside is correct and must
    // stay silent (spec 23's near miss).
    assert_eq!(
        codes
            .iter()
            .filter(|code| **code == "frappe-lsp/translation-literal")
            .count(),
        1,
        "{found:?}"
    );

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

/// The schema set, against the DocTypes the fixture bench carries for it.
///
/// `tests/fixtures/README.md` lists the expected code for every position in
/// these two files; this asserts the list.
#[test]
fn the_broken_doctypes_report_exactly_what_the_fixture_documents() {
    let bench = fixture_bench();
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let field_json = bench.join(
        "apps/circulation-app/circulation/circulation/doctype/circulation_broken_field/circulation_broken_field.json",
    );
    let uri = format!("file://{}", field_json.display());
    let text = std::fs::read_to_string(&field_json).expect("the fixture");
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "json", "version": 1, "text": text}}),
    );
    let found = loop {
        let message = session.receive();
        if message["method"]
            .as_str()
            .is_some_and(|m| m.ends_with("publishDiagnostics"))
            && message["params"]["uri"] == uri
        {
            break message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    };
    let codes: Vec<&str> = found.iter().filter_map(|d| d["code"].as_str()).collect();

    for expected in [
        "frappe-lsp/unknown-link-target",
        "frappe-lsp/table-not-child",
        "frappe-lsp/dangling-dynamic-link",
        "frappe-lsp/unresolvable-fetch-from",
        "frappe-lsp/dangling-depends-on",
    ] {
        assert!(
            codes.contains(&expected),
            "{expected} missing from {codes:?}"
        );
    }
    // The four `good_*` fields in the same file are the negative case: five
    // deliberate errors, and nothing said about anything else.
    assert_eq!(codes.len(), 5, "{found:?}");

    let meta_json = bench.join(
        "apps/circulation-app/circulation/circulation/doctype/circulation_broken_meta/circulation_broken_meta.json",
    );
    let uri = format!("file://{}", meta_json.display());
    let text = std::fs::read_to_string(&meta_json).expect("the fixture");
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "json", "version": 1, "text": text}}),
    );
    let found = loop {
        let message = session.receive();
        if message["method"]
            .as_str()
            .is_some_and(|m| m.ends_with("publishDiagnostics"))
            && message["params"]["uri"] == uri
        {
            break message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    };
    let codes: Vec<&str> = found.iter().filter_map(|d| d["code"].as_str()).collect();

    assert!(
        codes.contains(&"frappe-lsp/duplicate-fieldname"),
        "{codes:?}"
    );
    assert!(
        codes.contains(&"frappe-lsp/field-order-mismatch"),
        "{codes:?}"
    );
    // `title_field` and `sort_field` both name fields that do not exist.
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "frappe-lsp/unknown-meta-field")
            .count(),
        2,
        "{found:?}"
    );

    session.request(500, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_correct_doctype_json_reports_nothing() {
    // Library Member uses a self Link, a Table, a Dynamic Link, a fetch_from
    // and an `eval:` depends_on — every construct the schema set judges.
    let bench = fixture_bench();
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let path =
        bench.join("apps/library/library/library/doctype/library_member/library_member.json");
    let uri = format!("file://{}", path.display());
    let text = std::fs::read_to_string(&path).expect("the fixture");
    session.notify(
        "textDocument/didOpen",
        json!({"textDocument": {"uri": uri, "languageId": "json", "version": 1, "text": text}}),
    );
    let found = loop {
        let message = session.receive();
        if message["method"]
            .as_str()
            .is_some_and(|m| m.ends_with("publishDiagnostics"))
            && message["params"]["uri"] == uri
        {
            break message["params"]["diagnostics"]
                .as_array()
                .cloned()
                .unwrap_or_default();
        }
    };
    assert!(found.is_empty(), "false positives: {found:?}");

    session.request(500, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_case_mismatch_offers_the_replacement_as_a_quick_fix() {
    let bench = fixture_bench();
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    params.as_object_mut().expect("params is an object").insert(
        "initializationOptions".to_string(),
        json!({"diagnostics": {"unknown-doctype": "warning"}}),
    );
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let path = bench.join("apps/library/library/probe.py");
    let uri = format!("file://{}", path.display());
    let found = diagnostics_for(
        &mut session,
        &uri,
        "import frappe\nfrappe.get_doc(\"library member\")\n",
    );
    let diagnostic = found
        .iter()
        .find(|d| d["code"] == "frappe-lsp/unknown-doctype")
        .expect("the diagnostic");

    let response = session.request(
        100,
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri}, "range": diagnostic["range"],
               "context": {"diagnostics": [diagnostic]}}),
    );
    let actions = response["result"].as_array().expect("actions");
    assert_eq!(actions.len(), 1, "{actions:?}");
    assert_eq!(actions[0]["title"], "Replace with `Library Member`");
    assert_eq!(actions[0]["kind"], "quickfix");

    // The edit replaces the name and nothing else — the quotes survive.
    let edit = &actions[0]["edit"]["changes"][&uri][0];
    assert_eq!(edit["newText"], "Library Member");
    assert_eq!(edit["range"], diagnostic["range"]);

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}

#[test]
fn a_typo_with_no_single_candidate_offers_no_fix() {
    // Spec 23 allows a replacement only for a single candidate; guessing
    // between several is how a quick fix corrupts a codebase.
    let bench = fixture_bench();
    let mut session = Session::start();
    let mut params = initialize_params();
    params["rootUri"] = json!(format!("file://{}", bench.display()));
    params.as_object_mut().expect("params is an object").insert(
        "initializationOptions".to_string(),
        json!({"diagnostics": {"unknown-doctype": "warning"}}),
    );
    session.request(1, "initialize", params);
    session.notify("initialized", json!({}));
    settled(&mut session, 10);

    let path = bench.join("apps/library/library/probe.py");
    let uri = format!("file://{}", path.display());
    let found = diagnostics_for(
        &mut session,
        &uri,
        "import frappe\nfrappe.get_doc(\"Libary Member\")\n",
    );
    let diagnostic = found
        .iter()
        .find(|d| d["code"] == "frappe-lsp/unknown-doctype")
        .expect("the diagnostic");

    let response = session.request(
        100,
        "textDocument/codeAction",
        json!({"textDocument": {"uri": uri}, "range": diagnostic["range"],
               "context": {"diagnostics": [diagnostic]}}),
    );
    assert!(response["result"].is_null(), "{response}");

    session.request(200, "shutdown", json!(null));
    session.notify("exit", json!(null));
    session.finish();
}
