//! The cast code action — [24](../../../docs/specs/24-type-integration.md),
//! mechanism B.
//!
//! `frappe.get_doc("Sales Invoice", name)` is typed `Document`, so pyright
//! knows none of its fields. Wrapping it in `cast("SalesInvoice", …)` names the
//! controller class without costing anything at runtime. The import goes under
//! `if TYPE_CHECKING:` for the same reason: erpnext's controllers import each
//! other, and a runtime import of one for a type is a cycle waiting to happen.

use frappe_index::Index;
use frappe_index::resolve::Target;
use frappe_syntax::{Buffer, Language};
use tower_lsp_server::ls_types::{
    CodeAction, CodeActionKind, CodeActionOrCommand, Range, TextEdit, Uri, WorkspaceEdit,
};

use crate::analysis::Found;
use crate::locations::LineIndex;

/// The calls whose return type is the DocType named by their first argument.
const CONSTRUCTORS: [&str; 5] = [
    "frappe.get_doc",
    "frappe.get_cached_doc",
    "frappe.new_doc",
    "frappe.get_last_doc",
    "frappe.get_single",
];

/// The action for the call under the cursor, if it is one worth casting.
#[must_use]
pub fn action(
    index: &Index,
    uri: &Uri,
    buffer: &Buffer,
    found: &Found,
    encoding: frappe_syntax::PositionEncoding,
) -> Option<CodeActionOrCommand> {
    if buffer.language() != Language::Python || !CONSTRUCTORS.contains(&found.matched.pattern.0) {
        return None;
    }
    let Target::DocType { name } = &found.target else {
        return None;
    };
    let controller = index.doctype(name)?.controller.as_ref()?;
    let source = buffer.text();
    let call = found.matched.span;
    if already_cast(&source, call.start) {
        return None;
    }

    let lines = LineIndex::new(&source, encoding);
    let at = |offset: usize| Range {
        start: lines.position(offset),
        end: lines.position(offset),
    };
    let class = &controller.class_name;
    let mut edits = vec![
        TextEdit {
            range: at(call.start),
            new_text: format!("cast(\"{class}\", "),
        },
        TextEdit {
            range: at(call.end),
            new_text: ")".to_string(),
        },
    ];

    let defined_here = uri
        .to_file_path()
        .is_some_and(|path| path.as_ref() == controller.path.as_path());
    let import = (!defined_here).then(|| format!("from {} import {class}", controller.module));
    edits.extend(imports(buffer, &source, import.as_deref(), &at));

    let mut changes = std::collections::HashMap::new();
    changes.insert(uri.clone(), edits);
    Some(CodeActionOrCommand::CodeAction(CodeAction {
        title: format!("Cast to {class}"),
        kind: Some(CodeActionKind::REFACTOR_REWRITE),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..WorkspaceEdit::default()
        }),
        ..CodeAction::default()
    }))
}

/// Whether the call is already the second argument of a `cast(`.
fn already_cast(source: &str, call_start: usize) -> bool {
    let before = source[..call_start].trim_end();
    let Some(head) = before.strip_suffix(',') else {
        return false;
    };
    head.rfind("cast(")
        .is_some_and(|open| !head[open..].contains(')') && !head[open..].contains('\n'))
}

/// What the module needs: `cast` and `TYPE_CHECKING` from `typing`, and the
/// controller under `if TYPE_CHECKING:`. What is already there is reused.
fn imports(
    buffer: &Buffer,
    source: &str,
    controller: Option<&str>,
    at: &dyn Fn(usize) -> Range,
) -> Vec<TextEdit> {
    let root = buffer.tree().root_node();
    let mut cursor = root.walk();
    let statements: Vec<_> = root.named_children(&mut cursor).collect();
    let text = |node: tree_sitter::Node<'_>| &source[node.byte_range()];
    let indent = indentation(source);

    let typing = statements.iter().find(|node| {
        node.kind() == "import_from_statement" && text(**node).starts_with("from typing import")
    });
    let checking = statements.iter().find(|node| {
        node.kind() == "if_statement"
            && node
                .child_by_field_name("condition")
                .is_some_and(|condition| {
                    matches!(text(condition), "TYPE_CHECKING" | "typing.TYPE_CHECKING")
                })
    });
    let imported = |name: &str| {
        typing.is_some_and(|node| {
            text(*node)["from typing import".len()..]
                .split(|c: char| c == ',' || c.is_whitespace() || c == '(' || c == ')')
                .any(|word| word == name)
        })
    };
    let has_import = |line: &str| source.lines().any(|existing| existing.trim() == line);

    let mut needed: Vec<&str> = Vec::new();
    if !imported("cast") {
        needed.push("cast");
    }
    let wants_block = controller.is_some_and(|line| !has_import(line));
    if wants_block && checking.is_none() && !imported("TYPE_CHECKING") {
        needed.insert(0, "TYPE_CHECKING");
    }

    let mut edits = Vec::new();
    let mut after_imports = String::new();
    match typing {
        // `from typing import Any` → `from typing import Any, cast`, unless
        // it wraps; a wrapped one gets a line of its own.
        Some(node) if !needed.is_empty() && !text(*node).contains('(') => edits.push(TextEdit {
            range: at(node.end_byte()),
            new_text: format!(", {}", needed.join(", ")),
        }),
        _ if !needed.is_empty() => {
            after_imports.push_str(&format!("from typing import {}\n", needed.join(", ")));
        }
        _ => {}
    }

    if let Some(line) = controller.filter(|line| !has_import(line)) {
        match checking {
            Some(block) => edits.push(TextEdit {
                range: at(block.end_byte()),
                new_text: format!("\n{indent}{line}"),
            }),
            None => after_imports.push_str(&format!("\nif TYPE_CHECKING:\n{indent}{line}\n")),
        }
    }

    if !after_imports.is_empty() {
        edits.push(TextEdit {
            range: at(insertion_point(source, &statements)),
            new_text: after_imports,
        });
    }
    edits
}

