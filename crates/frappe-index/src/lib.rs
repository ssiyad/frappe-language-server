//! Workspace discovery, the metadata graph, layer merge and the on-disk cache.
//!
//! Discovery is implemented here; see `docs/specs/03-workspace-model.md`. The
//! metadata graph of `docs/specs/04-metadata-index.md` arrives with 005.

#![warn(missing_docs)]

pub mod app;
pub mod artifacts;
pub mod detect;
pub mod installed;
pub mod site;
pub mod walk;
pub mod workspace;

mod problem;
mod scrub;

pub use app::{App, Module};
pub use artifacts::{Artifacts, Record};
pub use detect::{Detection, Mode};
pub use scrub::scrub;
pub use site::Site;
pub use workspace::{Problem, Workspace};
