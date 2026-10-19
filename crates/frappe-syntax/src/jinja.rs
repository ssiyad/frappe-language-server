//! Jinja templates, read through the Python grammar —
//! [13](../../../docs/specs/13-surface-jinja.md).
//!
//! Jinja expressions are Python expressions with a few additions, and the
//! calls this server cares about — `frappe.get_doc(D)`, `_("…")` — are plain
//! Python inside them. So a template is rewritten into a Python document of
//! exactly the same length, every byte where it was: markup becomes spaces,
//! `{{ e }}` becomes `( e ),`, the whole is one list literal. The Python grammar
//! parses that, the Python extractor reads it, and every span it reports is a
//! span in the template.

/// The template as a Python list literal of its expressions, byte for byte the
/// same length, with every newline kept so lines line up too.
#[must_use]
pub fn to_python(template: &str) -> String {
    let bytes = template.as_bytes();
    let mut out: Vec<u8> = bytes
        .iter()
        .map(|byte| if *byte == b'\n' { b'\n' } else { b' ' })
        .collect();

    let blocks = blocks(template);
    // frappe's client-side micro-templates share the `{% %}` delimiters and
    // hold JavaScript. Read as Jinja they are nonsense, so they are text.
    if is_micro_template(template, &blocks) {
        return String::from_utf8(out).unwrap_or_default();
    }

    for block in blocks {
        match block.kind {
            Kind::Expression => {
                keep(&mut out, bytes, block.inner.clone());
                element(&mut out, &block);
            }
            Kind::Statement => {
                if let Some(expression) = statement_expression(template, &block) {
                    keep(&mut out, bytes, expression);
                    element(&mut out, &block);
                }
            }
            Kind::Comment => {}
        }
    }

    // One list around everything, so a blank line before an expression is not
    // an indented statement. The ends are blank or delimiters, never content.
    if let (Some(first), Some(last)) = (out.first().copied(), out.last().copied())
        && out.len() >= 2
        && matches!(first, b' ' | b'\n')
        && matches!(last, b' ' | b'\n' | b',')
    {
        out[0] = b'[';
        let end = out.len() - 1;
        out[end] = b']';
    }
    // Every byte is ASCII where it changed, and copied where it did not.
    String::from_utf8(out).unwrap_or_default()
}

/// A path a template names with `include`, `extends`, `import` or `from`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateRef {
    /// The path as written, quotes stripped.
    pub path: String,
    /// Byte range of the path, quotes excluded.
    pub span: std::ops::Range<usize>,
}

/// Every template path the template refers to.
#[must_use]
pub fn references(template: &str) -> Vec<TemplateRef> {
    let mut found = Vec::new();
    for block in blocks(template) {
        if block.kind != Kind::Statement {
            continue;
        }
        let inner = &template[block.inner.clone()];
        let keyword = inner.trim_start();
        let named = ["include", "extends", "import", "from"].iter().any(|word| {
            keyword.starts_with(word) && keyword[word.len()..].starts_with(char::is_whitespace)
        });
        if !named {
            continue;
        }
        let Some(open) = inner.find(['"', '\'']) else {
            continue;
        };
        let quote = inner.as_bytes()[open];
        let Some(length) = inner[open + 1..].find(quote as char) else {
            continue;
        };
        let start = block.inner.start + open + 1;
        found.push(TemplateRef {
            path: inner[open + 1..open + 1 + length].to_string(),
            span: start..start + length,
        });
    }
    found
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Expression,
    Statement,
    Comment,
}

#[derive(Debug, Clone)]
struct Block {
    kind: Kind,
    /// The delimiters included.
    outer: std::ops::Range<usize>,
    /// Between the delimiters, whitespace-control markers excluded.
    inner: std::ops::Range<usize>,
}

