//! Byte ranges into a source file, and where the pieces of a JSON document sit.
//!
//! `serde_json` gives values but not positions, and
//! [21](../../../docs/specs/21-navigation.md) needs to point an editor at the
//! field object inside a 200-field DocType JSON. This module supplies the
//! positions; the values still come from `serde_json`, so there is exactly one
//! implementation of JSON semantics in the crate.

use std::fmt;

/// A half-open byte range into the file it was read from.
///
/// Byte offsets rather than line/character: the index has no line table, and
/// `positionEncoding` is negotiated per session, so the conversion belongs at
/// the protocol edge where the buffer text is already open.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct Range {
    /// First byte.
    pub start: u32,
    /// One past the last byte.
    pub end: u32,
}

/// `frappe-syntax` spans measure the same bytes; the index keeps them in `u32`
/// so a `Field` stays small.
impl From<frappe_syntax::Span> for Range {
    fn from(span: frappe_syntax::Span) -> Self {
        Self::new(span.start, span.end)
    }
}

impl Range {
    /// A range over `start..end`, saturating at `u32::MAX`.
    ///
    /// Saturation rather than a panic: a DocType JSON larger than 4 GiB would
    /// produce a wrong range, and a wrong range is survivable where a dead LSP
    /// session is not.
    #[must_use]
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start: u32::try_from(start).unwrap_or(u32::MAX),
            end: u32::try_from(end).unwrap_or(u32::MAX),
        }
    }

    /// The empty range at offset 0 — what an entity with no file position gets.
    #[must_use]
    pub fn none() -> Self {
        Self { start: 0, end: 0 }
    }

    /// Whether this range covers nothing.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.start >= self.end
    }

    /// Byte length.
    #[must_use]
    pub fn len(self) -> usize {
        (self.end.saturating_sub(self.start)) as usize
    }

    /// Whether `offset` falls inside.
    #[must_use]
    pub fn contains(self, offset: u32) -> bool {
        self.start <= offset && offset < self.end
    }

    /// The text this range covers, or `None` if it does not land on character
    /// boundaries of `source`.
    #[must_use]
    pub fn text(self, source: &str) -> Option<&str> {
        source.get(self.start as usize..self.end as usize)
    }

    /// The range without its first and last byte — the interior of a quoted
    /// string, which is what `originSelectionRange` must cover
    /// ([21](../../../docs/specs/21-navigation.md)).
    #[must_use]
    pub fn interior(self) -> Self {
        if self.len() < 2 {
            return self;
        }
        Self {
            start: self.start + 1,
            end: self.end - 1,
        }
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}..{}", self.start, self.end)
    }
}

/// A JSON value and where it sits.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Node {
    /// The whole value, quotes and brackets included.
    pub range: Range,
    /// What kind of value it is, and its children.
    pub kind: Kind,
}

/// The shape of a [`Node`]. Scalars carry no value — `serde_json` owns those.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Kind {
    /// `{ … }`
    Object(Vec<Member>),
    /// `[ … ]`
    Array(Vec<Node>),
    /// A string, number, `true`, `false` or `null`.
    Scalar,
}

/// One `"key": value` pair.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Member {
    /// The key, unescaped.
    pub key: String,
    /// The key including its quotes.
    pub key_range: Range,
    /// The value.
    pub value: Node,
}

impl Node {
    /// The member named `key`, if this node is an object that has one.
    #[must_use]
    pub fn member(&self, key: &str) -> Option<&Member> {
        match &self.kind {
            Kind::Object(members) => members.iter().find(|member| member.key == key),
            _ => None,
        }
    }

    /// The value of `key`.
    #[must_use]
    pub fn value(&self, key: &str) -> Option<&Node> {
        self.member(key).map(|member| &member.value)
    }

    /// The range of `key`'s value, if present.
    #[must_use]
    pub fn value_range(&self, key: &str) -> Option<Range> {
        self.value(key).map(|node| node.range)
    }

    /// This node's elements, or nothing if it is not an array.
    #[must_use]
    pub fn elements(&self) -> &[Node] {
        match &self.kind {
            Kind::Array(elements) => elements,
            _ => &[],
        }
    }

