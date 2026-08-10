//! The discovered workspace — spec 03 end to end.

use std::path::{Path, PathBuf};

use crate::app::{self, App};
use crate::detect::{self, Detection, Mode};
use crate::installed::Installed;
use crate::site::{self, Site};

pub use crate::problem::Problem;

/// Everything discovery knows before anything is parsed into the metadata
/// graph of [04](../../../docs/specs/04-metadata-index.md).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    /// What matched, and where.
    pub detection: Detection,
    /// Installed apps, in install order. Uninstalled apps are not here at all
    /// — spec 03 excludes them rather than flagging them.
    pub apps: Vec<App>,
    /// Apps found on disk and left out, by directory. Kept for the log: "why
    /// is my DocType missing" has exactly one common answer.
    pub excluded: Vec<PathBuf>,
    /// Sites under `sites/`.
    pub sites: Vec<Site>,
    /// The site `currentsite.txt` names.
    pub current_site: Option<String>,
    /// Files discovery could not use.
    pub problems: Vec<Problem>,
}

impl Workspace {
    /// Discover the workspace containing `start`.
    ///
    /// Never fails. A directory that is neither a bench nor an app yields an
    /// empty workspace in [`Mode::None`], because losing the session is a
    /// worse answer than serving nothing
    /// ([02](../../../docs/specs/02-architecture.md)).
    #[must_use]
    pub fn discover(start: &Path) -> Self {
        let detection = detect::detect(start);
        let mut problems = Vec::new();

        let (installed, sites, current_site) = match detection.bench_root() {
            Some(bench) => {
                let sites_directory = bench.join("sites");
                let (installed, installed_problems) = Installed::read(&sites_directory);
                problems.extend(installed_problems);
                let (sites, current) = site::discover(&sites_directory);
                (installed, sites, current)
            }
            // An app checkout with no bench has nothing to filter against.
            None => (Installed::unfiltered(), Vec::new(), None),
        };

        let candidates = match (&detection.mode, detection.bench_root()) {
            (Mode::None, _) => Vec::new(),
            (_, Some(bench)) => app::packages(&bench.join("apps")),
            // A lone app checkout: the app is its own workspace.
            (Mode::App, None) => package_of(&detection.root),
            (Mode::Bench, None) => Vec::new(),
        };

        let mut apps = Vec::new();
        let mut excluded = Vec::new();
        for (directory, package) in candidates {
            let (mut loaded, app_problems) = app::load(&directory, &package, 0);

            if !installed.contains(&loaded.name) {
                // Not a warning: an uninstalled app on disk is the normal state
                // of a bench that has renamed one.
                excluded.push(directory);
                continue;
            }

            problems.extend(app_problems);
            loaded.order = installed.order(&loaded.name);
            if let Some(meta) = installed.meta(&loaded.name) {
                loaded.version.clone_from(&meta.version);
                loaded.branch.clone_from(&meta.branch);
                loaded.commit_hash.clone_from(&meta.commit_hash);
            }
            apps.push(loaded);
        }

        apps.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.name.cmp(&b.name)));
        excluded.sort();

        Self {
            detection,
            apps,
            excluded,
            sites,
            current_site,
            problems,
        }
    }

    /// The detected Frappe version — `__version__` in the `frappe` package,
    /// falling back to what `apps.json` claims
    /// ([31](../../../docs/specs/31-performance.md)).
    ///
    /// `None` means every check gated on a vendored snapshot stays silent, so
    /// a wrong answer here is worse than no answer.
    #[must_use]
    pub fn frappe_version(&self) -> Option<&str> {
        let frappe = self.app("frappe")?;
        frappe
            .source_version
            .as_deref()
            .or(frappe.version.as_deref())
    }

    /// An app by name.
    #[must_use]
    pub fn app(&self, name: &str) -> Option<&App> {
        self.apps.iter().find(|app| app.name == name)
    }

    /// Every DocType record across every installed app, in app order.
    pub fn doctypes(&self) -> impl Iterator<Item = (&App, &crate::artifacts::Record)> {
        self.apps
            .iter()
            .flat_map(|app| app.artifacts.doctypes().map(move |record| (app, record)))
    }

    /// DocType names defined by more than one installed app — the conflict of
    /// [04](../../../docs/specs/04-metadata-index.md), which the later app wins.
    #[must_use]
    pub fn duplicate_doctypes(&self) -> Vec<(String, Vec<PathBuf>)> {
        let mut by_name: std::collections::BTreeMap<&str, Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        for (_, record) in self.doctypes() {
            by_name
                .entry(record.name.as_str())
                .or_default()
                .push(record.path.clone());
        }

        by_name
            .into_iter()
            .filter(|(_, paths)| paths.len() > 1)
            .map(|(name, paths)| (name.to_string(), paths))
            .collect()
    }
}