/// The `{{ }}`, `{% %}` and `{# #}` blocks, in order. The contents of a
/// `raw` block are text, and an unclosed block runs to the end.
fn blocks(template: &str) -> Vec<Block> {
    let bytes = template.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    let mut raw = false;

    while at + 1 < bytes.len() {
        let kind = match (bytes[at], bytes[at + 1]) {
            (b'{', b'{') => Kind::Expression,
            (b'{', b'%') => Kind::Statement,
            (b'{', b'#') => Kind::Comment,
            _ => {
                at += 1;
                continue;
            }
        };
        let close = match kind {
            Kind::Expression => "}}",
            Kind::Statement => "%}",
            Kind::Comment => "#}",
        };
        let body = at + 2;
        let end = find_close(template, body, close, kind);
        let after = (end + 2).min(bytes.len());

        let mut inner = body..end.min(bytes.len());
        if inner.start < inner.end && matches!(bytes[inner.start], b'-' | b'+') {
            inner.start += 1;
        }
        if inner.start < inner.end && matches!(bytes[inner.end - 1], b'-' | b'+') {
            inner.end -= 1;
        }

        let word = template[inner.clone()].trim();
        if raw {
            // Inside `{% raw %}` everything is text until `{% endraw %}`.
            if kind == Kind::Statement && word == "endraw" {
                raw = false;
            }
            at = after;
            continue;
        }
        if kind == Kind::Statement && word == "raw" {
            raw = true;
        }
        found.push(Block {
            kind,
            outer: at..after,
            inner,
        });
        at = after;
    }
    found
}

/// Where a block closes. A string inside an expression may hold the closing
/// delimiter, so strings are skipped there.
fn find_close(template: &str, from: usize, close: &str, kind: Kind) -> usize {
    let bytes = template.as_bytes();
    let mut at = from;
    let mut quote: Option<u8> = None;
    while at + 1 < bytes.len() {
        let byte = bytes[at];
        match quote {
            Some(_) if byte == b'\\' => at += 1,
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if kind != Kind::Comment && matches!(byte, b'"' | b'\'') => quote = Some(byte),
            // Bytes, not `&str`: `at` steps a byte at a time and may sit inside
            // a multi-byte character, where slicing the string panics.
            None if bytes[at..].starts_with(close.as_bytes()) => return at,
            None => {}
        }
        at += 1;
    }
    bytes.len()
}

/// The expression a statement carries, if it carries one worth reading.
fn statement_expression(template: &str, block: &Block) -> Option<std::ops::Range<usize>> {
    let inner = &template[block.inner.clone()];
    let leading = inner.len() - inner.trim_start().len();
    let text = inner.trim_start();
    let word_end = text
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    let keyword = &text[..word_end];
    let rest = block.inner.start + leading + word_end;

    match keyword {
        // `if a`, `elif a`, and `for x in xs`, which reads as a comparison.
        "if" | "elif" | "for" => Some(rest..block.inner.end),
        // `set x = e` keeps `e`; a block `set x` keeps nothing.
        "set" => {
            let equals = template[rest..block.inner.end].find('=')?;
            Some(rest + equals + 1..block.inner.end)
        }
        _ => None,
    }
}

/// Copy `range` from the template into the output. Jinja's `~` joins two
/// strings and has no unary form, so outside a string it becomes `+`.
fn keep(out: &mut [u8], bytes: &[u8], range: std::ops::Range<usize>) {
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    for at in range.clone() {
        // A newline inside an expression is fine inside the list's brackets.
        let byte = bytes[at];
        out[at] = byte;
        match quote {
            Some(_) if escaped => escaped = false,
            Some(_) if byte == b'\\' => escaped = true,
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None if matches!(byte, b'"' | b'\'') => quote = Some(byte),
            None if byte == b'~' => out[at] = b'+',
            None => {}
        }
    }
    python_words(out, bytes, range);
}