    /// The innermost node covering `offset`, outermost first along the way.
    ///
    /// Positions inside a DocType JSON buffer resolve through this rather than
    /// through a second parser.
    #[must_use]
    pub fn path_to(&self, offset: u32) -> Vec<&Node> {
        let mut found = Vec::new();
        let mut current = self;
        loop {
            if !current.range.contains(offset) {
                break;
            }
            found.push(current);
            let next = match &current.kind {
                Kind::Object(members) => members
                    .iter()
                    .find(|member| member.value.range.contains(offset))
                    .map(|member| &member.value),
                Kind::Array(elements) => elements.iter().find(|node| node.range.contains(offset)),
                Kind::Scalar => None,
            };
            match next {
                Some(node) => current = node,
                None => break,
            }
        }
        found
    }
}

/// Why a document could not be scanned for positions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScanError {
    /// Byte offset where the scan gave up.
    pub offset: usize,
    /// What was wrong there.
    pub message: &'static str,
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at byte {}", self.message, self.offset)
    }
}

/// Nesting a DocType JSON never approaches. Bounded because a deeply nested
/// input would otherwise overflow the stack, and a panic kills the session.
const MAX_DEPTH: usize = 64;

/// Locate every value in a JSON document.
///
/// Validation is `serde_json`'s job; this scanner reports only the errors it
/// must to keep its own offsets honest.
pub fn scan(source: &str) -> Result<Node, ScanError> {
    let mut scanner = Scanner {
        source,
        bytes: source.as_bytes(),
        at: 0,
    };
    scanner.skip_whitespace();
    let node = scanner.value(0)?;
    scanner.skip_whitespace();
    if scanner.at != scanner.bytes.len() {
        return Err(ScanError {
            offset: scanner.at,
            message: "trailing content",
        });
    }
    Ok(node)
}

struct Scanner<'a> {
    source: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Scanner<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn error(&self, message: &'static str) -> ScanError {
        ScanError {
            offset: self.at,
            message,
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node, ScanError> {
        if depth > MAX_DEPTH {
            return Err(self.error("nested past the depth limit"));
        }
        match self.peek() {
            Some(b'{') => self.object(depth),
            Some(b'[') => self.array(depth),
            Some(b'"') => {
                let start = self.at;
                self.string()?;
                Ok(Node {
                    range: Range::new(start, self.at),
                    kind: Kind::Scalar,
                })
            }
            Some(_) => {
                let start = self.at;
                while let Some(byte) = self.peek() {
                    if matches!(byte, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r') {
                        break;
                    }
                    self.at += 1;
                }
                if self.at == start {
                    return Err(self.error("expected a value"));
                }
                Ok(Node {
                    range: Range::new(start, self.at),
                    kind: Kind::Scalar,
                })
            }
            None => Err(self.error("expected a value")),
        }
    }

    /// Consumes a string, leaving `at` past the closing quote.
    ///
    /// Byte-wise scanning is UTF-8 safe here: every byte of a multi-byte
    /// sequence is >= 0x80, so it can never be mistaken for a quote or a
    /// backslash.
    fn string(&mut self) -> Result<Range, ScanError> {
        let start = self.at;
        self.at += 1;
        loop {
            match self.peek() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(Range::new(start, self.at));
                }
                Some(b'\\') => self.at += 2,
                Some(_) => self.at += 1,
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Node, ScanError> {
        let start = self.at;
        self.at += 1;
        let mut members = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b'}') => {
                    self.at += 1;
                    return Ok(Node {
                        range: Range::new(start, self.at),
                        kind: Kind::Object(members),
                    });
                }
                Some(b',') => {
                    self.at += 1;
                    continue;
                }
                Some(b'"') => {}
                _ => return Err(self.error("expected a key")),
            }

            let key_range = self.string()?;
            let key = unescape(key_range.interior().text(self.source).unwrap_or_default());

            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            self.at += 1;
            self.skip_whitespace();

            let value = self.value(depth + 1)?;
            members.push(Member {
                key,
                key_range,
                value,
            });
        }
    }

    fn array(&mut self, depth: usize) -> Result<Node, ScanError> {
        let start = self.at;
        self.at += 1;
        let mut elements = Vec::new();
        loop {
            self.skip_whitespace();
            match self.peek() {
                Some(b']') => {
                    self.at += 1;
                    return Ok(Node {
                        range: Range::new(start, self.at),
                        kind: Kind::Array(elements),
                    });
                }
                Some(b',') => {
                    self.at += 1;
                    continue;
                }
                None => return Err(self.error("unterminated array")),
                _ => elements.push(self.value(depth + 1)?),
            }
        }
    }
}

