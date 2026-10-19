//! Client and Server Scripts from the live site —
//! [05](../../../docs/specs/05-live-bridge.md).
//!
//! They live in the site's database, not in any app. The server writes each one
//! to a read-only file, so any editor can open it, and the index keeps where.

use std::path::PathBuf;

/// Which of the two script DocTypes a script came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScriptKind {
    /// `Client Script`: JavaScript bound to a form or a list.
    Client,
    /// `Server Script`: restricted Python, run on an event, an API call or a
    /// schedule.
    Server,
}

/// One script, and the file it was written to.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SiteScript {
    /// Which DocType holds it.
    pub kind: ScriptKind,
    /// Its `name` on the site.
    pub name: String,
    /// The DocType it runs for: `dt`, or `reference_doctype`.
    pub doctype: Option<String>,
    /// When it runs: the view, the document event, the API method or the
    /// frequency.
    pub trigger: Option<String>,
    /// `enabled`, or not `disabled`.
    pub enabled: bool,
    /// The read-only copy.
    pub path: PathBuf,
}
