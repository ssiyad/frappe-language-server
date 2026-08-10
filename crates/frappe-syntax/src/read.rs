//! Reading literals and call structure out of a tree, per grammar.
//!
//! The two grammars spell the same shapes differently — `argument_list` against
//! `arguments`, `dictionary` against `object` — so every node-kind name lives
//! here and the extractor above stays grammar-agnostic.

use tree_sitter::Node;

use crate::language::Language;
use crate::matches::{Span, TranslationArg};

/// A string literal read out of the tree.
pub(crate) struct Literal {
    /// The content between the quotes, escapes **not** decoded: a DocType or
    /// fieldname carrying an escape is not a name any index would hold.
    pub text: String,
    /// The whole literal, quotes included, so completion has a range to
    /// replace.
    pub span: Span,
    /// Just the content, so a position inside it can be located: `order_by`
    /// holds several fieldnames in one string.
    ///
    /// Only usable for that when it is as long as `text`. Python's adjacent
    /// concatenation spans the quotes between the parts, so a caller offsetting
    /// into it MUST compare the two lengths first.
    pub content: Span,
}

/// The text a node covers.
pub(crate) fn text<'a>(node: Node<'_>, source: &'a str) -> &'a str {
    source.get(node.byte_range()).unwrap_or_default()
}

/// A plain string literal, or `None` for anything with a substitution or a
/// prefix that changes its type.
///
/// Python's implicit adjacent concatenation — `"one " "string"` — is one
/// literal to the language and to Frappe's own translation extractor, so it is
/// read as one here too.
pub(crate) fn literal(node: Node<'_>, source: &str, language: Language) -> Option<Literal> {
    match language {
        Language::Python => python_literal(node, source),
        Language::JavaScript => javascript_literal(node, source),
        Language::Json => (node.kind() == "string").then(|| Literal {
            text: string_content(node, source),
            span: node.into(),
            content: content_span(node),
        }),
    }
}

fn python_literal(node: Node<'_>, source: &str) -> Option<Literal> {
    if node.kind() == "concatenated_string" {
        return adjacent_literals(node, source);
    }
    if node.kind() != "string" {
        return None;
    }
    // `f"…"` carries `interpolation`; `b"…"` is not a name at all.
    if node.named_children(&mut node.walk()).any(is_interpolation) {
        return None;
    }
    let prefix = node
        .named_child(0)
        .map(|start| text(start, source))
        .unwrap_or_default();
    let is_bytes_or_fstring = prefix
        .chars()
        .take_while(char::is_ascii_alphabetic)
        .any(|c| matches!(c.to_ascii_lowercase(), 'b' | 'f'));
    if is_bytes_or_fstring {
        return None;
    }

    Some(Literal {
        text: string_content(node, source),
        span: node.into(),
        content: content_span(node),
    })
}

/// `"one " "string"`, every part a plain literal.
fn adjacent_literals(node: Node<'_>, source: &str) -> Option<Literal> {
    let parts: Vec<Literal> = node
        .named_children(&mut node.walk())
        .map(|child| python_literal(child, source))
        .collect::<Option<Vec<_>>>()?;
    let (first, last) = (parts.first()?, parts.last()?);

    Some(Literal {
        // The content is not contiguous, so it does not describe the text;
        // `Literal::content` documents that callers must check.
        content: Span::new(first.content.start, last.content.end),
        text: parts.iter().map(|part| part.text.as_str()).collect(),
        span: node.into(),
    })
}

fn javascript_literal(node: Node<'_>, source: &str) -> Option<Literal> {
    match node.kind() {
        "string" => Some(Literal {
            text: string_content(node, source),
            span: node.into(),
            content: content_span(node),
        }),
        // A template literal is a literal only while nothing is substituted in.
        "template_string" => {
            let substituted = node
                .named_children(&mut node.walk())
                .any(|child| child.kind() == "template_substitution");
            (!substituted).then(|| Literal {
                text: string_content(node, source),
                span: node.into(),
                content: content_span(node),
            })
        }
        _ => None,
    }
}

/// The content between the quotes. An empty literal has no content child, so
/// the span collapses just inside the quotes.
fn content_span(node: Node<'_>) -> Span {
    let mut cursor = node.walk();
    let parts: Vec<Node<'_>> = node
        .named_children(&mut cursor)
        .filter(|child| matches!(child.kind(), "string_content" | "string_fragment"))
        .collect();
    match (parts.first(), parts.last()) {
        (Some(first), Some(last)) => Span::new(first.start_byte(), last.end_byte()),
        _ => Span::new(
            node.start_byte().saturating_add(1).min(node.end_byte()),
            node.end_byte().saturating_sub(1).max(node.start_byte()),
        ),
    }
}

fn is_interpolation(node: Node<'_>) -> bool {
    node.kind() == "interpolation"
}

/// Everything between the opening and closing quote.
fn string_content(node: Node<'_>, source: &str) -> String {
    let mut content = String::new();
    for child in node.named_children(&mut node.walk()) {
        match child.kind() {
            "string_content" | "string_fragment" => content.push_str(text(child, source)),
            "escape_sequence" => content.push_str(text(child, source)),
            _ => {}
        }
    }
    content
}

