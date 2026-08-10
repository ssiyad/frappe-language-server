//! Guards on the fixture bench itself — `tests/fixtures/README.md`.
//!
//! The corpus is only useful while it stays the shape it claims to be: real
//! Frappe output, annotated with codes the spec actually defines. Nothing here
//! tests `frappe-index`; these are the assertions the rest of the suite
//! assumes without checking.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use serde_json::ser::{PrettyFormatter, Serializer};

/// Files whose contents are deliberately not valid — excluded from the shape
/// checks, asserted separately.
const MALFORMED: &str = "circulation_malformed.json";

fn repo_root() -> PathBuf {
    // crates/frappe-index -> repo root
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the repo root")
        .to_path_buf()
}

fn bench() -> PathBuf {
    repo_root().join("tests/fixtures/bench")
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(&directory).expect("readable directory") {
            let path = entry.expect("readable entry").path();
            if path.is_dir() {
                stack.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// Candidates per spec 03: `**/doctype/<x>/<x>.json`.
fn doctype_candidates() -> Vec<PathBuf> {
    walk(&bench())
        .into_iter()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
                && path
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .is_some_and(|name| name == "doctype")
        })
        .collect()
}

fn stem_matches_directory(path: &Path) -> bool {
    path.file_stem() == path.parent().and_then(Path::file_name)
}

/// `json.dumps(obj, indent=1, sort_keys=True, ensure_ascii=True) + "\n"`.
///
/// Key order comes free: without serde_json's `preserve_order` feature a JSON
/// object is a `BTreeMap`, which is exactly Python's `sort_keys=True`.
fn frappe_json(value: &Value) -> String {
    let mut output = Vec::new();
    let mut serializer =
        Serializer::with_formatter(&mut output, PrettyFormatter::with_indent(b" "));
    serde::Serialize::serialize(value, &mut serializer).expect("value re-serializes");
    let mut text = String::from_utf8(output).expect("serializer emits UTF-8");
    text.push('\n');
    text
}

#[test]
fn every_doctype_json_is_byte_shaped_like_frappe_output() {
    let mut checked = 0;
    for path in doctype_candidates() {
        if path.file_name().is_some_and(|name| name == MALFORMED) {
            continue;
        }

        let text = fs::read_to_string(&path).expect("readable JSON");
        assert!(
            text.is_ascii(),
            "{}: frappe exports with ensure_ascii=True",
            path.display()
        );

        let value: Value = serde_json::from_str(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(
            text,
            frappe_json(&value),
            "{}: not shaped like frappe's exporter — see tests/fixtures/README.md",
            path.display()
        );
        checked += 1;
    }

    // A filter that silently matched nothing would make this test vacuous.
    assert!(checked >= 10, "only {checked} JSON files checked");
}

#[test]
fn discovery_rules_have_something_to_reject() {
    let candidates = doctype_candidates();
    let (accepted, rejected): (Vec<_>, Vec<_>) = candidates
        .iter()
        .partition(|path| stem_matches_directory(path));

    assert!(
        rejected.len() >= 2,
        "the bench must carry files a stem check has to reject"
    );
    for path in &rejected {
        let text = fs::read_to_string(path).expect("readable JSON");
        // Rejected on the stem rule alone: these parse fine.
        serde_json::from_str::<Value>(&text).expect("valid JSON");
    }

    for path in &accepted {
        let text = fs::read_to_string(path).expect("readable JSON");
        if path.file_name().is_some_and(|name| name == MALFORMED) {
            assert!(
                serde_json::from_str::<Value>(&text).is_err(),
                "{}: must not parse",
                path.display()
            );
            continue;
        }
        let value: Value = serde_json::from_str(&text).expect("valid JSON");
        assert_eq!(
            value.get("doctype").and_then(Value::as_str),
            Some("DocType"),
            "{}: accepted by the stem rule, so it must be a DocType record",
            path.display()
        );
    }
}

#[test]
fn apps_txt_has_no_trailing_newline() {
    let text = fs::read_to_string(bench().join("sites/apps.txt")).expect("apps.txt");
    assert!(
        !text.ends_with('\n'),
        "the tolerance case in spec 03 stops being tested the moment this file gains one"
    );

    let installed: Vec<&str> = text.lines().collect();
    assert_eq!(installed, ["frappe", "library", "circulation_app"]);
    assert!(
        !installed.contains(&"library_archive"),
        "library-old must stay uninstalled"
    );
}

#[test]
fn the_uninstalled_app_really_does_collide() {
    let names = |app: &str| -> BTreeSet<String> {
        walk(&bench().join("apps").join(app))
            .into_iter()
            .filter(|path| {
                stem_matches_directory(path) && path.extension().is_some_and(|e| e == "json")
            })
            .filter_map(|path| {
                let value: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
                Some(value.get("name")?.as_str()?.to_string())
            })
            .collect()
    };

    let collisions: Vec<_> = names("library-old")
        .intersection(&names("library"))
        .cloned()
        .collect();
    assert_eq!(
        collisions,
        ["Library Member", "Library Settings"],
        "the exclusion rule needs a conflict to prevent"
    );
}

/// The diagnostic codes of spec 23, read out of the Code column.
fn registry(spec: &str) -> BTreeSet<&str> {
    let codes: BTreeSet<&str> = spec
        .lines()
        .filter(|line| line.starts_with('|'))
        // Table rows are `| check | `code` | severity |`.
        .filter_map(|line| line.split('|').nth(2))
        .map(str::trim)
        .filter_map(|cell| cell.strip_prefix('`')?.strip_suffix('`'))
        .filter(|code| !code.contains(' '))
        .collect();
    assert!(codes.len() >= 25, "parsed only {} codes", codes.len());
    codes
}

/// Every `expect:` annotation names a code the registry defines.
#[test]
fn annotations_name_codes_that_exist() {
    let spec = fs::read_to_string(repo_root().join("docs/specs/23-diagnostics.md"))
        .expect("spec 23 is readable");
    let codes = registry(&spec);

    let mut annotated = 0;
    for path in walk(&bench()) {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            let Some((_, rest)) = line.split_once("expect:") else {
                continue;
            };
            // `expect: clean` may carry prose after a comma explaining why, and
            // the marker itself appears quoted in prose.
            let code = rest
                .trim()
                .split([' ', ','])
                .next()
                .unwrap_or_default()
                .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-');
            annotated += 1;
            if code == "clean" {
                continue;
            }
            assert!(
                codes.contains(code),
                "{}:{}: `{code}` is not a code in specs/23",
                path.display(),
                number + 1
            );
        }
    }

    assert!(annotated >= 30, "only {annotated} annotations found");
}

/// The other direction: a check with no artifact here has no test. Codes are
/// covered by an inline annotation, or — where the format takes no comments —
/// by the expectations table in `tests/fixtures/README.md`.
#[test]
fn every_code_in_the_registry_has_a_fixture() {
    let spec = fs::read_to_string(repo_root().join("docs/specs/23-diagnostics.md"))
        .expect("spec 23 is readable");
    let readme = fs::read_to_string(repo_root().join("tests/fixtures/README.md"))
        .expect("fixture README is readable");

    let annotations: String = walk(&bench())
        .iter()
        .filter_map(|path| fs::read_to_string(path).ok())
        .collect();

    let missing: Vec<&str> = registry(&spec)
        .into_iter()
        .filter(|code| {
            !annotations.contains(&format!("expect: {code}"))
                && !readme.contains(&format!("`{code}`"))
        })
        .collect();

    assert!(
        missing.is_empty(),
        "no fixture produces: {}",
        missing.join(", ")
    );
}
