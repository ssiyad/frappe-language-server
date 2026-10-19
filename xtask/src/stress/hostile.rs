//! What a careless or buggy client sends: positions past the end, edits that
//! do not fit the text, URIs that are not files, documents never opened.
//! A server may answer any of it with nothing. It may not exit, hang, panic or
//! answer with an internal error.

use serde_json::{Value, json};

use super::client::Client;
use super::{Drive, Stop, notify};

/// A line with `\r\n` endings, a character outside the BMP, and an unclosed
/// call at the end: three things position arithmetic gets wrong.
const SOURCE: &str = "import frappe\r\nx = frappe.get_doc(\"User\", name)\r\ny = \"\u{e9}\u{1f642}\"; frappe.db.get_value(\"User\", n, \"full_";

impl Drive {
    pub(super) fn hostile(
        &mut self,
        client: &mut Client,
        package: &std::path::Path,
    ) -> Result<(), Stop> {
        let uri = format!(
            "file://{}",
            package.join("stress_hostile_probe.py").display()
        );
        let document = json!({"uri": uri});
        notify(
            client,
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "python", "version": 1, "text": SOURCE,
            }}),
            "hostile didOpen",
        )?;

        let elsewhere = [
            "file:///nonexistent/x.py".to_string(),
            "untitled:Untitled-1".to_string(),
            format!("file://{}", package.join("__init__.py").display()),
        ];
        let positions = [
            (0, 0),
            (1, 1_000_000),
            (1_000_000, 0),
            (2, 5),
            (2, 6),
            (2, 7),
            (2, 61),
        ];
        for method in [
            "textDocument/hover",
            "textDocument/definition",
            "textDocument/completion",
            "textDocument/references",
        ] {
            for (line, character) in positions {
                self.call(
                    client,
                    method,
                    at(method, &document, line, character),
                    &format!("hostile position {line}:{character}"),
                )?;
            }
            for other in &elsewhere {
                self.call(
                    client,
                    method,
                    at(method, &json!({"uri": other}), 0, 0),
                    &format!("hostile uri {other}"),
                )?;
            }
        }
        for method in [
            "textDocument/documentSymbol",
            "textDocument/codeLens",
            "textDocument/diagnostic",
        ] {
            for other in &elsewhere {
                self.call(
                    client,
                    method,
                    json!({"textDocument": {"uri": other}}),
                    &format!("hostile uri {other}"),
                )?;
            }
        }
        for command in [
            json!({"command": "frappe-lsp.runTest", "arguments": [{"bogus": 1}]}),
            json!({"command": "frappe-lsp.runTest", "arguments": ["not a scope", 3]}),
            json!({"command": "frappe-lsp.selectSite", "arguments": [[]]}),
            json!({"command": "no-such-command", "arguments": []}),
        ] {
            self.call(
                client,
                "workspace/executeCommand",
                command,
                "hostile command",
            )?;
        }
        self.call(
            client,
            "completionItem/resolve",
            json!({"label": "User", "data": "garbage"}),
            "hostile resolve",
        )?;
        self.call(
            client,
            "workspace/symbol",
            json!({"query": "\u{e9}\u{1f642}".repeat(100)}),
            "hostile symbol query",
        )?;

        // Edits that do not fit the text. After each, the buffer must still answer.
        let edits = [
            json!({"range": range(99, 0, 99, 5), "text": "x"}),
            json!({"range": range(2, 50, 1, 0), "text": "reversed"}),
            // Inside the surrogate pair of the emoji.
            json!({"range": range(2, 6, 2, 7), "text": ""}),
            json!({"range": range(0, 1_000_000, 1_000_000, 0), "text": "\n\n"}),
            json!({"text": ""}),
            json!({"text": "\u{0}\u{1} binary \u{fffd}"}),
            json!({"range": range(0, 0, 0, 0), "text": "\u{e9}".repeat(10_000)}),
        ];
        for (version, change) in (2..).zip(edits) {
            notify(
                client,
                "textDocument/didChange",
                json!({
                    "textDocument": {"uri": uri, "version": version},
                    "contentChanges": [change],
                }),
                "hostile didChange",
            )?;
            for method in ["textDocument/hover", "textDocument/completion"] {
                self.call(
                    client,
                    method,
                    at(method, &document, 0, 3),
                    "after a hostile edit",
                )?;
            }
            self.call(
                client,
                "textDocument/diagnostic",
                json!({"textDocument": document}),
                "after a hostile edit",
            )?;
        }
        notify(
            client,
            "textDocument/didClose",
            json!({"textDocument": document}),
            "hostile didClose",
        )?;

        for (method, params) in [
            (
                "textDocument/didChange",
                json!({"textDocument": {"uri": "file:///never/opened.py", "version": 1}, "contentChanges": [{"text": "x"}]}),
            ),
            (
                "textDocument/didClose",
                json!({"textDocument": {"uri": "file:///never/opened.py"}}),
            ),
            (
                "textDocument/didSave",
                json!({"textDocument": {"uri": "untitled:x"}}),
            ),
            (
                "workspace/didChangeConfiguration",
                json!({"settings": {"frappe-lsp": {"diagnostics": "not a table"}}}),
            ),
            (
                "workspace/didChangeConfiguration",
                json!({"settings": null}),
            ),
            (
                "workspace/didChangeWatchedFiles",
                json!({"changes": [{"uri": "file:///nope/hooks.py", "type": 3}]}),
            ),
        ] {
            notify(client, method, params, "a hostile notification")?;
        }
        self.call(
            client,
            "frappe-lsp/indexStatus",
            Value::Null,
            "after hostile notifications",
        )?;
        Ok(())
    }
}

fn at(method: &str, document: &Value, line: u32, character: u32) -> Value {
    let mut params = json!({
        "textDocument": document,
        "position": {"line": line, "character": character},
    });
    // `context` means something different to each method, and completion
    // rejects the references one.
    if method == "textDocument/references" {
        params["context"] = json!({"includeDeclaration": true});
    }
    params
}

fn range(start_line: u32, start: u32, end_line: u32, end: u32) -> Value {
    json!({
        "start": {"line": start_line, "character": start},
        "end": {"line": end_line, "character": end},
    })
}
