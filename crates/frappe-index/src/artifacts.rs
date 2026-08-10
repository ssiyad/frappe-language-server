//! What an app package contains — spec 03, "Other artifacts".

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::problem::Problem;
use crate::scrub::scrub;
use crate::walk::Walk;

/// A record exported to a file: a DocType, Report, Page, Web Form and so on.
///
/// One rule finds them all. Frappe writes every one of these to
/// `<kind>/<name>/<name>.json`, where `<kind>` is the scrubbed name of the
/// DocType the record belongs to — so `scrub(record.doctype) == <kind>` is the
/// test, and the whitelist spec 03 lists is what it happens to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The record's own DocType: `DocType`, `Report`, `Web Form`, …
    pub kind: String,
    /// The JSON `name`. For a DocType this is the DocType name.
    pub name: String,
    /// The JSON `module`, never the path — apps do nest records below their
    /// module directory.
    pub module: Option<String>,
    /// The file itself.
    pub path: PathBuf,
}

impl Record {
    /// Whether this record defines a DocType.
    #[must_use]
    pub fn is_doctype(&self) -> bool {
        self.kind == "DocType"
    }

    /// The directory holding the record and its sibling files (`<x>.py`,
    /// `<x>.js`, `test_<x>.py`).
    #[must_use]
    pub fn directory(&self) -> &Path {
        self.path.parent().unwrap_or(&self.path)
    }
}

/// Why a JSON file in a record position was not taken as a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// Rule 1: the basename stem is not its parent directory name.
    StemMismatch,
    /// Rule 2: not a JSON object, or not parseable at all.
    NotAnObject,
    /// Rule 3: `doctype` does not match the directory it sits in.
    WrongKind,
}

/// A file the record rules turned away, with the kind directory it sat in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rejected {
    /// The file.
    pub path: PathBuf,
    /// The directory two levels up: `doctype`, `report`, `fixtures`, …
    pub kind_directory: String,
    /// Which rule turned it away.
    pub rejection: Rejection,
}

/// Everything discovery found in one app package.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Artifacts {
    /// `<package>/hooks.py`.
    pub hooks: Option<PathBuf>,
    /// `<package>/patches.txt`.
    pub patches: Option<PathBuf>,
    /// `<package>/fixtures/*.json` — Layer 2 of
    /// [04](../../../docs/specs/04-metadata-index.md).
    pub fixtures: Vec<PathBuf>,
    /// DocTypes, Reports, Pages, Web Forms, Print Formats, Workspaces, …
    pub records: Vec<Record>,
    /// `<package>/**/templates/**` and `<package>/www/**`.
    pub templates: Vec<PathBuf>,
    /// `<package>/public/**` — the targets of `app_include_js` and friends.
    pub public_assets: Vec<PathBuf>,
    /// JSON in a record position that no rule accepted.
    pub rejected: Vec<Rejected>,
}

impl Artifacts {
    /// The DocTypes of this app.
    pub fn doctypes(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().filter(|record| record.is_doctype())
    }

    /// Records of one kind, named as the record's own DocType (`Report`).
    pub fn of_kind<'a>(&'a self, kind: &'a str) -> impl Iterator<Item = &'a Record> {
        self.records
            .iter()
            .filter(move |record| record.kind == kind)
    }

    /// Files that sat in `<kind>/<x>/` and were turned away.
    pub fn rejected_in(&self, kind_directory: &str) -> impl Iterator<Item = &Rejected> {
        self.rejected
            .iter()
            .filter(move |rejected| rejected.kind_directory == kind_directory)
    }
}

/// Scan one app package.
///
/// Every file is visited once and routed by where it sits; a second pass per
/// artifact kind would multiply the only expensive thing discovery does.
pub fn scan(package: &Path) -> (Artifacts, Vec<Problem>) {
    let mut artifacts = Artifacts::default();
    let mut problems = Vec::new();

    let hooks = package.join("hooks.py");
    if hooks.is_file() {
        artifacts.hooks = Some(hooks);
    }
    let patches = package.join("patches.txt");
    if patches.is_file() {
        artifacts.patches = Some(patches);
    }

    for path in Walk::new(package) {
        let relative = path.strip_prefix(package).unwrap_or(&path);

        if is_under(relative, "fixtures") && has_extension(&path, "json") {
            artifacts.fixtures.push(path);
            continue;
        }
        if is_under(relative, "public") {
            artifacts.public_assets.push(path);
            continue;
        }
        if is_under(relative, "www") || has_ancestor_named(relative, "templates") {
            artifacts.templates.push(path);
            continue;
        }
        if !has_extension(&path, "json") {
            continue;
        }

        match classify(&path) {
            Ok(record) => artifacts.records.push(record),
            Err((rejection, problem)) => {
                if let Some(kind_directory) = kind_directory(&path) {
                    artifacts.rejected.push(Rejected {
                        path: path.clone(),
                        kind_directory,
                        rejection,
                    });
                }
                problems.extend(problem);
            }
        }
    }

    artifacts.fixtures.sort();
    artifacts.templates.sort();
    artifacts.public_assets.sort();
    artifacts.records.sort_by(|a, b| a.path.cmp(&b.path));
    artifacts.rejected.sort_by(|a, b| a.path.cmp(&b.path));

    (artifacts, problems)
}

