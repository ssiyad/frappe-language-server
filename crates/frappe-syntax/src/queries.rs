//! Where the call shapes might be — layer one of
//! [06](../../../docs/specs/06-analysis-pipeline.md).
//!
//! Queries locate call sites, decorators and the attribute forms, and stop
//! there. Which kwarg is a fieldname list, which positional re-anchors, which
//! dict keys belong to a child DocType: none of that is expressible as a
//! pattern over the tree, so it lives in the shape tables instead. Splitting
//! the two is what keeps grammar node names out of the rest of the crate.

use std::sync::OnceLock;

use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

use crate::language::Language;

/// What a captured node is, in the vocabulary the handlers use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Site {
    /// A call expression.
    Call,
    /// A `@decorator`.
    Decorator,
    /// Attribute or member access.
    Attribute,
    /// An assignment, for `frappe.listview_settings[D] = {…}`.
    Assignment,
    /// A subscript, for `locals[D][name]`.
    Subscript,
}

impl Site {
    fn from_capture(name: &str) -> Option<Self> {
        match name {
            "call" => Some(Self::Call),
            "decorator" => Some(Self::Decorator),
            "attribute" => Some(Self::Attribute),
            "assignment" => Some(Self::Assignment),
            "subscript" => Some(Self::Subscript),
            _ => None,
        }
    }
}

const PYTHON: &str = r"
(call) @call
(decorator) @decorator
(attribute) @attribute
";

const JAVASCRIPT: &str = r"
(call_expression) @call
(member_expression) @attribute
(assignment_expression) @assignment
(subscript_expression) @subscript
";

/// Compiled once per language: `Query::new` parses the pattern text, and doing
/// that per file would cost more than the walk it replaces.
fn query(language: Language) -> Option<&'static Query> {
    static PYTHON_QUERY: OnceLock<Option<Query>> = OnceLock::new();
    static JAVASCRIPT_QUERY: OnceLock<Option<Query>> = OnceLock::new();

    let compiled = match language {
        Language::Python | Language::Jinja => {
            PYTHON_QUERY.get_or_init(|| compile(language, PYTHON))
        }
        Language::JavaScript => JAVASCRIPT_QUERY.get_or_init(|| compile(language, JAVASCRIPT)),
        Language::Json => return None,
    };
    compiled.as_ref()
}

fn compile(language: Language, source: &str) -> Option<Query> {
    match Query::new(&language.grammar(), source) {
        Ok(query) => Some(query),
        // A query that will not compile against the grammar it was written for
        // is a build fault. Extraction then finds nothing, which is the same
        // answer it gives for a language it does not know — never a panic.
        Err(error) => {
            debug_assert!(false, "{language} query does not compile: {error}");
            None
        }
    }
}

/// Every located site, in source order, paired with what it is.
///
/// Source order matters only for reproducibility; matches are sorted by span
/// before they leave [`crate::extract`].
#[must_use]
pub fn sites<'tree>(
    language: Language,
    tree: &'tree Tree,
    source: &str,
) -> Vec<(Site, tree_sitter::Node<'tree>)> {
    let Some(query) = query(language) else {
        return Vec::new();
    };

    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut found = Vec::new();

    let mut captures = cursor.captures(query, tree.root_node(), source.as_bytes());
    while let Some((capture, _)) = captures.next() {
        for capture in capture.captures {
            let Some(name) = names.get(capture.index as usize) else {
                continue;
            };
            if let Some(site) = Site::from_capture(name) {
                found.push((site, capture.node));
            }
        }
    }

    // Free next to the query engine's own cost, and it makes the dispatch
    // order reproducible for anyone reading a trace.
    found.sort_by_key(|(_, node)| (node.start_byte(), node.end_byte()));
    found.dedup_by_key(|(site, node)| (*site, node.id()));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(language: Language, source: &str) -> Tree {
        language
            .parser()
            .expect("the grammar loads")
            .parse(source, None)
            .expect("a parse with no cancellation always yields a tree")
    }

    #[test]
    fn python_sites_are_found_in_source_order() {
        let source = "@frappe.whitelist()\ndef f(doc):\n\treturn frappe.get_doc(\"Task\").name\n";
        let tree = tree(Language::Python, source);
        let found = sites(Language::Python, &tree, source);

        assert_eq!(found.first().map(|(site, _)| *site), Some(Site::Decorator));
        assert!(found.iter().any(|(site, _)| *site == Site::Call));
        assert!(found.iter().any(|(site, _)| *site == Site::Attribute));

        let starts: Vec<usize> = found.iter().map(|(_, node)| node.start_byte()).collect();
        let mut sorted = starts.clone();
        sorted.sort_unstable();
        assert_eq!(starts, sorted);
    }

    #[test]
    fn javascript_sites_cover_every_form_the_handlers_expect() {
        let source = "frappe.listview_settings[\"Task\"] = { x: locals[\"Task\"][name] };\n\
                      frappe.ui.form.on(\"Task\", { refresh(frm) { frm.doc.subject; } });\n";
        let tree = tree(Language::JavaScript, source);
        let found = sites(Language::JavaScript, &tree, source);

        for expected in [
            Site::Call,
            Site::Attribute,
            Site::Assignment,
            Site::Subscript,
        ] {
            assert!(
                found.iter().any(|(site, _)| *site == expected),
                "{expected:?} not located"
            );
        }
    }

    #[test]
    fn a_node_is_reported_once() {
        let source = "frappe.get_doc(\"Task\")\n";
        let tree = tree(Language::Python, source);
        let found = sites(Language::Python, &tree, source);

        let calls: Vec<_> = found
            .iter()
            .filter(|(site, _)| *site == Site::Call)
            .collect();
        assert_eq!(calls.len(), 1);
    }

    #[test]
    fn json_has_no_sites() {
        let source = "{\"doctype\": \"DocType\"}\n";
        let tree = tree(Language::Json, source);
        assert!(sites(Language::Json, &tree, source).is_empty());
    }

    #[test]
    fn a_file_that_does_not_parse_still_yields_what_it_can() {
        let source = "frappe.get_doc(\"Task\")\ndef broken(\n";
        let tree = tree(Language::Python, source);
        assert!(!sites(Language::Python, &tree, source).is_empty());
    }
}