/// What Jinja reads differently from Python, rewritten in place.
///
/// A name that is a Python keyword, `{{ class }}` or `data.from`, gets `_`
/// for its first letter. An `if` with no `else` — `{{ x if y }}`, or
/// `{% for x in xs if cond %}` — becomes `or`: the same two bytes, and both
/// sides stay expressions for the extractor to read.
fn python_words(out: &mut [u8], bytes: &[u8], range: std::ops::Range<usize>) {
    const KEYWORDS: [&str; 22] = [
        "class", "from", "def", "import", "return", "lambda", "with", "while", "pass", "del",
        "global", "nonlocal", "async", "await", "try", "except", "finally", "raise", "assert",
        "yield", "continue", "break",
    ];
    let mut words: Vec<(usize, usize)> = Vec::new();
    let mut quote: Option<u8> = None;
    let mut escaped = false;
    let mut at = range.start;
    while at < range.end {
        let byte = bytes[at];
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == open {
                quote = None;
            }
            at += 1;
            continue;
        }
        if matches!(byte, b'"' | b'\'') {
            quote = Some(byte);
            at += 1;
            continue;
        }
        if byte.is_ascii_alphabetic() || byte == b'_' {
            let start = at;
            while at < range.end && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            words.push((start, at));
            continue;
        }
        at += 1;
    }

    let text = |(start, end): (usize, usize)| std::str::from_utf8(&bytes[start..end]).unwrap_or("");
    for word in &words {
        if KEYWORDS.contains(&text(*word)) {
            out[word.0] = b'_';
        }
    }
    let ifs: Vec<usize> = words
        .iter()
        .filter(|w| text(**w) == "if")
        .map(|w| w.0)
        .collect();
    let has_else = words.iter().any(|w| text(*w) == "else");
    if !has_else {
        for start in ifs {
            out[start] = b'o';
            out[start + 1] = b'r';
        }
    }
}

/// Whether the `{% %}` blocks hold JavaScript: `{%= e %}`, or a statement
/// that opens or closes a brace, as `{% if (x) { %}` does.
fn is_micro_template(template: &str, blocks: &[Block]) -> bool {
    blocks.iter().any(|block| {
        block.kind == Kind::Statement && {
            let inner = template[block.inner.clone()].trim();
            inner.starts_with('=') || inner.ends_with('{') || inner.starts_with('}')
        } || javascript_operators(&template[block.inner.clone()])
    })
}

/// `&&`, `||`, `===`, `!==` or a prefix `!` outside a string: JavaScript.
fn javascript_operators(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut quote: Option<u8> = None;
    for (at, byte) in bytes.iter().enumerate() {
        match quote {
            Some(open) if *byte == open => quote = None,
            Some(_) => {}
            None if matches!(byte, b'"' | b'\'') => quote = Some(*byte),
            None => {
                let next = bytes.get(at + 1).copied();
                let pair = matches!((byte, next), (b'&', Some(b'&')) | (b'|', Some(b'|')));
                let strict = matches!((byte, next), (b'=' | b'!', Some(b'=')))
                    && bytes.get(at + 2) == Some(&b'=');
                let not = *byte == b'!' && next != Some(b'=');
                if pair || strict || not {
                    return true;
                }
            }
        }
    }
    false
}

