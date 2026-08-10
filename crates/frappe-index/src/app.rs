//! App enumeration — spec 03, "App enumeration".

use std::path::{Path, PathBuf};

use crate::artifacts::{self, Artifacts};
use crate::problem::Problem;
use crate::scrub::scrub;

/// One app on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// The `app_name` assignment in `hooks.py` — not the directory name, and
    /// not the package name. All three differ on real benches.
    pub name: String,
    /// `<bench>/apps/<directory>`.
    pub directory: PathBuf,
    /// The Python package inside it, the one holding `hooks.py`.
    pub package: PathBuf,
    /// Install order from `apps.json` or `apps.txt`. Breaks ties in the layer
    /// merge of [04](../../../docs/specs/04-metadata-index.md).
    pub order: usize,
    /// `__version__` from the package's `__init__.py`.
    pub source_version: Option<String>,
    /// `version` from `sites/apps.json`.
    pub version: Option<String>,
    /// `resolution.branch` from `sites/apps.json`.
    pub branch: Option<String>,
    /// `resolution.commit_hash` from `sites/apps.json`.
    pub commit_hash: Option<String>,
    /// From `<package>/modules.txt`.
    pub modules: Vec<Module>,
    /// Everything the package contains.
    pub artifacts: Artifacts,
}

/// A module of an app. Its directory is its scrubbed name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// As written in `modules.txt`.
    pub name: String,
    /// `<package>/<scrubbed name>`, when it exists. A module may be declared
    /// without a directory; nothing depends on the directory existing.
    pub directory: Option<PathBuf>,
}

/// Every `apps/*/*/hooks.py`: the app directory and the package inside it.
///
/// The directory under `apps/` is not the app name and the package is not named
/// after it, so the package is found by looking for `hooks.py` rather than by
/// assuming any of the three agree.
#[must_use]
pub fn packages(apps: &Path) -> Vec<(PathBuf, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(apps) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    for app in entries.flatten() {
        let directory = app.path();
        if !directory.is_dir() {
            continue;
        }
        let Ok(children) = std::fs::read_dir(&directory) else {
            continue;
        };
        for child in children.flatten() {
            let package = child.path();
            if package.join("hooks.py").is_file() {
                found.push((directory.clone(), package));
            }
        }
    }

    found.sort();
    found
}

/// Read one app. `order` is its install position.
#[must_use]
pub fn load(directory: &Path, package: &Path, order: usize) -> (App, Vec<Problem>) {
    let mut problems = Vec::new();

    let hooks = package.join("hooks.py");
    let name = match std::fs::read_to_string(&hooks) {
        Ok(source) => python_string(&source, "app_name").unwrap_or_else(|| {
            problems.push(Problem::new(&hooks, "no app_name assignment"));
            fallback_name(package)
        }),
        Err(error) => {
            problems.push(Problem::new(&hooks, format!("unreadable: {error}")));
            fallback_name(package)
        }
    };

    let source_version = std::fs::read_to_string(package.join("__init__.py"))
        .ok()
        .and_then(|source| python_string(&source, "__version__"));

    let (modules, module_problems) = modules(package);
    problems.extend(module_problems);

    let (artifacts, artifact_problems) = artifacts::scan(package);
    problems.extend(artifact_problems);

    let app = App {
        name,
        directory: directory.to_path_buf(),
        package: package.to_path_buf(),
        order,
        source_version,
        version: None,
        branch: None,
        commit_hash: None,
        modules,
        artifacts,
    };

    (app, problems)
}

/// The package directory name, used only when `hooks.py` will not give a name.
fn fallback_name(package: &Path) -> String {
    package
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

fn modules(package: &Path) -> (Vec<Module>, Vec<Problem>) {
    let path = package.join("modules.txt");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // An app without modules.txt declares no modules; that is legal.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), Vec::new());
        }
        Err(error) => {
            return (
                Vec::new(),
                vec![Problem::new(&path, format!("unreadable: {error}"))],
            );
        }
    };

    let modules = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|name| {
            let directory = package.join(scrub(name));
            Module {
                name: name.to_string(),
                directory: directory.is_dir().then_some(directory),
            }
        })
        .collect();

    (modules, Vec::new())
}