/// The three-part filter of spec 03, generalized past `doctype`.
///
/// The incremental re-index of 005 applies the same filter to one file, so a
/// JSON that appears beside a DocType does not become one.
pub(crate) fn classify(path: &Path) -> Result<Record, (Rejection, Option<Problem>)> {
    let Some(kind_directory) = kind_directory(path) else {
        return Err((Rejection::StemMismatch, None));
    };

    // 1. Stem equals the parent directory name.
    if path.file_stem() != path.parent().and_then(Path::file_name) {
        return Err((Rejection::StemMismatch, None));
    }

    // 2. Parses as a JSON object.
    let Ok(text) = std::fs::read_to_string(path) else {
        return Err((
            Rejection::NotAnObject,
            Some(Problem::new(path, "unreadable")),
        ));
    };
    let value: Value = match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(error) => {
            // Only worth reporting where a record was expected: a malformed
            // DocType is a diagnostic, a malformed anything-else is noise.
            let problem = (kind_directory == "doctype")
                .then(|| Problem::new(path, format!("malformed JSON: {error}")));
            return Err((Rejection::NotAnObject, problem));
        }
    };
    let Some(object) = value.as_object() else {
        return Err((Rejection::NotAnObject, None));
    };

    // 3. The record's own DocType matches the directory it sits in.
    let kind = object.get("doctype").and_then(Value::as_str).unwrap_or("");
    if kind.is_empty() || scrub(kind) != kind_directory {
        return Err((Rejection::WrongKind, None));
    }

    Ok(Record {
        kind: kind.to_string(),
        name: object
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        module: object
            .get("module")
            .and_then(Value::as_str)
            .map(str::to_string),
        path: path.to_path_buf(),
    })
}

/// The directory two levels up — the one naming the record kind.
fn kind_directory(path: &Path) -> Option<String> {
    Some(
        path.parent()?
            .parent()?
            .file_name()?
            .to_str()?
            .to_ascii_lowercase(),
    )
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension().is_some_and(|found| found == extension)
}

fn is_under(relative: &Path, directory: &str) -> bool {
    relative
        .components()
        .next()
        .is_some_and(|first| first.as_os_str() == directory)
}

fn has_ancestor_named(relative: &Path, directory: &str) -> bool {
    relative
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .any(|component| component.as_os_str() == directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(2)
            .expect("repo root")
            .join("tests/fixtures/bench/apps/library/library")
    }

    #[test]
    fn the_library_app_scans_to_its_readme() {
        let (artifacts, problems) = scan(&library());
        assert!(problems.is_empty(), "{problems:?}");

        let mut names: Vec<&str> = artifacts
            .doctypes()
            .map(|record| record.name.as_str())
            .collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "Library Availability",
                "Library Loan Item",
                "Library Member",
                "Library Membership",
                "Library Settings",
            ]
        );

        assert!(artifacts.hooks.is_some());
        assert!(artifacts.patches.is_some());
        assert_eq!(artifacts.fixtures.len(), 2);
        assert_eq!(artifacts.of_kind("Report").count(), 0);
    }

    #[test]
    fn the_rejected_files_are_the_two_the_readme_names() {
        let (artifacts, _) = scan(&library());
        let rejected: Vec<_> = artifacts.rejected_in("doctype").collect();

        assert_eq!(rejected.len(), 2);
        assert!(
            rejected
                .iter()
                .all(|file| file.rejection == Rejection::StemMismatch)
        );
        let mut names: Vec<_> = rejected
            .iter()
            .filter_map(|file| file.path.file_name()?.to_str())
            .collect();
        names.sort_unstable();
        assert_eq!(names, ["member_activity_report.json", "test_records.json"]);
    }

    #[test]
    fn node_modules_and_built_assets_never_appear() {
        let (artifacts, _) = scan(&library());
        for path in artifacts
            .public_assets
            .iter()
            .chain(&artifacts.templates)
            .chain(&artifacts.fixtures)
        {
            assert!(
                !path.to_string_lossy().contains("node_modules"),
                "{}",
                path.display()
            );
        }
    }

    #[test]
    fn malformed_doctype_json_is_a_problem_not_a_panic() {
        let circulation = library()
            .parent()
            .and_then(Path::parent)
            .expect("apps directory")
            .join("circulation-app/circulation");

        let (artifacts, problems) = scan(&circulation);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].message.starts_with("malformed JSON"));
        assert!(problems[0].path.ends_with("circulation_malformed.json"));

        // The rest of the app still indexed.
        assert_eq!(artifacts.doctypes().count(), 4);
    }
}
