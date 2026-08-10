use std::fmt;
use std::path::{Path, PathBuf};

/// Something discovery could not use, kept rather than thrown away.
///
/// Discovery never fails: a workspace with one bad file still indexes
/// ([02](../../../docs/specs/02-architecture.md)). The path travels with the
/// message because a diagnostic has to land somewhere
/// ([23](../../../docs/specs/23-diagnostics.md)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// The file that caused it.
    pub path: PathBuf,
    /// What went wrong, in the words a user would need.
    pub message: String,
}

impl Problem {
    pub(crate) fn new(path: impl AsRef<Path>, message: impl Into<String>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
            message: message.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}
