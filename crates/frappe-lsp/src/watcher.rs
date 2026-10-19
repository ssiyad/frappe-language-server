//! Staying current with the filesystem — [31](../../../docs/specs/31-performance.md).
//!
//! `didSave` covers edits made in the editor. Everything else — `git checkout`,
//! `bench migrate`, a second editor — arrives here, and without it the index
//! quietly describes a bench that no longer exists.
//!
//! Spec 30 prefers the client's own watcher where it has one: the editor
//! already knows which files changed, and a second inotify tree over a bench is
//! thousands of watches for the same answer. This is the fallback.

use crate::guard::Recover;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, RecvTimeoutError, channel};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use frappe_index::Index;
use frappe_index::walk::is_excluded;
use notify::event::{EventKind, ModifyKind};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

/// Spec 31: watcher events coalesce at 250 ms per path.
const DEBOUNCE: Duration = Duration::from_millis(250);

/// A running watcher. Dropping it stops the thread and releases the watches.
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
}

impl std::fmt::Debug for FileWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FileWatcher")
    }
}

/// Watch each app package, re-indexing what changes.
///
/// # Errors
///
/// When the platform watcher cannot be created or the root cannot be watched.
/// A workspace with no watcher still works; it just goes stale, so this is
/// reported and not fatal.
pub fn start(
    roots: &[PathBuf],
    bench: Option<PathBuf>,
    index: Arc<RwLock<Index>>,
) -> notify::Result<FileWatcher> {
    let (sender, receiver) = channel();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
        if let Ok(event) = event {
            // A send failure means the drain thread is gone, which happens on
            // shutdown; there is nothing useful to do about it here.
            let _ = sender.send(event);
        }
    })?;
    // The app packages, not the bench root. A recursive watch over a bench
    // also covers `sites/`, `node_modules` and `.git` — tens of thousands of
    // inotify handles for directories the index never reads, and on a real
    // bench establishing them takes about a second.
    for root in roots {
        // `sites/` is watched flat: apps.txt and apps.json live directly in it,
        // and everything below is logs, backups and private files.
        let mode = if root.file_name().is_some_and(|name| name == "sites") {
            RecursiveMode::NonRecursive
        } else {
            RecursiveMode::Recursive
        };
        watcher.watch(root, mode)?;
    }

    // macOS reports events under the resolved path: a bench in `/var/folders`
    // is watched there but heard from at `/private/var/folders`, and no app
    // package starts with that, so every change was dropped. A bench under any
    // symlinked directory does the same.
    let aliases: Vec<(PathBuf, PathBuf)> = roots
        .iter()
        .filter_map(|root| {
            let resolved = root.canonicalize().ok()?;
            (resolved != *root).then(|| (resolved, root.clone()))
        })
        .collect();

    std::thread::Builder::new()
        .name("frappe-lsp-watcher".to_string())
        .spawn(move || drain(&receiver, &index, bench.as_deref(), &aliases))
        .map_err(notify::Error::io)?;

    Ok(FileWatcher { _watcher: watcher })
}

