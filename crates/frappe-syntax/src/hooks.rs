//! `hooks.py` values — [12](../../../docs/specs/12-surface-hooks.md).
//!
//! An app wires itself to the framework entirely through string keys and dotted
//! paths here, and nothing else in the toolchain validates any of it: a typo
//! fails silently at runtime, months later, on someone else's site.
//!
//! Only module-level assignments with literal right-hand sides are read.
//! Anything computed — a comprehension, a conditional, `.update()` — is
//! recorded as present and opaque: the key is known to be set, and no value
//! validation runs against a value this cannot see.

use tree_sitter::{Node, Tree};

use crate::matches::Span;
use crate::read;

/// One module-level assignment in `hooks.py`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    /// The hook name, as written.
    pub key: String,
    /// Where the name is, for hover and completion.
    pub key_span: Span,
    /// The whole statement.
    pub span: Span,
    /// What it was assigned.
    pub value: Value,
}

/// A literal value, or a marker that one could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string literal, quotes stripped.
    Text {
        /// The contents.
        text: String,
        /// The literal, quotes included.
        span: Span,
    },
    /// A list or tuple.
    List(Vec<Value>),
    /// A dict. Keys that are not string literals are dropped — a computed key
    /// names nothing this side can check.
    Map(Vec<(Key, Value)>),
    /// A boolean or number; legal, and nothing resolves against it.
    Scalar(Span),
    /// Present, but not a literal. Spec 12: the key is known to be set and no
    /// value validation runs.
    Opaque(Span),
}

impl Value {
    /// The span this value occupies.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Text { span, .. } | Self::Scalar(span) | Self::Opaque(span) => *span,
            Self::List(items) => items.first().map_or(Span::new(0, 0), |first| {
                Span::new(first.span().start, items.last().map_or(0, |l| l.span().end))
            }),
            Self::Map(entries) => entries.first().map_or(Span::new(0, 0), |(key, _)| {
                Span::new(
                    key.span.start,
                    entries.last().map_or(0, |(_, value)| value.span().end),
                )
            }),
        }
    }

    /// Every string this value contains, flattened. A hook whose schema takes
    /// `File | [File]` accepts both shapes, and the caller rarely cares which
    /// was written.
    pub fn texts(&self) -> Vec<(&str, Span)> {
        match self {
            Self::Text { text, span } => vec![(text.as_str(), *span)],
            Self::List(items) => items.iter().flat_map(Value::texts).collect(),
            Self::Map(_) | Self::Scalar(_) | Self::Opaque(_) => Vec::new(),
        }
    }

    /// The entries of a map, or nothing for any other shape.
    #[must_use]
    pub fn entries(&self) -> &[(Key, Value)] {
        match self {
            Self::Map(entries) => entries,
            _ => &[],
        }
    }
}

/// A dict key that is a string literal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    /// The contents, quotes stripped.
    pub text: String,
    /// The literal, quotes included.
    pub span: Span,
}

/// Read every module-level assignment.
#[must_use]
pub fn read_tree(tree: &Tree, source: &str) -> Vec<Assignment> {
    let root = tree.root_node();
    let mut cursor = root.walk();
    let mut found = Vec::new();

    for statement in root.children(&mut cursor) {
        // Only module level: an assignment inside a function or an `if` is not
        // a hook, whatever it is named.
        if statement.kind() != "expression_statement" {
            continue;
        }
        let Some(assignment) = statement.named_child(0) else {
            continue;
        };
        if assignment.kind() != "assignment" {
            continue;
        }

        let (Some(left), Some(right)) = (
            assignment.child_by_field_name("left"),
            assignment.child_by_field_name("right"),
        ) else {
            continue;
        };
        if left.kind() != "identifier" {
            continue;
        }

        found.push(Assignment {
            key: read::text(left, source).to_string(),
            key_span: left.into(),
            span: statement.into(),
            value: value(right, source),
        });
    }

    found
}

/// Parse a Python source into assignments.
///
/// # Errors
///
/// Only when the Python grammar cannot be loaded.
pub fn read(source: &str) -> Result<Vec<Assignment>, crate::GrammarError> {
    let mut parser = crate::Language::Python.parser()?;
    let Some(tree) = parser.parse(source, None) else {
        return Ok(Vec::new());
    };
    Ok(read_tree(&tree, source))
}