/// A module-level `name = "value"` assignment.
///
/// Deliberately not a Python parse. Discovery reads two scalars — `app_name`
/// and `__version__` — from every candidate directory while deciding what is
/// even an app, and parsing each one to answer that costs more than the whole
/// of discovery. Symbols that carry meaning are read with tree-sitter, in
/// [`frappe_syntax::symbols`]; hook *values* follow in 010.
#[must_use]
pub fn python_string(source: &str, name: &str) -> Option<String> {
    for line in source.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some(rest) = line.strip_prefix(name) else {
            continue;
        };
        // `app_name_suffix = ...` must not match `app_name`.
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };

        let rest = rest.trim_start();
        let quote = rest.chars().next()?;
        if quote != '"' && quote != '\'' {
            continue;
        }
        let value = &rest[quote.len_utf8()..];
        let end = value.find(quote)?;
        return Some(value[..end].to_string());
    }
    None
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
    fn packages_are_found_by_hooks_not_by_name() {
        let found = packages(&bench().join("apps"));
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .filter_map(|(directory, package)| {
                Some((
                    directory.file_name()?.to_str()?,
                    package.file_name()?.to_str()?,
                ))
            })
            .collect();

        assert_eq!(
            pairs,
            [
                ("circulation-app", "circulation"),
                ("frappe", "frappe"),
                ("library", "library"),
                ("library-old", "library_archive"),
            ]
        );
    }

    #[test]
    fn the_app_name_comes_from_hooks_py() {
        let apps = bench().join("apps");
        let (app, problems) = load(
            &apps.join("circulation-app"),
            &apps.join("circulation-app/circulation"),
            0,
        );

        assert!(problems.iter().all(|p| p.message.starts_with("malformed")));
        // Directory, package and name all differ — the case spec 03 exists for.
        assert_eq!(app.name, "circulation_app");
        assert_eq!(app.package.file_name().unwrap(), "circulation");
        assert_eq!(app.directory.file_name().unwrap(), "circulation-app");
    }

    #[test]
    fn modules_resolve_to_scrubbed_directories() {
        let apps = bench().join("apps");
        let (app, _) = load(&apps.join("library"), &apps.join("library/library"), 0);

        assert_eq!(app.modules.len(), 1);
        assert_eq!(app.modules[0].name, "Library");
        assert_eq!(
            app.modules[0].directory.as_deref(),
            Some(apps.join("library/library/library").as_path())
        );
    }

    #[test]
    fn a_module_without_a_directory_is_still_declared() {
        let apps = bench().join("apps");
        let (app, _) = load(
            &apps.join("library-old"),
            &apps.join("library-old/library_archive"),
            0,
        );

        assert_eq!(app.name, "library_archive");
        assert_eq!(app.modules[0].name, "Library Archive");
        // modules.txt says `Library Archive`; the directory is `library_archive`.
        assert!(app.modules[0].directory.is_some());
    }

    #[test]
    fn frappes_version_comes_from_the_package() {
        let apps = bench().join("apps");
        let (app, _) = load(&apps.join("frappe"), &apps.join("frappe/frappe"), 0);
        assert_eq!(app.source_version.as_deref(), Some("17.0.0-dev"));
    }

    #[test]
    fn python_string_reads_only_real_assignments() {
        let source = "\
# app_name = \"commented\"
app_name_suffix = \"suffix\"
app_name = 'quoted'
app_title = \"Library\"
";
        assert_eq!(python_string(source, "app_name").as_deref(), Some("quoted"));
        assert_eq!(
            python_string(source, "app_title").as_deref(),
            Some("Library")
        );
        assert_eq!(python_string(source, "app_email"), None);
    }
}
