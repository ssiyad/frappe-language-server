//! Which apps are installed — spec 03, "Installed apps are authoritative".
//!
//! An app on disk that is not installed is excluded from the index entirely.
//! On the reference bench, `apps/resource-management/` is a stale rename of
//! `apps/resource_management/` and the two define 41 of 41 identical DocType
//! names; indexing both would emit 41 conflicts before the user typed anything.

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value;

use crate::problem::Problem;

/// Where the installed list came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// `sites/apps.json` — carries `idx`, `version` and the git resolution.
    AppsJson,
    /// `sites/apps.txt` — names in install order, nothing else.
    AppsTxt,
    /// Neither file exists. Nothing is filtered: an app checkout opened on its
    /// own has no bench to ask.
    Unfiltered,
}

/// Per-app metadata from `apps.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    /// Declared version.
    pub version: Option<String>,
    /// `resolution.branch`, absent when the app is not a repo.
    pub branch: Option<String>,
    /// `resolution.commit_hash`.
    pub commit_hash: Option<String>,
}

/// The installed apps, in install order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    order: Vec<String>,
    meta: HashMap<String, Meta>,
    source: Source,
}

impl Installed {
    /// Read `sites/apps.json`, falling back to `sites/apps.txt`.
    #[must_use]
    pub fn read(sites: &Path) -> (Self, Vec<Problem>) {
        let mut problems = Vec::new();

        let json = sites.join("apps.json");
        if json.is_file() {
            match read_apps_json(&json) {
                Ok(installed) => return (installed, problems),
                Err(problem) => problems.push(problem),
            }
        }

        let txt = sites.join("apps.txt");
        if let Ok(text) = std::fs::read_to_string(&txt) {
            // A bench may write this file without a trailing newline; `lines`
            // tolerates that, which is the whole requirement.
            let order: Vec<String> = text
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
            return (
                Self {
                    order,
                    meta: HashMap::new(),
                    source: Source::AppsTxt,
                },
                problems,
            );
        }

        (
            Self {
                order: Vec::new(),
                meta: HashMap::new(),
                source: Source::Unfiltered,
            },
            problems,
        )
    }

    /// Nothing to filter against: every app found on disk counts as installed.
    #[must_use]
    pub fn unfiltered() -> Self {
        Self {
            order: Vec::new(),
            meta: HashMap::new(),
            source: Source::Unfiltered,
        }
    }

    /// Where the list came from.
    #[must_use]
    pub fn source(&self) -> Source {
        self.source
    }

    /// Whether this app is installed.
    #[must_use]
    pub fn contains(&self, app: &str) -> bool {
        self.source == Source::Unfiltered || self.order.iter().any(|name| name == app)
    }

    /// Install position; apps absent from the list sort last, stably.
    #[must_use]
    pub fn order(&self, app: &str) -> usize {
        self.order
            .iter()
            .position(|name| name == app)
            .unwrap_or(usize::MAX)
    }

    /// Version, branch and commit for an app.
    #[must_use]
    pub fn meta(&self, app: &str) -> Option<&Meta> {
        self.meta.get(app)
    }

    /// The installed names, in order.
    #[must_use]
    pub fn names(&self) -> &[String] {
        &self.order
    }
}

fn read_apps_json(path: &Path) -> Result<Installed, Problem> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| Problem::new(path, format!("unreadable: {error}")))?;
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| Problem::new(path, format!("malformed JSON: {error}")))?;
    let object = value
        .as_object()
        .ok_or_else(|| Problem::new(path, "expected an object keyed by app name"))?;

    let mut apps: Vec<(u64, String, Meta)> = Vec::new();
    for (name, entry) in object {
        // `resolution` is an object for a git checkout and the string "not a
        // repo" otherwise; both are normal.
        let resolution = entry.get("resolution").and_then(Value::as_object);
        apps.push((
            entry.get("idx").and_then(Value::as_u64).unwrap_or(u64::MAX),
            name.clone(),
            Meta {
                version: string(entry.get("version")),
                branch: resolution.and_then(|r| string(r.get("branch"))),
                commit_hash: resolution.and_then(|r| string(r.get("commit_hash"))),
            },
        ));
    }
    apps.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    Ok(Installed {
        order: apps.iter().map(|(_, name, _)| name.clone()).collect(),
        meta: apps
            .into_iter()
            .map(|(_, name, meta)| (name, meta))
            .collect(),
        source: Source::AppsJson,
    })
}

fn string(value: Option<&Value>) -> Option<String> {
    Some(value?.as_str()?.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sites() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench/sites")
    }

    #[test]
    fn apps_json_wins_and_carries_install_order() {
        let (installed, problems) = Installed::read(&sites());
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(installed.source(), Source::AppsJson);
        assert_eq!(installed.names(), ["frappe", "library", "circulation_app"]);

        let meta = installed.meta("library").expect("library metadata");
        assert_eq!(meta.version.as_deref(), Some("0.1.0"));
        assert_eq!(meta.branch.as_deref(), Some("main"));
    }

    #[test]
    fn an_app_that_is_not_a_repo_has_no_branch() {
        let (installed, _) = Installed::read(&sites());
        let meta = installed.meta("circulation_app").expect("metadata");
        assert_eq!(meta.branch, None);
        assert_eq!(meta.version.as_deref(), Some("0.0.1"));
    }

    #[test]
    fn the_uninstalled_app_is_absent() {
        let (installed, _) = Installed::read(&sites());
        assert!(installed.contains("library"));
        assert!(!installed.contains("library_archive"));
    }

    #[test]
    fn apps_txt_is_the_fallback_and_tolerates_a_missing_newline() {
        let directory =
            std::env::temp_dir().join(format!("frappe-lsp-apps-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temp dir");
        std::fs::write(directory.join("apps.txt"), "frappe\nlibrary").expect("write");

        let (installed, problems) = Installed::read(&directory);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(installed.source(), Source::AppsTxt);
        assert_eq!(installed.names(), ["frappe", "library"]);
        assert_eq!(installed.order("library"), 1);

        std::fs::remove_dir_all(&directory).expect("cleanup");
    }

    #[test]
    fn without_either_file_nothing_is_filtered() {
        let (installed, problems) = Installed::read(Path::new("/nonexistent/sites"));
        assert!(problems.is_empty());
        assert_eq!(installed.source(), Source::Unfiltered);
        assert!(installed.contains("anything"));
    }
}
