//! The on-disk index cache — [31](../../../docs/specs/31-performance.md).
//!
//! A cache is an optimisation and nothing more: a corrupt, stale or absent one
//! falls back to a full scan, silently. Anything else trades a fast start for
//! a wrong answer, and a wrong answer here is a wrong answer everywhere.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::index::Index;

/// Bumped whenever the cached types change shape, or the scan records
/// different sites for the same files. An old file is then simply not this
/// format, which is the same as absent.
pub const SCHEMA_VERSION: u32 = 7;

/// Default cache directory, relative to the bench root — spec 30.
pub const DIRECTORY: &str = ".frappe-lsp";

/// What one indexed file looked like when it was read.
///
/// Content hashing would be more precise and costs a read of every file, which
/// is most of what the cache exists to avoid. `(mtime, len)` is what every
/// build system uses for the same reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Stamp {
    path: PathBuf,
    modified: Option<u128>,
    len: u64,
}

impl Stamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            modified: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|since| since.as_nanos()),
            len: metadata.len(),
        })
    }

    /// A stamp for a file that is not there. An input that stays absent is
    /// unchanged; one that appears is not.
    fn absent(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            modified: None,
            len: 0,
        }
    }

    /// Whether the file on disk still matches.
    fn current(&self) -> bool {
        match Stamp::of(&self.path) {
            Some(now) => now == *self,
            None => self.modified.is_none() && self.len == 0,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Cached {
    schema: u32,
    root: PathBuf,
    index: Index,
    stamps: Vec<Stamp>,
    /// The files that decide the app set. Stamped separately because a change
    /// to one cannot be patched — it invalidates the whole cache.
    discovery: Vec<Stamp>,
}

/// The cache file for a bench.
#[must_use]
pub fn path_for(root: &Path, directory: &Path) -> PathBuf {
    let base = if directory.is_absolute() {
        directory.to_path_buf()
    } else {
        root.join(directory)
    };
    base.join(format!("index.v{SCHEMA_VERSION}.bin"))
}

/// Write the index beside the bench.
///
/// # Errors
///
/// Any I/O or encoding failure. The caller logs and carries on: failing to
/// write a cache costs a slower start next time and nothing else.
pub fn save(index: &Index, root: &Path, directory: &Path) -> std::io::Result<()> {
    let cached = Cached {
        schema: SCHEMA_VERSION,
        root: root.to_path_buf(),
        stamps: index
            .indexed_files()
            .filter_map(|p| Stamp::of(&p))
            .collect(),
        // A missing input stamps as absent, and staying absent is a match.
        discovery: index
            .discovery_inputs()
            .iter()
            .map(|path| Stamp::of(path).unwrap_or_else(|| Stamp::absent(path)))
            .collect(),
        index: index.clone(),
    };

    let encoded = bincode::serde::encode_to_vec(&cached, bincode::config::standard())
        .map_err(std::io::Error::other)?;

    let path = path_for(root, directory);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Written beside and renamed: a half-written cache that happens to parse
    // is the one failure mode this design cannot detect.
    let temporary = path.with_extension("bin.tmp");
    std::fs::write(&temporary, encoded)?;
    std::fs::rename(&temporary, &path)
}

/// Load the index for `root`, re-indexing whatever changed since it was
/// written.
///
/// `None` means "start from scratch" for every reason a cache can fail: absent,
/// truncated, written by another version, written for another bench, or so
/// stale that reusing it would cost more than a rebuild.
#[must_use]
pub fn load(root: &Path, directory: &Path) -> Option<Index> {
    let bytes = std::fs::read(path_for(root, directory)).ok()?;
    let (cached, _): (Cached, usize) =
        bincode::serde::decode_from_slice(&bytes, bincode::config::standard()).ok()?;

    if cached.schema != SCHEMA_VERSION || cached.root != root {
        return None;
    }

    // An app installed or uninstalled since the cache was written changes both
    // the app set and the merge order. Reusing the cache then serves a bench
    // that no longer exists, silently, which is worse than any slow start.
    if !cached.discovery.iter().all(Stamp::current) {
        tracing::debug!("the installed apps changed; rebuilding rather than patching");
        return None;
    }

    let mut index = cached.index;
    let mut stale: Vec<PathBuf> = cached
        .stamps
        .iter()
        .filter(|stamp| !stamp.current())
        .map(|stamp| stamp.path.clone())
        .collect();

    // A file that appeared since the cache was written has no stamp to fail
    // and is absent from the artifact lists the cache carries, so discovery is
    // re-run before the sets are compared. It is the cheap half of a build —
    // tens of milliseconds against hundreds — and skipping it buys a fast
    // start at the price of a wrong answer.
    index.refresh_workspace(crate::workspace::Workspace::discover(root));

    let known: std::collections::HashSet<&PathBuf> =
        cached.stamps.iter().map(|stamp| &stamp.path).collect();
    stale.extend(
        index
            .discoverable_files()
            .filter(|path| !known.contains(path)),
    );

    // Past a certain fraction, patching costs more than rebuilding and carries
    // more risk of a subtle divergence.
    if stale.len() > cached.stamps.len() / 2 {
        return None;
    }

    for path in &stale {
        index.reindex(path);
    }

    tracing::debug!(
        files = cached.stamps.len(),
        stale = stale.len(),
        "index loaded from cache"
    );
    Some(index)
}

/// Remove the cache file. Used when it cannot be trusted.
pub fn discard(root: &Path, directory: &Path) {
    let _ = std::fs::remove_file(path_for(root, directory));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_copy(name: &str) -> PathBuf {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench");
        let target =
            std::env::temp_dir().join(format!("frappe-lsp-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&target);
        copy_tree(&source, &target);
        target
    }

    fn copy_tree(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("destination");
        for entry in std::fs::read_dir(from).expect("readable source").flatten() {
            let source = entry.path();
            let target = to.join(entry.file_name());
            if source.is_dir() {
                copy_tree(&source, &target);
            } else {
                std::fs::copy(&source, &target).expect("copy");
            }
        }
    }

    #[test]
    fn a_saved_index_loads_back_identically() {
        let bench = fixture_copy("roundtrip");
        let built = Index::build(&bench);
        save(&built, &bench, Path::new(DIRECTORY)).expect("save");

        let loaded = load(&bench, Path::new(DIRECTORY)).expect("a cache");
        // The fingerprint is the canonical form: symbol ids are insertion
        // ordered, so in-memory equality is the wrong question.
        assert_eq!(loaded.fingerprint(), built.fingerprint());

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn a_changed_file_is_re_indexed_on_load() {
        let bench = fixture_copy("stale");
        let built = Index::build(&bench);
        save(&built, &bench, Path::new(DIRECTORY)).expect("save");

        // Rename a DocType on disk, behind the cache's back.
        let path = bench
            .join("apps/library/library/library/doctype/library_settings/library_settings.json");
        let text = std::fs::read_to_string(&path).expect("readable");
        std::fs::write(
            &path,
            text.replace("Library Settings", "Library Preferences"),
        )
        .expect("write");

        let loaded = load(&bench, Path::new(DIRECTORY)).expect("a cache");
        assert!(loaded.doctype("Library Preferences").is_some());
        assert!(loaded.doctype("Library Settings").is_none());
        // And it matches what a full rebuild would have produced.
        assert_eq!(loaded.fingerprint(), Index::build(&bench).fingerprint());

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn an_incremental_occurrence_update_matches_a_full_scan() {
        let bench = fixture_copy("occurrences");
        let mut built = Index::build(&bench);
        built.scan_occurrences();

        // Touch a file that both declares fields and names other DocTypes.
        let path =
            bench.join("apps/library/library/library/doctype/library_member/library_member.py");
        let text = std::fs::read_to_string(&path).expect("readable");
        std::fs::write(&path, format!("{text}\n\ndef extra():\n\tpass\n")).expect("write");

        built.reindex(&path);

        let mut fresh = Index::build(&bench);
        fresh.scan_occurrences();
        assert_eq!(
            built.occurrences(),
            fresh.occurrences(),
            "an incremental update diverged from a full scan"
        );

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn a_file_created_while_the_server_was_off_is_picked_up() {
        let bench = fixture_copy("created");
        let built = Index::build(&bench);
        save(&built, &bench, Path::new(DIRECTORY)).expect("save");

        // A new module has no stamp to fail and appears in no artifact list:
        // if the cache trusts what it stored, this method stays invisible for
        // as long as the file is never edited again.
        std::fs::write(
            bench.join("apps/library/library/renewals.py"),
            "import frappe\n\n\n@frappe.whitelist()\ndef renew(loan):\n\treturn loan\n",
        )
        .expect("write");

        let loaded = load(&bench, Path::new(DIRECTORY)).expect("a cache");
        assert!(loaded.whitelisted("library.renewals.renew").is_some());
        assert_eq!(loaded.fingerprint(), Index::build(&bench).fingerprint());

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn installing_an_app_invalidates_the_whole_cache() {
        let bench = fixture_copy("installed");
        let built = Index::build(&bench);
        save(&built, &bench, Path::new(DIRECTORY)).expect("save");
        assert!(load(&bench, Path::new(DIRECTORY)).is_some());

        // `library-old` is on disk and uninstalled; installing it changes both
        // the app set and the merge order behind it.
        let apps = bench.join("sites/apps.txt");
        let text = std::fs::read_to_string(&apps).expect("apps.txt");
        std::fs::write(&apps, format!("{text}\nlibrary_archive")).expect("write");

        // No patch can express this, so the cache refuses itself.
        assert!(load(&bench, Path::new(DIRECTORY)).is_none());

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn a_truncated_cache_is_simply_absent() {
        let bench = fixture_copy("truncated");
        let built = Index::build(&bench);
        save(&built, &bench, Path::new(DIRECTORY)).expect("save");

        let path = path_for(&bench, Path::new(DIRECTORY));
        let bytes = std::fs::read(&path).expect("readable");
        std::fs::write(&path, &bytes[..bytes.len() / 3]).expect("truncate");

        // No error, no panic: the caller falls back to a full scan.
        assert!(load(&bench, Path::new(DIRECTORY)).is_none());

        std::fs::remove_dir_all(&bench).expect("cleanup");
    }

    #[test]
    fn a_cache_written_for_another_bench_is_refused() {
        let bench = fixture_copy("elsewhere");
        let built = Index::build(&bench);
        save(&built, &bench, Path::new(DIRECTORY)).expect("save");

        let moved = bench.with_extension("moved");
        let _ = std::fs::remove_dir_all(&moved);
        std::fs::rename(&bench, &moved).expect("rename");

        // Paths inside the cache point at the old root; reusing it would give
        // every navigation target a path that does not exist.
        assert!(load(&moved, Path::new(DIRECTORY)).is_none());

        std::fs::remove_dir_all(&moved).expect("cleanup");
    }

    #[test]
    fn an_absent_cache_is_not_an_error() {
        let bench = std::env::temp_dir().join("frappe-lsp-cache-missing");
        assert!(load(&bench, Path::new(DIRECTORY)).is_none());
    }
}
