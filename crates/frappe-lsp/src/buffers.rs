//! Open buffers, and the translation between LSP types and `frappe-syntax`.
//!
//! The buffer itself — rope, cached tree, incremental `Tree::edit` — belongs to
//! `frappe-syntax`, which owns every grammar in the workspace. What lives here
//! is the protocol edge: LSP positions are line/character pairs in a negotiated
//! encoding, and the rest of the server works in bytes.

use std::collections::HashMap;

use frappe_syntax::{Buffer, Change, Language, Position, PositionEncoding, Range};
use tower_lsp_server::ls_types::{self, PositionEncodingKind, TextDocumentContentChangeEvent, Uri};

/// Every buffer the client has opened.
#[derive(Debug, Default)]
pub struct Buffers {
    open: HashMap<Uri, Buffer>,
}

impl Buffers {
    /// Parse and keep a buffer — the `didOpen` path.
    ///
    /// A file whose language has no grammar is not an error: the client may
    /// open anything, and the server simply has nothing to say about it.
    pub fn open(
        &mut self,
        uri: Uri,
        language_id: &str,
        text: &str,
        version: i32,
        encoding: PositionEncoding,
    ) -> Result<bool, frappe_syntax::GrammarError> {
        let Some(language) = language_of(&uri, language_id) else {
            return Ok(false);
        };
        let buffer = Buffer::open(language, text, version, encoding)?;
        self.open.insert(uri, buffer);
        Ok(true)
    }

    /// Every open buffer's URI. Used to leave open files to their own publish
    /// path rather than publishing over them from the index.
    #[must_use]
    pub fn uris(&self) -> Vec<Uri> {
        self.open.keys().cloned().collect()
    }

    /// Apply a `didChange` batch. `false` when the buffer was never opened,
    /// which means sync state has diverged and every later position would be
    /// computed against the wrong text.
    pub fn change(
        &mut self,
        uri: &Uri,
        changes: &[TextDocumentContentChangeEvent],
        version: i32,
    ) -> bool {
        let Some(buffer) = self.open.get_mut(uri) else {
            return false;
        };
        let changes: Vec<Change> = changes.iter().map(change).collect();
        buffer.apply(&changes, version);
        true
    }

    /// Drop a buffer — `didClose`. The tree goes with it, which is the point of
    /// only open buffers holding one.
    pub fn close(&mut self, uri: &Uri) {
        self.open.remove(uri);
    }

    /// A buffer by URI.
    #[must_use]
    pub fn get(&self, uri: &Uri) -> Option<&Buffer> {
        self.open.get(uri)
    }

    /// How many buffers are open.
    #[must_use]
    pub fn len(&self) -> usize {
        self.open.len()
    }

    /// Whether no buffer is open.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.open.is_empty()
    }
}

/// The grammar for a document.
///
/// The client's `languageId` is authoritative — it knows about buffers with no
/// path at all — and the extension is the fallback for clients that send
/// something unfamiliar.
#[must_use]
pub fn language_of(uri: &Uri, language_id: &str) -> Option<Language> {
    match language_id {
        "python" => Some(Language::Python),
        "javascript" | "javascriptreact" => Some(Language::JavaScript),
        "json" | "jsonc" => Some(Language::Json),
        _ => Language::from_path(uri.to_file_path()?.as_ref()),
    }
}

/// The encoding both sides agreed on, as the protocol spells it.
#[must_use]
pub fn encoding_kind(encoding: PositionEncoding) -> PositionEncodingKind {
    match encoding {
        PositionEncoding::Utf8 => PositionEncodingKind::UTF8,
        PositionEncoding::Utf16 => PositionEncodingKind::UTF16,
    }
}

