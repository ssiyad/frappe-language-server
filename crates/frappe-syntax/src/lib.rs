//! tree-sitter grammars, queries and surface pattern extraction.
//!
//! Extraction yields unresolved names; resolution happens in the server layer,
//! so this crate MUST NOT depend on `frappe-index` — see
//! `docs/specs/02-architecture.md`.
//!
//! ```
//! use frappe_syntax::{Anchor, Buffer, Context, Language, PositionEncoding, extract};
//!
//! let source = "frappe.get_all(\"Sales Invoice\", fields=[\"customer\"])\n";
//! let buffer = Buffer::open(Language::Python, source, 1, PositionEncoding::Utf16)?;
//! let found = extract(Language::Python, buffer.tree(), source, Context::default());
//!
//! assert_eq!(found[0].anchor, Anchor::DocType("Sales Invoice".to_string()));
//! # Ok::<(), frappe_syntax::GrammarError>(())
//! ```

#![warn(missing_docs)]

pub mod buffer;
pub mod extract;
pub mod generated;
pub mod handlers;
pub mod hooks;
pub mod language;
pub mod matches;
pub mod queries;
pub mod symbols;

mod binding;
mod read;
mod shapes;

pub use buffer::{Buffer, Change, Position, PositionEncoding, Range};
pub use extract::{Context, extract};
pub use language::{GrammarError, Language};
pub use matches::{Anchor, Binding, Match, PathKind, PatternId, Role, Slot, Span, TranslationArg};
pub use symbols::{Class, Exposure, Function, Whitelist};
