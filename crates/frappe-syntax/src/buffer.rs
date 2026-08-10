//! Open buffers: a rope, a cached tree, and incremental re-parse.
//!
//! [06](../../../docs/specs/06-analysis-pipeline.md): `didOpen` parses and
//! caches, `didChange` edits the tree and re-parses against it, `didClose` drops
//! both. Position conversion honours the negotiated `positionEncoding`.

use ropey::Rope;
use tree_sitter::{InputEdit, Parser, Point, Tree};

use crate::language::{GrammarError, Language};

/// The encoding both sides agreed on at `initialize`.
///
/// [30](../../../docs/specs/30-configuration.md) falls back to UTF-16 when the
/// client does not advertise UTF-8, which is what `Default` gives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    /// `character` counts UTF-8 bytes.
    Utf8,
    /// `character` counts UTF-16 code units.
    #[default]
    Utf16,
}

/// A zero-based line and a column in the negotiated encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Position {
    /// Zero-based line.
    pub line: u32,
    /// Column, counted in the units of the negotiated encoding.
    pub character: u32,
}

impl Position {
    /// A position at `line`, `character`.
    #[must_use]
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A half-open range of [`Position`]s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Range {
    /// First position in the range.
    pub start: Position,
    /// One past the last position in the range.
    pub end: Position,
}

impl Range {
    /// A range from `start` to `end`.
    #[must_use]
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }
}

/// One entry of a `textDocument/didChange` batch.
///
/// `range` is `None` for a whole-document replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The range replaced, in the encoding the buffer was opened with.
    pub range: Option<Range>,
    /// The replacement text.
    pub text: String,
}

/// An open buffer: text, the tree parsed from it, and the parser that made it.
///
/// Only open buffers hold trees ([31](../../../docs/specs/31-performance.md)),
/// so the per-buffer `Parser` is bounded by the number of open files.
pub struct Buffer {
    language: Language,
    encoding: PositionEncoding,
    parser: Parser,
    text: Rope,
    tree: Tree,
    version: i32,
}

impl std::fmt::Debug for Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Buffer")
            .field("language", &self.language)
            .field("encoding", &self.encoding)
            .field("version", &self.version)
            .field("len_bytes", &self.text.len_bytes())
            .finish_non_exhaustive()
    }
}

impl Buffer {
    /// Parse `text` and cache the tree — the `didOpen` path.
    pub fn open(
        language: Language,
        text: &str,
        version: i32,
        encoding: PositionEncoding,
    ) -> Result<Self, GrammarError> {
        let mut parser = language.parser()?;
        let tree = parser
            .parse(text, None)
            .ok_or(GrammarError::Empty { language })?;
        Ok(Self {
            language,
            encoding,
            parser,
            text: Rope::from_str(text),
            tree,
            version,
        })
    }

    /// The grammar this buffer was opened with.
    #[must_use]
    pub fn language(&self) -> Language {
        self.language
    }

    /// The last `didChange` version applied.
    #[must_use]
    pub fn version(&self) -> i32 {
        self.version
    }

    /// The cached tree.
    #[must_use]
    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    /// The rope backing the buffer.
    #[must_use]
    pub fn rope(&self) -> &Rope {
        &self.text
    }

    /// The whole buffer as a `String`.
    ///
    /// Allocates. `crate::extract::extract` wants contiguous bytes — a
    /// tree-sitter node reads its own text out of the source slice — so this is
    /// the one copy per extraction pass. Callers reading a single span should
    /// use [`Buffer::slice`], which copies only when the range straddles two
    /// rope chunks.
    #[must_use]
    pub fn text(&self) -> String {
        self.text.to_string()
    }

    /// The text of a byte range, or `""` if the range is out of bounds.
    #[must_use]
    pub fn slice(&self, range: std::ops::Range<usize>) -> std::borrow::Cow<'_, str> {
        match self.text.get_byte_slice(range) {
            Some(slice) => slice.into(),
            None => std::borrow::Cow::Borrowed(""),
        }
    }

    /// Apply one `didChange` batch and re-parse against the edited tree.
    ///
    /// Changes are sequential: each range refers to the document as of the
    /// previous change in the same batch.
    pub fn apply(&mut self, changes: &[Change], version: i32) {
        let mut full_reparse = false;
        for change in changes {
            match &change.range {
                Some(range) => self.splice(*range, &change.text),
                None => {
                    self.text = Rope::from_str(&change.text);
                    full_reparse = true;
                }
            }
        }
        self.version = version;

        let old = (!full_reparse).then_some(&self.tree);
        // A `None` here means the parser produced no tree at all; the last good
        // tree is kept — see specs/02, "Grammar parse error".
        if let Some(tree) = parse_rope(&mut self.parser, &self.text, old) {
            self.tree = tree;
        }
    }

    /// Byte offset of a position. Out-of-range positions clamp: a panic here
    /// would take the session down over a stale client range.
    #[must_use]
    pub fn offset(&self, position: Position) -> usize {
        let line = position.line as usize;
        if line >= self.text.len_lines() {
            return self.text.len_bytes();
        }

        let slice = self.text.line(line);
        let column = position.character as usize;
        let within_line = match self.encoding {
            PositionEncoding::Utf16 => slice.utf16_cu_to_char(column.min(slice.len_utf16_cu())),
            PositionEncoding::Utf8 => slice.byte_to_char(column.min(slice.len_bytes())),
        };

        self.text.line_to_byte(line) + slice.char_to_byte(within_line)
    }

    /// Position of a byte offset, in the negotiated encoding. Offsets past the
    /// end clamp to the end of the buffer.
    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len_bytes());
        let line = self.text.byte_to_line(offset);
        let slice = self.text.line(line);
        let within_line = offset - self.text.line_to_byte(line);
        let character = match self.encoding {
            PositionEncoding::Utf16 => slice.char_to_utf16_cu(slice.byte_to_char(within_line)),
            PositionEncoding::Utf8 => within_line,
        };

        Position {
            line: u32::try_from(line).unwrap_or(u32::MAX),
            character: u32::try_from(character).unwrap_or(u32::MAX),
        }
    }

    fn splice(&mut self, range: Range, text: &str) {
        let start_byte = self.offset(range.start);
        let old_end_byte = self.offset(range.end).max(start_byte);
        let start_position = self.point(start_byte);
        let old_end_position = self.point(old_end_byte);

        let start_char = self.text.byte_to_char(start_byte);
        let old_end_char = self.text.byte_to_char(old_end_byte);
        self.text.remove(start_char..old_end_char);
        self.text.insert(start_char, text);

        let new_end_byte = start_byte + text.len();
        let new_end_position = self.point(new_end_byte);
        self.tree.edit(&InputEdit {
            start_byte,
            old_end_byte,
            new_end_byte,
            start_position,
            old_end_position,
            new_end_position,
        });
    }

    /// tree-sitter's own coordinates: row, and column in **bytes**, regardless
    /// of the encoding negotiated with the client.
    fn point(&self, byte: usize) -> Point {
        let byte = byte.min(self.text.len_bytes());
        let row = self.text.byte_to_line(byte);
        Point {
            row,
            column: byte - self.text.line_to_byte(row),
        }
    }
}