/// The encoding to use, given what the client offered.
///
/// UTF-16 is the fallback every client must support
/// ([30](../../../docs/specs/30-configuration.md)).
#[must_use]
pub fn negotiate(offered: Option<&[PositionEncodingKind]>) -> PositionEncoding {
    let Some(offered) = offered else {
        return PositionEncoding::Utf16;
    };
    if offered.contains(&PositionEncodingKind::UTF8) {
        PositionEncoding::Utf8
    } else {
        // UTF-32 is legal in the protocol and no client asks for it; UTF-16 is
        // the fallback every client must support anyway.
        PositionEncoding::Utf16
    }
}

fn change(event: &TextDocumentContentChangeEvent) -> Change {
    Change {
        range: event.range.map(range),
        text: event.text.clone(),
    }
}

fn range(range: ls_types::Range) -> Range {
    Range::new(position(range.start), position(range.end))
}

fn position(position: ls_types::Position) -> Position {
    Position::new(position.line, position.character)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn uri(path: &str) -> Uri {
        Uri::from_str(path).expect("a valid URI")
    }

    fn edit(range: Option<((u32, u32), (u32, u32))>, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent {
            range: range.map(|(start, end)| ls_types::Range {
                start: ls_types::Position::new(start.0, start.1),
                end: ls_types::Position::new(end.0, end.1),
            }),
            range_length: None,
            text: text.to_string(),
        }
    }

    #[test]
    fn an_edit_reaches_the_buffer_and_reparses() {
        let mut buffers = Buffers::default();
        let uri = uri("file:///bench/apps/library/library/api.py");
        buffers
            .open(
                uri.clone(),
                "python",
                "frappe.get_doc(\"Task\")\n",
                1,
                PositionEncoding::Utf16,
            )
            .expect("the grammar loads");

        assert!(buffers.change(&uri, &[edit(Some(((0, 16), (0, 20))), "Sales Invoice")], 2));

        let buffer = buffers.get(&uri).expect("still open");
        assert_eq!(buffer.text(), "frappe.get_doc(\"Sales Invoice\")\n");
        assert_eq!(buffer.version(), 2);
        // The tree moved with the text, not just the rope.
        assert!(!buffer.tree().root_node().has_error());
    }

    #[test]
    fn a_change_to_an_unopened_buffer_is_reported() {
        let mut buffers = Buffers::default();
        assert!(!buffers.change(&uri("file:///nowhere.py"), &[edit(None, "x")], 2));
    }

    #[test]
    fn closing_drops_the_tree() {
        let mut buffers = Buffers::default();
        let uri = uri("file:///bench/x.py");
        buffers
            .open(uri.clone(), "python", "x = 1\n", 1, PositionEncoding::Utf16)
            .expect("the grammar loads");
        assert_eq!(buffers.len(), 1);

        buffers.close(&uri);
        assert!(buffers.is_empty());
    }

    #[test]
    fn a_language_with_no_grammar_is_not_an_error() {
        let mut buffers = Buffers::default();
        let opened = buffers
            .open(
                uri("file:///bench/README.md"),
                "markdown",
                "# Title\n",
                1,
                PositionEncoding::Utf16,
            )
            .expect("no grammar is not a failure");

        assert!(!opened);
        assert!(buffers.is_empty());
    }

    #[test]
    fn the_extension_decides_when_the_language_id_is_unfamiliar() {
        assert_eq!(
            language_of(&uri("file:///x/hooks.py"), "python3"),
            Some(Language::Python)
        );
        assert_eq!(
            language_of(&uri("file:///x/task.js"), "babel"),
            Some(Language::JavaScript)
        );
        assert_eq!(language_of(&uri("file:///x/notes.txt"), "plaintext"), None);
    }

    #[test]
    fn utf16_is_the_fallback_when_nothing_is_offered() {
        assert_eq!(negotiate(None), PositionEncoding::Utf16);
        assert_eq!(
            negotiate(Some(&[PositionEncodingKind::UTF16])),
            PositionEncoding::Utf16
        );
        assert_eq!(
            negotiate(Some(&[
                PositionEncodingKind::UTF8,
                PositionEncodingKind::UTF16
            ])),
            PositionEncoding::Utf8
        );
    }
}
