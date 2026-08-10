//! Workspace discovery, the metadata graph, layer merge and the on-disk cache.
//!
//! Discovery is `docs/specs/03-workspace-model.md`; the metadata graph it feeds
//! is `docs/specs/04-metadata-index.md`. The on-disk cache of both arrives with
//! 009.

#![warn(missing_docs)]

pub mod app;
pub mod artifacts;
pub mod cache;
pub mod controller;
pub mod detect;
pub mod doctype;
pub mod fieldtypes;
pub mod index;
pub mod installed;
pub mod intern;
pub mod paths;
pub mod resolve;
pub mod site;
pub mod span;
pub mod walk;
pub mod whitelist;
pub mod workspace;

mod fixtures;
mod problem;
mod scrub;

pub use app::{App, Module};
pub use artifacts::{Artifacts, Record};
pub use controller::ControllerRef;
pub use detect::{Detection, Mode};
pub use doctype::{DocType, Field, Layer, Options, Provenance};
pub use index::{Confidence, Conflict, ConflictCode, Index, IndexStatus};
pub use intern::{Interner, Symbol};
pub use paths::{Kind, Resolution, Resolver};
pub use resolve::{Resolved, Target};
pub use scrub::scrub;
pub use site::Site;
pub use span::Range;
pub use whitelist::WhitelistedMethod;
pub use workspace::{Problem, Workspace};