fn value(node: Node<'_>, source: &str) -> Value {
    match node.kind() {
        "string" | "concatenated_string" => {
            match read::literal(node, source, crate::Language::Python) {
                Some(literal) => Value::Text {
                    text: literal.text,
                    span: node.into(),
                },
                // An f-string: present, and its contents are not knowable.
                None => Value::Opaque(node.into()),
            }
        }
        "list" | "tuple" | "set" => {
            let mut cursor = node.walk();
            Value::List(
                node.named_children(&mut cursor)
                    .map(|item| value(item, source))
                    .collect(),
            )
        }
        "dictionary" => {
            let mut cursor = node.walk();
            let mut entries = Vec::new();
            for pair in node.named_children(&mut cursor) {
                if pair.kind() != "pair" {
                    continue;
                }
                let (Some(key), Some(entry)) = (
                    pair.child_by_field_name("key"),
                    pair.child_by_field_name("value"),
                ) else {
                    continue;
                };
                // A computed key names nothing this side can check.
                let Some(literal) = read::literal(key, source, crate::Language::Python) else {
                    continue;
                };
                entries.push((
                    Key {
                        text: literal.text,
                        span: key.into(),
                    },
                    value(entry, source),
                ));
            }
            Value::Map(entries)
        }
        "true" | "false" | "none" | "integer" | "float" => Value::Scalar(node.into()),
        _ => Value::Opaque(node.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hooks(source: &str) -> Vec<Assignment> {
        read(source).expect("the Python grammar loads")
    }

    fn by_key<'a>(found: &'a [Assignment], key: &str) -> &'a Assignment {
        found
            .iter()
            .find(|assignment| assignment.key == key)
            .unwrap_or_else(|| panic!("no assignment for {key}"))
    }

    #[test]
    fn a_string_hook_reads_its_value() {
        let found = hooks("app_name = \"library\"\n");
        assert_eq!(
            by_key(&found, "app_name").value,
            Value::Text {
                text: "library".to_string(),
                span: Span::new(11, 20)
            }
        );
    }

    #[test]
    fn a_list_flattens_to_its_strings() {
        let found = hooks("app_include_js = [\"/assets/a.js\", \"/assets/b.js\"]\n");
        let texts: Vec<&str> = by_key(&found, "app_include_js")
            .value
            .texts()
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert_eq!(texts, ["/assets/a.js", "/assets/b.js"]);
    }

    #[test]
    fn a_nested_map_keeps_its_keys_and_spans() {
        let source =
            "doc_events = {\n\t\"Task\": {\n\t\t\"validate\": \"app.on_validate\"\n\t}\n}\n";
        let found = hooks(source);
        let entries = by_key(&found, "doc_events").value.entries();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].0.text, "Task");
        assert_eq!(&source[entries[0].0.span.range()], "\"Task\"");

        let inner = entries[0].1.entries();
        assert_eq!(inner[0].0.text, "validate");
        assert_eq!(inner[0].1.texts()[0].0, "app.on_validate");
    }

    #[test]
    fn a_computed_value_is_present_and_opaque() {
        // Spec 12: the key is known to be set; no value validation runs.
        let found = hooks("standard_queries = {dt: handler for dt in doctypes}\n");
        assert!(matches!(
            by_key(&found, "standard_queries").value,
            Value::Opaque(_)
        ));
    }

    #[test]
    fn an_f_string_is_opaque_rather_than_wrong() {
        let found = hooks("app_include_js = f\"/assets/{app}/js/x.js\"\n");
        assert!(matches!(
            by_key(&found, "app_include_js").value,
            Value::Opaque(_)
        ));
    }

    #[test]
    fn only_module_level_assignments_count() {
        let found = hooks(
            "def setup():\n\
             \tdoc_events = {\"Task\": {}}\n\
             \n\
             if True:\n\
             \tscheduler_events = {}\n\
             \n\
             app_name = \"library\"\n",
        );

        let keys: Vec<&str> = found.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["app_name"]);
    }

    #[test]
    fn a_duplicate_assignment_is_reported_twice() {
        // Spec 12 makes the second silently win, which is only checkable if
        // both are kept.
        let found = hooks("after_install = \"a.b\"\nafter_install = \"a.c\"\n");
        let repeats: Vec<&Assignment> = found
            .iter()
            .filter(|assignment| assignment.key == "after_install")
            .collect();
        assert_eq!(repeats.len(), 2);
    }

    #[test]
    fn a_computed_dict_key_is_dropped_not_guessed() {
        let found = hooks("doc_events = {SOME_DOCTYPE: {\"validate\": \"a.b\"}}\n");
        assert!(by_key(&found, "doc_events").value.entries().is_empty());
    }
}
