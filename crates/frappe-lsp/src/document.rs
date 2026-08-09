//! Open buffers. Incremental `textDocument/didChange` applied to a rope.

use ropey::Rope;
use tower_lsp_server::ls_types::{
    Position, PositionEncodingKind, Range, TextDocumentContentChangeEvent,
};

/// The encoding both sides agreed on at `initialize` — spec 30 falls back to
/// UTF-16 when the client does not advertise UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PositionEncoding {
    Utf8,
    #[default]
    Utf16,
}

impl PositionEncoding {
    pub fn kind(self) -> PositionEncodingKind {
        match self {
            Self::Utf8 => PositionEncodingKind::UTF8,
            Self::Utf16 => PositionEncodingKind::UTF16,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    text: Rope,
    version: i32,
}

impl Document {
    pub fn new(text: &str, version: i32) -> Self {
        Self {
            text: Rope::from_str(text),
            version,
        }
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn rope(&self) -> &Rope {
        &self.text
    }

    pub fn text(&self) -> String {
        self.text.to_string()
    }

    /// Apply one `didChange` batch. Changes are sequential: each range refers
    /// to the document as of the previous change in the same batch.
    pub fn apply(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
        encoding: PositionEncoding,
    ) {
        for change in changes {
            match change.range {
                Some(range) => self.splice(range, &change.text, encoding),
                None => self.text = Rope::from_str(&change.text),
            }
        }
        self.version = version;
    }

    fn splice(&mut self, range: Range, text: &str, encoding: PositionEncoding) {
        let start = self.offset(range.start, encoding);
        let end = self.offset(range.end, encoding).max(start);
        self.text.remove(start..end);
        self.text.insert(start, text);
    }

    /// Char offset of a position. Out-of-range positions clamp rather than
    /// panic — a panic here would take the whole session down with it.
    fn offset(&self, position: Position, encoding: PositionEncoding) -> usize {
        let line = position.line as usize;
        if line >= self.text.len_lines() {
            return self.text.len_chars();
        }

        let line_start = self.text.line_to_char(line);
        let slice = self.text.line(line);
        let column = position.character as usize;

        let within_line = match encoding {
            PositionEncoding::Utf16 => slice.utf16_cu_to_char(column.min(slice.len_utf16_cu())),
            PositionEncoding::Utf8 => slice.byte_to_char(column.min(slice.len_bytes())),
        };

        line_start + within_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(range: Option<Range>, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range,
            range_length: None,
            text: text.to_string(),
        }
    }

    fn range(start: (u32, u32), end: (u32, u32)) -> Range {
        Range {
            start: Position::new(start.0, start.1),
            end: Position::new(end.0, end.1),
        }
    }

    #[test]
    fn incremental_edits_match_a_fresh_parse() {
        let mut document = Document::new("frappe.get_doc(\"Task\")\n", 1);
        document.apply(
            vec![change(Some(range((0, 16), (0, 20))), "Sales Invoice")],
            2,
            PositionEncoding::Utf16,
        );

        assert_eq!(document.text(), "frappe.get_doc(\"Sales Invoice\")\n");
        assert_eq!(document.version(), 2);
    }

    #[test]
    fn changes_in_one_batch_are_sequential() {
        let mut document = Document::new("a\nb\nc\n", 1);
        document.apply(
            vec![
                change(Some(range((1, 0), (1, 1))), "bb"),
                change(Some(range((1, 2), (1, 2))), "!"),
            ],
            2,
            PositionEncoding::Utf16,
        );

        assert_eq!(document.text(), "a\nbb!\nc\n");
    }

    #[test]
    fn utf16_columns_count_code_units() {
        // "🧾" is two UTF-16 code units, four UTF-8 bytes, one char.
        let mut document = Document::new("x = \"🧾 receipt\"\n", 1);
        document.apply(
            vec![change(Some(range((0, 8), (0, 15))), "invoice")],
            2,
            PositionEncoding::Utf16,
        );

        assert_eq!(document.text(), "x = \"🧾 invoice\"\n");
    }

    #[test]
    fn utf8_columns_count_bytes() {
        let mut document = Document::new("x = \"🧾 receipt\"\n", 1);
        document.apply(
            vec![change(Some(range((0, 10), (0, 17))), "invoice")],
            2,
            PositionEncoding::Utf8,
        );

        assert_eq!(document.text(), "x = \"🧾 invoice\"\n");
    }

    #[test]
    fn a_whole_document_change_replaces_the_buffer() {
        let mut document = Document::new("old\n", 1);
        document.apply(vec![change(None, "new\n")], 2, PositionEncoding::Utf16);

        assert_eq!(document.text(), "new\n");
    }

    #[test]
    fn out_of_range_positions_clamp() {
        let mut document = Document::new("short\n", 1);
        document.apply(
            vec![change(Some(range((99, 99), (120, 0))), "!")],
            2,
            PositionEncoding::Utf16,
        );

        assert_eq!(document.text(), "short\n!");
    }
}
