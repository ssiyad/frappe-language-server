//! Directory traversal with the built-in exclusions of spec 03.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Directories never traversed. `sites/` is absent because it is never walked
/// at all — the two files worth reading there are opened by name
/// ([`crate::site`]).
/// `dist` is esbuild's output directory; frappe's own `.gitignore` names it.
/// The bundles under it are minified copies of sources already indexed, and
/// reading them reports the minifier's rewrites as the author's mistakes.
const EXCLUDED: [&str; 6] = [
    "node_modules",
    ".git",
    "env",
    "__pycache__",
    ".venv",
    "dist",
];

/// True for a directory name the walker must not descend into.
#[must_use]
pub fn is_excluded(name: &OsStr) -> bool {
    let Some(name) = name.to_str() else {
        return false;
    };
    EXCLUDED.contains(&name) || name.ends_with(".egg-info")
}

/// Every file below `root`, skipping excluded directories.
///
/// Symlinked directories are not followed. A bench is full of them —
/// `sites/assets` alone links back into every app — and following them turns a
/// walk into a cycle.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct Walk {
    pending: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

impl Walk {
    /// Walk `root`. A missing or unreadable directory yields nothing rather
    /// than failing: discovery must survive a workspace it cannot fully read.
    #[must_use]
    pub fn new(root: impl AsRef<Path>) -> Self {
        Self {
            pending: vec![root.as_ref().to_path_buf()],
            files: Vec::new(),
        }
    }
}

impl Iterator for Walk {
    type Item = PathBuf;

    fn next(&mut self) -> Option<PathBuf> {
        loop {
            if let Some(file) = self.files.pop() {
                return Some(file);
            }

            let directory = self.pending.pop()?;
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };

            for entry in entries.flatten() {
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                if kind.is_symlink() {
                    continue;
                }
                if kind.is_dir() {
                    if !is_excluded(&entry.file_name()) {
                        self.pending.push(entry.path());
                    }
                } else {
                    self.files.push(entry.path());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excluded_directories_are_named_by_the_spec() {
        for name in [
            "node_modules",
            ".git",
            "env",
            "__pycache__",
            "frappe.egg-info",
        ] {
            assert!(is_excluded(OsStr::new(name)), "{name}");
        }
        for name in ["public", "doctype", "environment", "gitignore"] {
            assert!(!is_excluded(OsStr::new(name)), "{name}");
        }
    }

    #[test]
    fn a_missing_root_yields_nothing() {
        assert_eq!(Walk::new("/nonexistent/frappe-lsp").count(), 0);
    }
}