/// JSON string escapes, for keys only. Values go through `serde_json`.
fn unescape(text: &str) -> String {
    if !text.contains('\\') {
        return text.to_string();
    }
    let mut output = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            output.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('b') => output.push('\u{8}'),
            Some('f') => output.push('\u{c}'),
            Some('u') => {
                let hex: String = characters.by_ref().take(4).collect();
                match u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    Some(decoded) => output.push(decoded),
                    // A lone surrogate cannot appear in a Frappe key; keeping
                    // the escape verbatim beats dropping the key entirely.
                    None => output.push_str(&format!("\\u{hex}")),
                }
            }
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_range_covers_the_text_it_names() {
        let source = "{\"a\": 1}";
        let node = scan(source).expect("scans");
        let value = node.value("a").expect("member a");
        assert_eq!(value.range.text(source), Some("1"));
    }

    #[test]
    fn the_interior_of_a_string_drops_its_quotes() {
        let source = "{\"name\": \"Library Member\"}";
        let node = scan(source).expect("scans");
        let value = node.value("name").expect("member name");
        assert_eq!(value.range.text(source), Some("\"Library Member\""));
        assert_eq!(
            value.range.interior().text(source),
            Some("Library Member"),
            "originSelectionRange covers the interior only — specs/21"
        );
    }

    #[test]
    fn array_elements_keep_their_own_ranges() {
        let source =
            "{\n \"fields\": [\n  {\"fieldname\": \"a\"},\n  {\"fieldname\": \"b\"}\n ]\n}";
        let node = scan(source).expect("scans");
        let fields = node.value("fields").expect("fields");
        let elements = fields.elements();

        assert_eq!(elements.len(), 2);
        assert_eq!(
            elements[0].range.text(source),
            Some("{\"fieldname\": \"a\"}")
        );
        assert_eq!(
            elements[1].range.text(source),
            Some("{\"fieldname\": \"b\"}")
        );
    }

    #[test]
    fn escapes_never_end_a_string_early() {
        let source = r#"{"options": "\nOne\nTwo", "after": 1}"#;
        let node = scan(source).expect("scans");
        assert_eq!(
            node.value("options").and_then(|n| n.range.text(source)),
            Some(r#""\nOne\nTwo""#)
        );
        assert!(node.value("after").is_some());
    }

    #[test]
    fn multibyte_text_does_not_shift_offsets() {
        let source = "{\"label\": \"Café ☕\", \"next\": 2}";
        let node = scan(source).expect("scans");
        assert_eq!(
            node.value("label")
                .and_then(|n| n.range.interior().text(source)),
            Some("Café ☕")
        );
        assert_eq!(
            node.value("next").and_then(|n| n.range.text(source)),
            Some("2")
        );
    }

    #[test]
    fn a_position_resolves_to_the_innermost_value() {
        let source = "{\"fields\": [{\"options\": \"Library Member\"}]}";
        let offset = source.find("Library").expect("present") as u32;
        let node = scan(source).expect("scans");

        let path = node.path_to(offset);
        assert_eq!(path.len(), 4, "root, array, element, string");
        assert_eq!(
            path.last()
                .expect("innermost")
                .range
                .interior()
                .text(source),
            Some("Library Member")
        );
    }

    #[test]
    fn a_position_outside_the_document_resolves_to_nothing() {
        let node = scan("{\"a\": 1}").expect("scans");
        assert!(node.path_to(9999).is_empty());
    }

    #[test]
    fn malformed_input_reports_an_offset_rather_than_panicking() {
        let error = scan("{\"a\": ").expect_err("incomplete");
        assert_eq!(error.offset, 6);

        assert!(scan("").is_err());
        assert!(scan("{\"a\" 1}").is_err());
        assert!(scan("{\"a\": \"unterminated").is_err());
    }

    #[test]
    fn depth_is_bounded_rather_than_overflowing_the_stack() {
        let deep = "[".repeat(MAX_DEPTH + 10) + &"]".repeat(MAX_DEPTH + 10);
        assert!(scan(&deep).is_err());
    }

    #[test]
    fn escaped_keys_are_unescaped() {
        let source = r#"{"a\nb": 1}"#;
        let node = scan(source).expect("scans");
        assert!(node.member("a\nb").is_some());
    }
}