/// Feed the rope to the parser chunk by chunk rather than flattening it.
fn parse_rope(parser: &mut Parser, rope: &Rope, old: Option<&Tree>) -> Option<Tree> {
    parser.parse_with_options(
        &mut |byte, _| {
            let Some((chunk, chunk_start, _, _)) = rope.get_chunk_at_byte(byte) else {
                return &[][..];
            };
            &chunk.as_bytes()[byte - chunk_start..]
        },
        old,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(text: &str) -> Buffer {
        Buffer::open(Language::Python, text, 1, PositionEncoding::Utf16).expect("Python grammar")
    }

    fn change(range: Option<Range>, text: &str) -> Change {
        Change {
            range,
            text: text.to_string(),
        }
    }

    fn range(start: (u32, u32), end: (u32, u32)) -> Range {
        Range::new(Position::new(start.0, start.1), Position::new(end.0, end.1))
    }

    #[test]
    fn an_edit_updates_both_the_text_and_the_tree() {
        let mut buffer = open("frappe.get_doc(\"Task\")\n");
        buffer.apply(&[change(Some(range((0, 16), (0, 20))), "Sales Invoice")], 2);

        assert_eq!(buffer.text(), "frappe.get_doc(\"Sales Invoice\")\n");
        assert_eq!(buffer.version(), 2);
        assert_eq!(
            buffer.tree().root_node().to_sexp(),
            open(&buffer.text()).tree().root_node().to_sexp()
        );
    }

    #[test]
    fn changes_in_one_batch_are_sequential() {
        let mut buffer = open("a\nb\nc\n");
        buffer.apply(
            &[
                change(Some(range((1, 0), (1, 1))), "bb"),
                change(Some(range((1, 2), (1, 2))), "!"),
            ],
            2,
        );

        assert_eq!(buffer.text(), "a\nbb!\nc\n");
    }

    #[test]
    fn utf16_columns_count_code_units() {
        // "🧾" is two UTF-16 code units, four UTF-8 bytes, one char.
        let mut buffer = open("x = \"🧾 receipt\"\n");
        buffer.apply(&[change(Some(range((0, 8), (0, 15))), "invoice")], 2);

        assert_eq!(buffer.text(), "x = \"🧾 invoice\"\n");
    }

    #[test]
    fn utf8_columns_count_bytes() {
        let mut buffer = Buffer::open(
            Language::Python,
            "x = \"🧾 receipt\"\n",
            1,
            PositionEncoding::Utf8,
        )
        .expect("Python grammar");
        buffer.apply(&[change(Some(range((0, 10), (0, 17))), "invoice")], 2);

        assert_eq!(buffer.text(), "x = \"🧾 invoice\"\n");
    }

    #[test]
    fn offset_and_position_round_trip_through_astral_characters() {
        let buffer = open("x = \"🧾 receipt\"\n");
        for offset in [0, 4, 5, 9, 10, 17] {
            assert_eq!(buffer.offset(buffer.position(offset)), offset);
        }
    }

    #[test]
    fn a_whole_document_change_replaces_the_buffer() {
        let mut buffer = open("old = 1\n");
        buffer.apply(&[change(None, "new = 2\n")], 2);

        assert_eq!(buffer.text(), "new = 2\n");
        assert_eq!(
            buffer.tree().root_node().to_sexp(),
            open("new = 2\n").tree().root_node().to_sexp()
        );
    }

    #[test]
    fn out_of_range_positions_clamp_rather_than_panic() {
        let mut buffer = open("short\n");
        buffer.apply(&[change(Some(range((99, 99), (120, 0))), "!")], 2);

        assert_eq!(buffer.text(), "short\n!");
    }

    #[test]
    fn a_buffer_that_does_not_parse_still_yields_a_tree() {
        // Half-typed code produces ERROR nodes, not a missing tree — the
        // last-good-tree fallback of specs/02 is for the `None` case only.
        let buffer = open("frappe.get_doc(\n");
        assert!(buffer.tree().root_node().has_error());
    }
}