/// How a translation call's argument was written —
/// [23](../../../docs/specs/23-diagnostics.md).
pub(crate) fn translation_arg(node: Node<'_>, source: &str, language: Language) -> TranslationArg {
    if literal(node, source, language).is_some() {
        return TranslationArg::Literal;
    }
    match node.kind() {
        // An f-string or a template literal with substitutions.
        "string" | "template_string" => TranslationArg::Interpolated,
        "concatenated_string" => TranslationArg::Concatenated,
        // Only `+` concatenates. `label || "ID"` is a fallback, and calling it
        // a concatenation would report an Error on a shape frappe itself uses
        // throughout — [23](../../../docs/specs/23-diagnostics.md).
        "binary_expression" | "binary_operator"
            if node
                .child_by_field_name("operator")
                .map(|operator| text(operator, source))
                == Some("+") =>
        {
            TranslationArg::Concatenated
        }
        _ => {
            // `_("… {0} …".format(x))` — the format applied inside the call.
            if let Some((callee, _)) = call_parts(node, language)
                && let Some(receiver) = callee.child_by_field_name("object")
                && callee
                    .child_by_field_name("attribute")
                    .or_else(|| callee.child_by_field_name("property"))
                    .map(|name| text(name, source))
                    == Some("format")
                && literal(receiver, source, language).is_some()
            {
                return TranslationArg::Formatted;
            }
            TranslationArg::Dynamic
        }
    }
}

/// The callee and argument-list nodes of a call.
pub(crate) fn call_parts<'t>(node: Node<'t>, language: Language) -> Option<(Node<'t>, Node<'t>)> {
    let kind = match language {
        Language::Python => "call",
        Language::JavaScript => "call_expression",
        Language::Json => return None,
    };
    if node.kind() != kind {
        return None;
    }
    Some((
        node.child_by_field_name("function")?,
        node.child_by_field_name("arguments")?,
    ))
}

/// Positional arguments, in order. Keyword arguments are excluded and come back
/// from [`keyword_arguments`].
pub(crate) fn positional<'t>(arguments: Node<'t>, language: Language) -> Vec<Node<'t>> {
    arguments
        .named_children(&mut arguments.walk())
        .filter(|child| !(language == Language::Python && child.kind() == "keyword_argument"))
        .filter(|child| child.kind() != "comment")
        .collect()
}

/// `name=value` arguments. Python only; JavaScript passes an options object.
pub(crate) fn keyword_arguments<'t>(
    arguments: Node<'t>,
    source: &'t str,
) -> Vec<(&'t str, Node<'t>)> {
    arguments
        .named_children(&mut arguments.walk())
        .filter(|child| child.kind() == "keyword_argument")
        .filter_map(|child| {
            Some((
                text(child.child_by_field_name("name")?, source),
                child.child_by_field_name("value")?,
            ))
        })
        .collect()
}

/// The elements of a list, tuple or array literal.
pub(crate) fn sequence<'t>(node: Node<'t>, language: Language) -> Option<Vec<Node<'t>>> {
    let matches = match language {
        Language::Python => matches!(node.kind(), "list" | "tuple" | "set"),
        Language::JavaScript => node.kind() == "array",
        Language::Json => node.kind() == "array",
    };
    matches.then(|| {
        node.named_children(&mut node.walk())
            .filter(|child| child.kind() != "comment")
            .collect()
    })
}

/// The `key: value` pairs of a dict or object literal.
pub(crate) fn mapping<'t>(node: Node<'t>, language: Language) -> Option<Vec<(Node<'t>, Node<'t>)>> {
    let matches = match language {
        Language::Python => node.kind() == "dictionary",
        Language::JavaScript | Language::Json => node.kind() == "object",
    };
    matches.then(|| {
        node.named_children(&mut node.walk())
            .filter(|child| matches!(child.kind(), "pair"))
            .filter_map(|pair| {
                Some((
                    pair.child_by_field_name("key")?,
                    pair.child_by_field_name("value")?,
                ))
            })
            .collect()
    })
}