/// Turn a block's delimiters into `(` … `),`, which makes it one element.
fn element(out: &mut [u8], block: &Block) {
    let open = block.outer.start + 1;
    if open < out.len() {
        out[open] = b'(';
    }
    // The closing pair, when the block was closed.
    if block.outer.end >= block.outer.start + 4 && block.outer.end >= 2 {
        let close = block.outer.end - 2;
        if close > open {
            out[close] = b')';
            out[close + 1] = b',';
        }
    }
    // Unclosed: the template ends inside it, and the list's `]` will not
    // close a `(`. Leaving it open is what the grammar sees while typing.
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parses(python: &str) -> bool {
        let mut parser = crate::Language::Python.parser().expect("grammar");
        let tree = parser.parse(python, None).expect("a tree");
        !tree.root_node().has_error()
    }

    #[test]
    fn the_python_text_is_the_template_byte_for_byte_in_length_and_lines() {
        let template = "<h1>Café {{ frappe.get_doc(\"Item\", name).item_name }}</h1>\n\
                        {% if doc.status == \"Open\" %}<p>{{ _(\"Open\") }}</p>{% endif %}\n";
        let python = to_python(template);
        assert_eq!(python.len(), template.len());
        assert_eq!(python.lines().count(), template.lines().count());

        let call = template.find("frappe.get_doc").expect("in the template");
        assert_eq!(&python[call..call + 14], "frappe.get_doc");
        assert!(parses(&python), "{python}");
    }

    #[test]
    fn statements_keep_their_expressions_and_drop_their_keywords() {
        let template = "{% for row in frappe.get_all(\"Item\") %}{{ row.name }}{% endfor %}\n\
                        {%- set total = frappe.db.get_value(\"Item\", x, \"rate\") -%}\n\
                        {% include \"templates/includes/footer.html\" %}\n\
                        {# frappe.get_doc(\"Not code\") #}\n";
        let python = to_python(template);
        assert!(
            python.contains("row in frappe.get_all(\"Item\")"),
            "{python}"
        );
        assert!(
            python.contains("frappe.db.get_value(\"Item\", x, \"rate\")"),
            "{python}"
        );
        assert!(!python.contains("set total"), "{python}");
        assert!(!python.contains("Not code"), "{python}");
        assert!(!python.contains("footer"), "{python}");
        assert!(parses(&python), "{python}");
    }

    #[test]
    fn a_delimiter_inside_a_string_does_not_close_the_block() {
        let template = "{{ \"}}\" ~ frappe.get_doc(\"User\", u).full_name }}";
        let blocks = blocks(template);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].outer, 0..template.len());
    }

    #[test]
    fn jinja_concatenation_reads_as_addition() {
        let template = "{{ _(\"Amount in {0}\").format(currency ~ \"\") ~ \"~\" }}";
        let python = to_python(template);
        assert!(python.contains("currency + \"\") + \"~\""), "{python}");
        assert!(parses(&python), "{python}");
    }

    #[test]
    fn a_client_side_micro_template_is_text() {
        let template = "{% if (rows.length) { %}{%= frappe.format(rows[0]) %}{% } %}";
        assert!(to_python(template).trim().is_empty());
    }

    #[test]
    fn jinja_forms_python_lacks_are_rewritten_to_ones_it_has() {
        let template = "{% for item in items if item.label %}{{ item.label|e if item.show }}{% endfor %}\n\
                        <div class=\"{{ class }}\">{{ data.from }}</div>\n";
        let python = to_python(template);
        assert!(parses(&python), "{python}");
        assert!(python.contains("item in items or item.label"), "{python}");
        assert!(python.contains("_lass"), "{python}");
    }

    #[test]
    fn javascript_without_braces_is_a_micro_template_too() {
        assert!(to_python("{{ a || b }}").trim().is_empty());
        assert!(to_python("{% if (!x) %}{% endif %}").trim().is_empty());
        // `!=` is Jinja too.
        assert!(!to_python("{% if a != b %}{% endif %}").trim().is_empty());
    }

    #[test]
    fn text_with_multi_byte_characters_is_scanned_without_panicking() {
        let template = "<p>Café — naïve… {{ _(\"Total — {0}\") }} ✓</p>{% if a %}é{% endif %}";
        let python = to_python(template);
        assert_eq!(python.len(), template.len());
        assert!(parses(&python), "{python}");
    }

    #[test]
    fn a_raw_block_is_text() {
        let template = "{% raw %}{{ frappe.get_doc(\"X\") }}{% endraw %}";
        assert!(!to_python(template).contains("get_doc"));
    }

    #[test]
    fn include_extends_and_import_name_template_paths() {
        let template = "{% extends \"templates/web.html\" %}\n\
                        {%- include 'templates/includes/footer.html' -%}\n\
                        {% from \"frappe/macros.html\" import card %}\n\
                        {% if included %}{% endif %}\n";
        let found = references(template);
        let paths: Vec<&str> = found.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "templates/web.html",
                "templates/includes/footer.html",
                "frappe/macros.html"
            ]
        );
        assert_eq!(&template[found[0].span.clone()], "templates/web.html");
    }
}
