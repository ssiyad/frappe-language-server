//! Bench / App / None detection by upward walk — spec 03.

use std::path::{Path, PathBuf};

/// What the opened directory turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    /// A bench: `apps/` and `sites/`. Indexes every installed app.
    Bench,
    /// A single app checkout. Indexes this app, plus its siblings when it sits
    /// inside a bench.
    App,
    /// Neither. The server serves nothing Frappe-specific and does not error
    /// the session ([02](../../../docs/specs/02-architecture.md)).
    None,
}

/// The outcome of the upward walk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Detection {
    /// What matched.
    pub mode: Mode,
    /// The directory that matched: the bench root, or the app checkout.
    pub root: PathBuf,
    /// The bench an `App` sits in, when it does. Siblings come from here.
    pub bench: Option<PathBuf>,
}

impl Detection {
    /// The bench root, whichever mode found it.
    #[must_use]
    pub fn bench_root(&self) -> Option<&Path> {
        match self.mode {
            Mode::Bench => Some(&self.root),
            Mode::App => self.bench.as_deref(),
            Mode::None => None,
        }
    }
}

/// Walk up from `start` until a directory matches; first match wins.
///
/// The walk stops at the filesystem root, or at a `.git` boundary that is
/// neither a bench nor an app — an app checkout inside an unrelated repository
/// must not drag the whole repository in.
#[must_use]
pub fn detect(start: &Path) -> Detection {
    for directory in start.ancestors() {
        if is_bench(directory) {
            return Detection {
                mode: Mode::Bench,
                root: directory.to_path_buf(),
                bench: None,
            };
        }

        if is_app(directory) {
            return Detection {
                mode: Mode::App,
                root: directory.to_path_buf(),
                bench: enclosing_bench(directory),
            };
        }

        if directory.join(".git").exists() {
            break;
        }
    }

    Detection {
        mode: Mode::None,
        root: start.to_path_buf(),
        bench: None,
    }
}

/// `apps/` and `sites/` side by side.
#[must_use]
pub fn is_bench(directory: &Path) -> bool {
    directory.join("apps").is_dir() && directory.join("sites").is_dir()
}

/// A packaged app: `<name>/hooks.py` beside `pyproject.toml` or `setup.py`.
#[must_use]
pub fn is_app(directory: &Path) -> bool {
    if !directory.join("pyproject.toml").is_file() && !directory.join("setup.py").is_file() {
        return false;
    }

    let Ok(entries) = std::fs::read_dir(directory) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.path().join("hooks.py").is_file())
}

/// `<bench>/apps/<app>` — the only layout in which siblings are reachable.
fn enclosing_bench(app: &Path) -> Option<PathBuf> {
    let apps = app.parent()?;
    if apps.file_name()? != "apps" {
        return None;
    }
    let bench = apps.parent()?;
    is_bench(bench).then(|| bench.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_bench() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench")
    }

    #[test]
    fn a_bench_root_is_a_bench() {
        let detection = detect(&fixture_bench());
        assert_eq!(detection.mode, Mode::Bench);
        assert_eq!(detection.root, fixture_bench());
    }

    #[test]
    fn a_directory_inside_a_bench_walks_up_to_it() {
        let deep = fixture_bench().join("apps/library/library/library/doctype/library_member");
        let detection = detect(&deep);

        // The app matches first only if the walk stops there; it must not,
        // because `library/` has no pyproject.toml — that sits one level up.
        assert_eq!(detection.mode, Mode::App);
        assert_eq!(detection.root, fixture_bench().join("apps/library"));
        assert_eq!(detection.bench.as_deref(), Some(fixture_bench().as_path()));
        assert_eq!(detection.bench_root(), Some(fixture_bench().as_path()));
    }

    #[test]
    fn an_app_checkout_reports_its_bench() {
        let detection = detect(&fixture_bench().join("apps/circulation-app"));
        assert_eq!(detection.mode, Mode::App);
        assert_eq!(detection.bench.as_deref(), Some(fixture_bench().as_path()));
    }

    #[test]
    fn a_directory_that_is_neither_yields_none() {
        let detection = detect(Path::new(env!("CARGO_MANIFEST_DIR")));
        assert_eq!(detection.mode, Mode::None);
        assert_eq!(detection.bench_root(), None);
    }
}