/// The package inside a lone app checkout.
fn package_of(directory: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|package| package.join("hooks.py").is_file())
        .map(|package| (directory.to_path_buf(), package))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bench() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench")
    }

    #[test]
    fn the_fixture_bench_discovers_its_three_installed_apps() {
        let workspace = Workspace::discover(&bench());

        assert_eq!(workspace.detection.mode, Mode::Bench);
        let names: Vec<&str> = workspace.apps.iter().map(|app| app.name.as_str()).collect();
        assert_eq!(names, ["frappe", "library", "circulation_app"]);
    }

    #[test]
    fn the_uninstalled_app_is_excluded_and_raises_no_conflict() {
        let workspace = Workspace::discover(&bench());

        assert_eq!(workspace.excluded.len(), 1);
        assert!(workspace.excluded[0].ends_with("library-old"));

        // Library Availability is defined twice on purpose, by two *installed*
        // apps. Library Member and Library Settings are defined twice on disk
        // and must not appear here — that is the whole rule.
        let duplicates: Vec<String> = workspace
            .duplicate_doctypes()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(duplicates, ["Library Availability"]);
    }

    #[test]
    fn install_order_comes_from_apps_json() {
        let workspace = Workspace::discover(&bench());
        let orders: Vec<usize> = workspace.apps.iter().map(|app| app.order).collect();
        assert_eq!(orders, [0, 1, 2]);

        let library = workspace.app("library").expect("library");
        assert_eq!(library.version.as_deref(), Some("0.1.0"));
        assert_eq!(library.branch.as_deref(), Some("main"));
    }

    #[test]
    fn the_frappe_version_is_detected() {
        let workspace = Workspace::discover(&bench());
        assert_eq!(workspace.frappe_version(), Some("17.0.0-dev"));
    }

    #[test]
    fn doctype_counts_match_the_fixture_readme() {
        let workspace = Workspace::discover(&bench());

        assert_eq!(
            workspace
                .app("library")
                .unwrap()
                .artifacts
                .doctypes()
                .count(),
            5
        );
        assert_eq!(
            workspace
                .app("circulation_app")
                .unwrap()
                .artifacts
                .doctypes()
                .count(),
            4
        );
        assert_eq!(workspace.doctypes().count(), 9);
    }

    #[test]
    fn the_malformed_doctype_is_the_only_problem() {
        let workspace = Workspace::discover(&bench());

        assert_eq!(workspace.problems.len(), 1, "{:?}", workspace.problems);
        assert!(
            workspace.problems[0]
                .path
                .ends_with("circulation_malformed.json")
        );
    }

    #[test]
    fn sites_are_read_without_walking_them() {
        let workspace = Workspace::discover(&bench());
        assert_eq!(workspace.current_site.as_deref(), Some("library.localhost"));
        assert_eq!(workspace.sites.len(), 1);
    }

    #[test]
    fn an_app_opened_inside_a_bench_still_sees_its_siblings() {
        let workspace = Workspace::discover(&bench().join("apps/circulation-app"));

        assert_eq!(workspace.detection.mode, Mode::App);
        assert_eq!(workspace.apps.len(), 3);
        assert!(workspace.app("library").is_some());
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
    fn an_app_checkout_with_no_bench_indexes_itself() {
        // The other half of App mode: no `sites/`, so no installed list to
        // filter against and no siblings to find.
        let temporary =
            std::env::temp_dir().join(format!("frappe-lsp-lone-app-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temporary);
        copy_tree(&bench().join("apps/library"), &temporary.join("library"));

        let workspace = Workspace::discover(&temporary.join("library"));

        assert_eq!(workspace.detection.mode, Mode::App);
        assert_eq!(workspace.detection.bench, None);
        assert!(workspace.sites.is_empty());
        assert_eq!(workspace.apps.len(), 1);

        let app = &workspace.apps[0];
        assert_eq!(app.name, "library");
        assert_eq!(app.artifacts.doctypes().count(), 5);
        // No apps.json, so no install metadata — and nothing excluded for want
        // of a list that does not exist.
        assert_eq!(app.version, None);
        assert!(workspace.excluded.is_empty());

        std::fs::remove_dir_all(&temporary).expect("cleanup");
    }

    #[test]
    fn a_directory_that_is_neither_yields_an_empty_workspace() {
        let workspace = Workspace::discover(Path::new(env!("CARGO_MANIFEST_DIR")));

        assert_eq!(workspace.detection.mode, Mode::None);
        assert!(workspace.apps.is_empty());
        assert!(workspace.problems.is_empty());
        assert_eq!(workspace.frappe_version(), None);
    }
}