/// A dotted callee flattened to source order: `frappe.db.get_value`.
///
/// `None` as soon as a segment is not a plain name — `getattr(frappe, x).y`
/// and `self.items[0].get` are not call shapes any table lists.
pub(crate) fn dotted(node: Node<'_>, source: &str) -> Option<String> {
    let mut segments = Vec::new();
    let mut current = node;
    loop {
        match current.kind() {
            "identifier" | "property_identifier" => {
                segments.push(text(current, source));
                break;
            }
            "attribute" | "member_expression" => {
                let name = current
                    .child_by_field_name("attribute")
                    .or_else(|| current.child_by_field_name("property"))?;
                if !matches!(name.kind(), "identifier" | "property_identifier") {
                    return None;
                }
                segments.push(text(name, source));
                current = current.child_by_field_name("object")?;
            }
            "parenthesized_expression" => current = current.named_child(0)?,
            _ => return None,
        }
    }
    segments.reverse();
    Some(segments.join("."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::{Buffer, PositionEncoding};

    fn parse(language: Language, source: &str) -> Buffer {
        Buffer::open(language, source, 1, PositionEncoding::Utf16).expect("grammar")
    }

    /// The first node of `kind` in the tree, depth first.
    fn first<'t>(node: Node<'t>, kind: &str) -> Node<'t> {
        let mut stack = vec![node];
        while let Some(node) = stack.pop() {
            if node.kind() == kind {
                return node;
            }
            let mut cursor = node.walk();
            stack.extend(node.named_children(&mut cursor));
        }
        panic!("no `{kind}` node in the tree");
    }

    #[test]
    fn a_plain_python_string_reads_as_a_literal() {
        let source = "x = \"Library Member\"\n";
        let buffer = parse(Language::Python, source);
        let node = first(buffer.tree().root_node(), "string");
        let literal = literal(node, source, Language::Python).expect("literal");

        assert_eq!(literal.text, "Library Member");
        assert_eq!(&source[literal.span.range()], "\"Library Member\"");
    }

    #[test]
    fn adjacent_python_literals_read_as_one_string() {
        let source = "x = \"Library \" \"Member\"\n";
        let buffer = parse(Language::Python, source);
        let node = first(buffer.tree().root_node(), "concatenated_string");
        let literal = literal(node, source, Language::Python).expect("literal");

        assert_eq!(literal.text, "Library Member");
        assert_eq!(&source[literal.span.range()], "\"Library \" \"Member\"");
    }

    #[test]
    fn adjacent_literals_stop_being_one_when_a_part_is_not_one() {
        let source = "x = \"Library \" f\"{kind}\"\n";
        let buffer = parse(Language::Python, source);
        let node = first(buffer.tree().root_node(), "concatenated_string");

        assert!(literal(node, source, Language::Python).is_none());
    }

    #[test]
    fn an_f_string_and_a_bytes_string_are_not_literals() {
        for source in ["x = f\"Library {kind}\"\n", "x = b\"Library Member\"\n"] {
            let buffer = parse(Language::Python, source);
            let node = first(buffer.tree().root_node(), "string");
            assert!(
                literal(node, source, Language::Python).is_none(),
                "{source}"
            );
        }
    }

    #[test]
    fn a_template_literal_is_a_literal_only_without_substitutions() {
        let plain = "const x = `Library Member`;\n";
        let buffer = parse(Language::JavaScript, plain);
        let node = first(buffer.tree().root_node(), "template_string");
        assert_eq!(
            literal(node, plain, Language::JavaScript).map(|l| l.text),
            Some("Library Member".to_string())
        );

        let substituted = "const x = `Library ${kind}`;\n";
        let buffer = parse(Language::JavaScript, substituted);
        let node = first(buffer.tree().root_node(), "template_string");
        assert!(literal(node, substituted, Language::JavaScript).is_none());
    }

    #[test]
    fn a_dotted_callee_flattens_in_source_order() {
        let source = "frappe.db.get_value(\"Loan\", name, \"status\")\n";
        let buffer = parse(Language::Python, source);
        let call = first(buffer.tree().root_node(), "call");
        let (callee, _) = call_parts(call, Language::Python).expect("call");

        assert_eq!(
            dotted(callee, source).as_deref(),
            Some("frappe.db.get_value")
        );
    }

    #[test]
    fn a_callee_with_a_computed_segment_flattens_to_nothing() {
        let source = "getattr(frappe.db, name)(\"Loan\")\n";
        let buffer = parse(Language::Python, source);
        let call = first(buffer.tree().root_node(), "call");
        let (callee, _) = call_parts(call, Language::Python).expect("call");

        assert_eq!(dotted(callee, source), None);
    }

    #[test]
    fn translation_arguments_are_classified_by_how_they_are_written() {
        let cases = [
            ("_(\"Loan created\")", TranslationArg::Literal),
            ("_(f\"Loan {name}\")", TranslationArg::Interpolated),
            ("_(\"Loan {0}\".format(name))", TranslationArg::Formatted),
            ("_(\"Loan \" + name)", TranslationArg::Concatenated),
            // Adjacent literals are one string to Python and to the extractor.
            ("_(\"Loan \" \"created\")", TranslationArg::Literal),
            // A fallback is not a concatenation; `||` is pervasive in frappe.
            ("_(label or \"Loan\")", TranslationArg::Dynamic),
            ("_(name)", TranslationArg::Dynamic),
        ];
        for (source, expected) in cases {
            let source = format!("{source}\n");
            let buffer = parse(Language::Python, &source);
            let call = first(buffer.tree().root_node(), "call");
            let (_, arguments) = call_parts(call, Language::Python).expect("call");
            let argument = positional(arguments, Language::Python)[0];

            assert_eq!(
                translation_arg(argument, &source, Language::Python),
                expected,
                "{source}"
            );
        }
    }
}
