//! Form-handler names a workspace actually calls — [11](../../../docs/specs/11-surface-javascript.md).
//!
//! A key in a `frappe.ui.form.on` block fires if it is a lifecycle event, a
//! fieldname, or a grid event. It also fires if something calls it by name:
//! `frm.trigger("add_toolbar_buttons")` and `frm.events.convert_to_group(frm)`
//! are both ordinary Frappe, and erpnext alone writes several hundred of them.
//! Reporting those as dead handlers is the failure
//! [23](../../../docs/specs/23-diagnostics.md) forbids.

use std::sync::OnceLock;

use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use crate::language::{GrammarError, Language};

/// `.trigger("name")` in any of its receivers — `frm`, `cur_frm`,
/// `frm.script_manager`, `me.frm` — and `…events.name`, the form the modern
/// scripts use instead.
const HANDLERS: &str = r#"
(call_expression
  function: (member_expression property: (property_identifier) @method)
  arguments: (arguments . (string (string_fragment) @name))
  (#eq? @method "trigger"))

(member_expression
  object: (member_expression property: (property_identifier) @events)
  property: (property_identifier) @name
  (#eq? @events "events"))
"#;

/// Every handler name `source` calls by name.
///
/// # Errors
///
/// Only when the JavaScript grammar cannot be loaded, which is a build problem
/// rather than a source one.
pub fn read(source: &str) -> Result<Vec<String>, GrammarError> {
    let mut parser = Language::JavaScript.parser()?;
    let Some(tree) = parser.parse(source, None) else {
        return Ok(Vec::new());
    };
    Ok(read_tree(&tree, source))
}

/// Read from a tree already parsed — an open buffer, typically.
#[must_use]
pub fn read_tree(tree: &Tree, source: &str) -> Vec<String> {
    static QUERY: OnceLock<Option<Query>> = OnceLock::new();
    let compiled = QUERY.get_or_init(|| {
        match Query::new(&Language::JavaScript.grammar(), HANDLERS) {
            Ok(query) => Some(query),
            // A query that does not compile is a build fault, not a source one.
            Err(error) => {
                debug_assert!(false, "handler query does not compile: {error}");
                None
            }
        }
    });

    let mut names = Vec::new();
    let (Some(query), Some(index)) = (
        compiled.as_ref(),
        compiled
            .as_ref()
            .and_then(|query| query.capture_index_for_name("name")),
    ) else {
        return names;
    };

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source.as_bytes());
    while let Some(matched) = matches.next() {
        for capture in matched.captures.iter().filter(|c| c.index == index) {
            if let Ok(text) = capture.node.utf8_text(source.as_bytes())
                && !names.iter().any(|seen| seen == text)
            {
                names.push(text.to_string());
            }
        }
    }
    names
}

#[cfg(test)]
mod tests {
    #[test]
    fn both_call_forms_are_found() {
        let names = super::read(
            r#"
            frappe.ui.form.on("Account", {
                refresh(frm) {
                    frm.trigger("add_toolbar_buttons");
                    frm.script_manager.trigger("recalculate");
                    frm.events.convert_to_group(frm);
                },
            });
            "#,
        )
        .expect("grammar");
        assert!(
            names.contains(&"add_toolbar_buttons".to_string()),
            "{names:?}"
        );
        assert!(names.contains(&"recalculate".to_string()), "{names:?}");
        assert!(names.contains(&"convert_to_group".to_string()), "{names:?}");
    }

    #[test]
    fn an_ordinary_member_access_is_not_a_handler() {
        let names = super::read("frm.doc.customer; frm.fields_dict.items;").expect("grammar");
        assert!(names.is_empty(), "{names:?}");
    }
}
