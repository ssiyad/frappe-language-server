//! The rows `frappe.get_all` and its family return —
//! [08](../../../docs/specs/08-query-rows.md).

use tree_sitter::Node;

use crate::binding::Imports;
use crate::language::Language;
use crate::matches::Binding;
use crate::read;

/// Calls that return a list of rows.
const ROWS: &[&str] = &[
    "frappe.get_all",
    "frappe.get_list",
    "frappe.db.get_all",
    "frappe.db.get_list",
];

/// Calls that return rows only with `as_dict`.
const ROWS_AS_DICT: &[&str] = &["frappe.db.get_values"];

/// Calls that return one row only with `as_dict`.
const ROW_AS_DICT: &[&str] = &["frappe.db.get_value", "frappe.get_value"];

/// What a query call with a literal DocType returns, when that is rows.
pub(crate) fn queried(right: Node<'_>, source: &str, imports: &Imports) -> Option<Binding> {
    let (callee, arguments) = read::call_parts(right, Language::Python)?;
    let called = imports.resolve(&read::dotted(callee, source)?);
    let first = *read::positional(arguments, Language::Python).first()?;
    let doctype = read::literal(first, source, Language::Python)?.text;

    let keywords = read::keyword_arguments(arguments, source);
    let set = |name: &str| {
        keywords
            .iter()
            .any(|(key, value)| *key == name && matches!(read::text(*value, source), "True" | "1"))
    };
    // `pluck` returns values and `as_list` tuples: neither has attributes.
    let plucked = keywords.iter().any(|(key, _)| *key == "pluck");

    if ROWS.contains(&called.as_str()) && !plucked && !set("as_list") {
        return Some(Binding::Rows(doctype));
    }
    if ROWS_AS_DICT.contains(&called.as_str()) && set("as_dict") {
        return Some(Binding::Rows(doctype));
    }
    if ROW_AS_DICT.contains(&called.as_str()) && set("as_dict") {
        return Some(Binding::Row(doctype));
    }
    None
}

/// `x[<integer>]`: the name indexed, which is rows if anything.
pub(crate) fn indexed<'s>(node: Node<'_>, source: &'s str) -> Option<&'s str> {
    if node.kind() != "subscript" {
        return None;
    }
    let value = node.child_by_field_name("value")?;
    let subscript = node.child_by_field_name("subscript")?;
    (read::is_name(value) && subscript.kind() == "integer").then(|| read::text(value, source))
}
