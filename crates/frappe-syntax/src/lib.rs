//! tree-sitter grammars, queries and surface pattern extraction.
//!
//! Extraction yields unresolved names; resolution happens in the server layer,
//! so this crate MUST NOT depend on `frappe-index` — see
//! `docs/specs/02-architecture.md`. Empty until 006.

#![warn(missing_docs)]