/// Coalesce events per path, then re-index once the path goes quiet.
///
/// A single save can produce several events — write, chmod, rename-into-place —
/// and a `git checkout` produces thousands. Re-indexing per event would spend
/// more time in the index than the editor spends waiting for it.
fn drain(
    receiver: &Receiver<Event>,
    index: &Arc<RwLock<Index>>,
    bench: Option<&Path>,
    aliases: &[(PathBuf, PathBuf)],
) {
    let mut pending: HashMap<PathBuf, Instant> = HashMap::new();

    loop {
        match receiver.recv_timeout(DEBOUNCE) {
            Ok(event) => {
                if !changes_content(&event.kind) {
                    continue;
                }
                for path in event.paths {
                    let path = as_watched(path, aliases);
                    // A new DocType arrives as a new directory, and the watch
                    // on it is added after the file inside is already written:
                    // the only event that ever names the file is this one.
                    if path.is_dir() {
                        for inside in frappe_index::walk::Walk::new(&path) {
                            if interesting(&inside) {
                                pending.insert(inside, Instant::now());
                            }
                        }
                    } else if interesting(&path) {
                        pending.insert(path, Instant::now());
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The watcher was dropped: the session is over.
            Err(RecvTimeoutError::Disconnected) => return,
        }

        let now = Instant::now();
        let ready: Vec<PathBuf> = pending
            .iter()
            .filter(|(_, seen)| now.duration_since(**seen) >= DEBOUNCE)
            .map(|(path, _)| path.clone())
            .collect();

        for path in &ready {
            pending.remove(path);
        }
        if !ready.is_empty() {
            apply(index, &ready, bench);
        }
    }
}

/// Re-index a batch under one lock.
///
/// One acquisition per batch, not per file: a `git checkout` produces hundreds
/// of ready paths at once, and taking the write lock for each starves every
/// reader in between — the index would be unavailable for the whole sweep
/// rather than once at the end of it.
fn apply(index: &Arc<RwLock<Index>>, paths: &[PathBuf], bench: Option<&Path>) {
    // A change to the installed apps changes the merge order behind every app
    // after it, which no per-file patch expresses (spec 04).
    if let Some(bench) = bench
        && paths.iter().any(|path| decides_the_app_set(path))
    {
        tracing::info!("the installed apps changed; rebuilding the index");
        let rebuilt = Index::build(bench);
        *index.write_or_recover() = rebuilt;
        // A fresh index has no reverse occurrence table, and nothing else
        // would ever fill it: `references` would answer nothing until restart.
        crate::server::scan_occurrences(index);
        return;
    }

    let mut index = index.write_or_recover();
    let mut touched = 0;
    for path in paths {
        if index.reindex(path).known {
            touched += 1;
        }
    }
    if touched > 0 {
        tracing::debug!(files = touched, "re-indexed from the filesystem");
    }
}

/// The path as the index knows it: under the root that was watched rather than
/// the resolved one the platform reported.
fn as_watched(path: PathBuf, aliases: &[(PathBuf, PathBuf)]) -> PathBuf {
    for (resolved, watched) in aliases {
        if let Ok(rest) = path.strip_prefix(resolved) {
            return watched.join(rest);
        }
    }
    path
}

/// `sites/apps.txt` and `sites/apps.json` decide which apps exist at all.
fn decides_the_app_set(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        (name == "apps.txt" || name == "apps.json" || name == "modules.txt")
            && path.parent().is_some_and(|parent| {
                parent.file_name().is_some_and(|dir| dir == "sites") || name == "modules.txt"
            })
    })
}

/// Whether an event means the file's contents changed.
///
/// Reads are events too, and the index reads every file it indexes: acting on
/// an access event makes indexing trigger indexing, which starves every reader
/// of the index lock and hangs the server. Ask for that mistake once and it
/// looks exactly like a deadlock.
fn changes_content(kind: &EventKind) -> bool {
    match kind {
        EventKind::Create(_) | EventKind::Remove(_) => true,
        EventKind::Modify(modify) => !matches!(modify, ModifyKind::Metadata(_)),
        // `Any` is what a backend reports when it cannot say; `Access` and
        // `Other` are never a content change.
        EventKind::Any => true,
        EventKind::Access(_) | EventKind::Other => false,
    }
}

/// Whether a path is one the index reads at all.
///
/// The exclusions of [03](../../../docs/specs/03-workspace-model.md) matter
/// more here than during the scan: `sites/` alone churns constantly, and every
/// event costs a wakeup.
#[must_use]
pub fn interesting(path: &Path) -> bool {
    if path
        .components()
        .any(|component| is_excluded(component.as_os_str()))
    {
        return false;
    }
    // The index reads DocType JSON, fixtures, Python and patches.txt. A `.pyc`
    // or a log rotating under `sites/` is not a schema change.
    match path.extension().and_then(|extension| extension.to_str()) {
        Some("json" | "py") => true,
        Some("txt") => path.file_name().is_some_and(|name| {
            // `apps.txt` is not indexed content, but it decides what is.
            name == "patches.txt" || name == "apps.txt" || name == "modules.txt"
        }),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_event_under_the_resolved_path_maps_back_to_the_watched_one() {
        let aliases = [(
            PathBuf::from("/private/var/folders/x/bench/apps/library/library"),
            PathBuf::from("/var/folders/x/bench/apps/library/library"),
        )];
        assert_eq!(
            as_watched(
                PathBuf::from("/private/var/folders/x/bench/apps/library/library/doctype/a/a.json"),
                &aliases
            ),
            PathBuf::from("/var/folders/x/bench/apps/library/library/doctype/a/a.json")
        );
        // Anything else is left as reported.
        assert_eq!(
            as_watched(PathBuf::from("/elsewhere/a.py"), &aliases),
            PathBuf::from("/elsewhere/a.py")
        );
    }

    #[test]
    fn only_files_the_index_reads_are_interesting() {
        assert!(interesting(Path::new("/bench/apps/x/x/doctype/a/a.json")));
        assert!(interesting(Path::new("/bench/apps/x/x/api.py")));
        assert!(interesting(Path::new("/bench/apps/x/x/patches.txt")));

        // Not indexed content, but they decide what is.
        assert!(interesting(Path::new("/bench/sites/apps.txt")));
        assert!(interesting(Path::new("/bench/apps/x/x/modules.txt")));

        assert!(!interesting(Path::new("/bench/apps/x/x/README.md")));
        assert!(!interesting(Path::new("/bench/sites/x/logs/web.log")));
    }

    #[test]
    fn the_files_that_decide_the_app_set_are_recognized() {
        // These force a rebuild rather than a patch: install order decides the
        // merge, so no per-file patch can express a change to them.
        assert!(decides_the_app_set(Path::new("/bench/sites/apps.txt")));
        assert!(decides_the_app_set(Path::new("/bench/sites/apps.json")));
        assert!(decides_the_app_set(Path::new(
            "/bench/apps/x/x/modules.txt"
        )));

        assert!(!decides_the_app_set(Path::new(
            "/bench/apps/x/x/patches.txt"
        )));
        // A site's own apps.txt is not the bench's.
        assert!(!decides_the_app_set(Path::new("/bench/sites/x/apps.txt")));
    }

    #[test]
    fn excluded_directories_are_ignored_wherever_they_appear() {
        // A watcher fires for everything under the root; the scan's exclusions
        // are what keep node_modules from waking the index thousands of times.
        assert!(!interesting(Path::new(
            "/bench/apps/x/x/public/node_modules/left-pad/index.json"
        )));
        assert!(!interesting(Path::new("/bench/apps/x/.git/index.json")));
        assert!(!interesting(Path::new(
            "/bench/apps/x/x/__pycache__/api.cpython-312.py"
        )));
    }
}
