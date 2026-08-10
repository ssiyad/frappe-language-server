//! Byte ranges in files on disk, as the protocol wants them.
//!
//! The index measures in bytes ([04](../../../docs/specs/04-metadata-index.md))
//! because that is what a file is; LSP measures in lines and code units in a
//! negotiated encoding. The conversion needs the target file's text, so it
//! happens here, at the edge, and only for the handful of ranges one request
//! actually returns.

use std::path::Path;

use frappe_index::Range as ByteRange;
use frappe_syntax::PositionEncoding;
use tower_lsp_server::ls_types::{Location, LocationLink, Position, Range, Uri};

/// A target to navigate to.
#[derive(Debug, Clone)]
pub struct Target {
    /// The file.
    pub path: std::path::PathBuf,
    /// The whole definition — the class, the field object, the `def`.
    pub range: ByteRange,
    /// The name alone, which is what the editor should select.
    pub selection: ByteRange,
}

impl Target {
    /// A target whose selection is the whole range.
    #[must_use]
    pub fn whole(path: impl Into<std::path::PathBuf>, range: ByteRange) -> Self {
        let path = path.into();
        Self {
            path,
            range,
            selection: range,
        }
    }

    /// A target with a narrower selection inside it.
    #[must_use]
    pub fn named(
        path: impl Into<std::path::PathBuf>,
        range: ByteRange,
        selection: ByteRange,
    ) -> Self {
        Self {
            path: path.into(),
            range,
            selection,
        }
    }

    /// The whole file, for a target that is a module rather than a symbol.
    #[must_use]
    pub fn file(path: impl Into<std::path::PathBuf>) -> Self {
        Self::whole(path, ByteRange::new(0, 0))
    }
}

/// Turn targets into `LocationLink`s, or plain `Location`s for clients that
/// cannot take them ([30](../../../docs/specs/30-configuration.md)).
///
/// A file that cannot be read is dropped rather than reported: the index may be
/// a moment behind the filesystem, and half an answer beats an error.
#[must_use]
pub fn links(
    targets: &[Target],
    origin: Option<Range>,
    encoding: PositionEncoding,
    location_link: bool,
) -> Option<GotoResponse> {
    let mut links = Vec::new();
    let mut locations = Vec::new();

    for target in targets {
        let Ok(text) = std::fs::read_to_string(&target.path) else {
            continue;
        };
        let Some(uri) = uri_of(&target.path) else {
            continue;
        };
        let index = LineIndex::new(&text, encoding);
        let range = index.range(target.range);
        let selection = index.range(target.selection);

        if location_link {
            links.push(LocationLink {
                origin_selection_range: origin,
                target_uri: uri,
                target_range: range,
                target_selection_range: selection,
            });
        } else {
            locations.push(Location {
                uri,
                range: selection,
            });
        }
    }

    if location_link {
        (!links.is_empty()).then_some(GotoResponse::Links(links))
    } else {
        (!locations.is_empty()).then_some(GotoResponse::Locations(locations))
    }
}

/// Either shape of a definition response.
#[derive(Debug, Clone)]
pub enum GotoResponse {
    /// `LocationLink[]` — carries the origin range, so the editor highlights
    /// the name that was navigated from.
    Links(Vec<LocationLink>),
    /// `Location[]`, for clients without `linkSupport`.
    Locations(Vec<Location>),
}

impl From<GotoResponse> for tower_lsp_server::ls_types::GotoDefinitionResponse {
    fn from(response: GotoResponse) -> Self {
        match response {
            GotoResponse::Links(links) => Self::Link(links),
            GotoResponse::Locations(locations) => Self::Array(locations),
        }
    }
}

/// `file://` URI for a path.
#[must_use]
pub fn uri_of(path: &Path) -> Option<Uri> {
    Uri::from_file_path(path)
}

/// Line starts for one file, so byte offsets become positions.
///
/// Built per request and thrown away. The alternative — caching a line index
/// per indexed file — would hold a second copy of every DocType JSON in memory
/// to save a scan that only runs when the user navigates.
pub struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    encoding: PositionEncoding,
}

impl<'a> LineIndex<'a> {
    /// Scan `text` once.
    #[must_use]
    pub fn new(text: &'a str, encoding: PositionEncoding) -> Self {
        let mut starts = vec![0];
        starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(offset, _)| offset + 1),
        );
        Self {
            text,
            starts,
            encoding,
        }
    }

    /// The position of a byte offset, clamped to the end of the file.
    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        let offset = offset.min(self.text.len());
        let line = self.starts.partition_point(|start| *start <= offset) - 1;
        let start = self.starts[line];

        let column = match self.encoding {
            PositionEncoding::Utf8 => offset - start,
            PositionEncoding::Utf16 => self.text[start..offset].chars().map(char::len_utf16).sum(),
        };

        Position::new(
            u32::try_from(line).unwrap_or(u32::MAX),
            u32::try_from(column).unwrap_or(u32::MAX),
        )
    }

    /// The LSP range for a byte range.
    #[must_use]
    pub fn range(&self, range: ByteRange) -> Range {
        Range {
            start: self.position(range.start as usize),
            end: self.position(range.end as usize),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_offset_becomes_a_line_and_column() {
        let text = "one\ntwo\nthree\n";
        let index = LineIndex::new(text, PositionEncoding::Utf16);

        assert_eq!(index.position(0), Position::new(0, 0));
        assert_eq!(index.position(4), Position::new(1, 0));
        assert_eq!(index.position(6), Position::new(1, 2));
        assert_eq!(index.position(8), Position::new(2, 0));
    }

    #[test]
    fn utf16_columns_count_code_units() {
        // The emoji is one char, two UTF-16 units, four UTF-8 bytes.
        let text = "x = \"🧾 receipt\"\n";
        let utf16 = LineIndex::new(text, PositionEncoding::Utf16);
        let utf8 = LineIndex::new(text, PositionEncoding::Utf8);

        let offset = text.find("receipt").expect("present");
        assert_eq!(utf16.position(offset).character, 8);
        assert_eq!(utf8.position(offset).character, 10);
    }

    #[test]
    fn an_offset_past_the_end_clamps() {
        let text = "short\n";
        let index = LineIndex::new(text, PositionEncoding::Utf16);
        assert_eq!(index.position(9_999), index.position(text.len()));
    }

    #[test]
    fn a_file_that_cannot_be_read_is_dropped_not_reported() {
        let targets = [Target::file("/nonexistent/frappe-lsp/x.py")];
        assert!(links(&targets, None, PositionEncoding::Utf16, true).is_none());
    }
}