/// The start of the line after the last module-level import, or after the
/// module docstring when there are none.
fn insertion_point(source: &str, statements: &[tree_sitter::Node<'_>]) -> usize {
    let last_import = statements
        .iter()
        .rfind(|node| matches!(node.kind(), "import_statement" | "import_from_statement"));
    let after = match last_import {
        Some(node) => node.end_byte(),
        None => match statements.first() {
            Some(node)
                if node.kind() == "expression_statement"
                    && node
                        .named_child(0)
                        .is_some_and(|child| child.kind() == "string") =>
            {
                node.end_byte()
            }
            _ => return 0,
        },
    };
    source[after..]
        .find('\n')
        .map_or(source.len(), |newline| after + newline + 1)
}

/// The file's own indentation: frappe and erpnext indent with tabs.
fn indentation(source: &str) -> String {
    source
        .lines()
        .find_map(|line| {
            let indent: String = line
                .chars()
                .take_while(|c| *c == '\t' || *c == ' ')
                .collect();
            (!indent.is_empty() && line.len() > indent.len()).then_some(indent)
        })
        .unwrap_or_else(|| "\t".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use frappe_syntax::{Position, PositionEncoding};

    fn bench() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench")
    }

    /// The source after the action at `needle`, or `None` when none is offered.
    fn cast_at(
        index: &Index,
        path: &std::path::Path,
        source: &str,
        needle: &str,
    ) -> Option<String> {
        let buffer =
            Buffer::open(Language::Python, source, 1, PositionEncoding::Utf8).expect("parses");
        let offset = source.find(needle).expect("needle") + 1;
        let context = crate::analysis::context_for(index, Some(path));
        let found = crate::analysis::at(index, &buffer, offset, context)?;
        let uri: Uri = format!("file://{}", path.display()).parse().expect("uri");
        let CodeActionOrCommand::CodeAction(action) =
            action(index, &uri, &buffer, &found, PositionEncoding::Utf8)?
        else {
            return None;
        };
        let mut edits = action.edit?.changes?.remove(&uri)?;
        let offset_of = |position: tower_lsp_server::ls_types::Position| {
            buffer.offset(Position::new(position.line, position.character))
        };
        edits.sort_by_key(|edit| std::cmp::Reverse(offset_of(edit.range.start)));
        let mut text = source.to_string();
        for edit in edits {
            let (start, end) = (offset_of(edit.range.start), offset_of(edit.range.end));
            text.replace_range(start..end, &edit.new_text);
        }
        Some(text)
    }

    fn parses(source: &str) -> bool {
        let mut parser = Language::Python.parser().expect("grammar");
        !parser
            .parse(source, None)
            .expect("tree")
            .root_node()
            .has_error()
    }

    #[test]
    fn a_file_with_no_typing_import_gets_cast_and_a_type_checking_block() {
        let index = Index::build(&bench());
        let path = bench().join("apps/library/library/api.py");
        let source = "import frappe\n\n\ndef f(name):\n\tdoc = frappe.get_doc(\"Library Member\", name)\n\treturn doc\n";
        let cast = cast_at(&index, &path, source, "\"Library Member\"").expect("offered");

        assert!(
            cast.contains(
                "doc = cast(\"LibraryMember\", frappe.get_doc(\"Library Member\", name))"
            ),
            "{cast}"
        );
        assert!(
            cast.contains("from typing import TYPE_CHECKING, cast\n"),
            "{cast}"
        );
        assert!(cast.contains(
            "if TYPE_CHECKING:\n\tfrom library.library.doctype.library_member.library_member import LibraryMember\n"
        ), "{cast}");
        assert!(parses(&cast), "{cast}");
    }

    #[test]
    fn existing_imports_are_extended_rather_than_repeated() {
        let index = Index::build(&bench());
        let path = bench().join("apps/library/library/api.py");
        let source = "from typing import TYPE_CHECKING, Any\n\nimport frappe\n\nif TYPE_CHECKING:\n\tfrom frappe.model.document import Document\n\n\nx = frappe.get_doc(\"Library Member\", n)\n";
        let cast = cast_at(&index, &path, source, "\"Library Member\"").expect("offered");

        assert!(
            cast.starts_with("from typing import TYPE_CHECKING, Any, cast\n"),
            "{cast}"
        );
        assert_eq!(cast.matches("if TYPE_CHECKING:").count(), 1, "{cast}");
        assert!(cast.contains("Document\n\tfrom library.library.doctype.library_member.library_member import LibraryMember"), "{cast}");
        assert!(parses(&cast), "{cast}");
    }

    #[test]
    fn a_cast_call_is_left_alone() {
        let index = Index::build(&bench());
        let path = bench().join("apps/library/library/api.py");
        let source =
            "import frappe\nx = cast(\"LibraryMember\", frappe.get_doc(\"Library Member\", n))\n";
        assert!(cast_at(&index, &path, source, "\"Library Member\"").is_none());
    }

    #[test]
    fn the_controllers_own_file_does_not_import_itself() {
        let index = Index::build(&bench());
        let path =
            bench().join("apps/library/library/library/doctype/library_member/library_member.py");
        let source = "import frappe\n\nclass LibraryMember:\n\tpass\n\nx = frappe.get_doc(\"Library Member\", n)\n";
        let cast = cast_at(&index, &path, source, "\"Library Member\"").expect("offered");
        assert!(!cast.contains("import LibraryMember"), "{cast}");
        assert!(cast.contains("from typing import cast\n"), "{cast}");
    }
}
