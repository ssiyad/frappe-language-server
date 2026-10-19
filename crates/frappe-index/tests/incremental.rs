//! The invariant of [32](../../../docs/specs/32-testing.md): an incremental
//! re-index yields the graph a full rebuild would.
//!
//! Property tested against hand-written generators rather than a crate. The
//! input space that matters here is small and named — a field added, a DocType
//! renamed, a fixture deleted — and a generator that enumerates those is both
//! reproducible and readable, which a shrinking random one is not.
//!
//! Everything runs against a copy of the fixture bench, because the bench
//! itself is frozen (`tests/fixtures/README.md`).

use std::fs;
use std::path::{Path, PathBuf};

use frappe_index::Index;
use serde_json::{Value, json};

// --- The copy under test -----------------------------------------------------

struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "frappe-lsp-incremental-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        copy_tree(&fixture_bench(), &root);
        Self { root }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.root.join(relative)
    }

    /// The whole point: patch, then compare against a rebuild from scratch.
    fn assert_incremental_equals_rebuild(&self, index: &mut Index, touched: &Path, label: &str) {
        index.reindex(touched);
        let rebuilt = Index::build(&self.root);

        let incremental = index.fingerprint();
        let full = rebuilt.fingerprint();
        if incremental != full {
            panic!(
                "{label}: incremental re-index diverged\n{}",
                diff(&incremental, &full)
            );
        }
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn fixture_bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the repo root")
        .join("tests/fixtures/bench")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("destination");
    for entry in fs::read_dir(from).expect("readable source").flatten() {
        let source = entry.path();
        let target = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &target);
        } else {
            fs::copy(&source, &target).expect("copy");
        }
    }
}

/// The first differing line of two fingerprints, so a failure names the entity
/// that diverged rather than dumping the whole graph.
fn diff(incremental: &str, full: &str) -> String {
    for (at, (left, right)) in incremental.lines().zip(full.lines()).enumerate() {
        if left != right {
            return format!(
                "line {}:\n  incremental: {left}\n  rebuild:     {right}",
                at + 1
            );
        }
    }
    let only_incremental: Vec<&str> = incremental
        .lines()
        .filter(|line| !full.lines().any(|other| other == *line))
        .collect();
    let only_full: Vec<&str> = full
        .lines()
        .filter(|line| !incremental.lines().any(|other| other == *line))
        .collect();
    format!(
        "only in incremental:\n  {}\nonly in rebuild:\n  {}",
        only_incremental.join("\n  "),
        only_full.join("\n  ")
    )
}

// --- Generators --------------------------------------------------------------

const MEMBER: &str = "apps/library/library/library/doctype/library_member/library_member.json";
const MEMBERSHIP: &str =
    "apps/library/library/library/doctype/library_membership/library_membership.json";
const AVAILABILITY: &str = "apps/circulation-app/circulation/circulation/doctype/library_availability/library_availability.json";
const CUSTOM_FIELDS: &str = "apps/library/library/fixtures/custom_field.json";
const PROPERTY_SETTERS: &str = "apps/library/library/fixtures/property_setter.json";
const CONTROLLER: &str = "apps/library/library/library/doctype/library_member/library_member.py";
const API: &str = "apps/library/library/api.py";
const NEW_DOCTYPE: &str = "apps/library/library/library/doctype/library_shelf/library_shelf.json";
const NEW_FIXTURE: &str = "apps/library/library/fixtures/extra_custom_field.json";

/// One deterministic mutation. Each returns the file that changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Edit {
    AddField,
    RemoveField,
    RenameDocType,
    MakeChild,
    DeleteDocType,
    RestoreDocType,
    Corrupt,
    Repair,
    AddDocType,
    AddCustomField,
    RemoveCustomField,
    ChangePropertySetter,
    DeleteFixtureFile,
    AddFixtureFile,
    AddWhitelistedFunction,
    DropWhitelistDecorator,
    RemoveDuplicateDocType,
    RewriteControllerClass,
}

const EVERY_EDIT: [Edit; 18] = [
    Edit::AddField,
    Edit::RemoveField,
    Edit::RenameDocType,
    Edit::MakeChild,
    Edit::DeleteDocType,
    Edit::RestoreDocType,
    Edit::Corrupt,
    Edit::Repair,
    Edit::AddDocType,
    Edit::AddCustomField,
    Edit::RemoveCustomField,
    Edit::ChangePropertySetter,
    Edit::DeleteFixtureFile,
    Edit::AddFixtureFile,
    Edit::AddWhitelistedFunction,
    Edit::DropWhitelistDecorator,
    Edit::RemoveDuplicateDocType,
    Edit::RewriteControllerClass,
];

impl Edit {
    fn apply(self, sandbox: &Sandbox, step: usize) -> PathBuf {
        match self {
            Self::AddField => {
                let path = sandbox.path(MEMBER);
                edit_json(&path, |value| {
                    let name = format!("generated_{step}");
                    push(
                        value,
                        "fields",
                        json!({
                            "fieldname": name,
                            "fieldtype": "Data",
                            "label": format!("Generated {step}"),
                        }),
                    );
                    push(value, "field_order", json!(name));
                });
                path
            }
            Self::RemoveField => {
                let path = sandbox.path(MEMBER);
                edit_json(&path, |value| {
                    drop_field(value, "email_id");
                });
                path
            }
            Self::RenameDocType => {
                let path = sandbox.path(MEMBERSHIP);
                edit_json(&path, |value| {
                    value["name"] = json!(format!("Library Membership {step}"));
                });
                path
            }
            Self::MakeChild => {
                let path = sandbox.path(MEMBERSHIP);
                edit_json(&path, |value| {
                    value["istable"] = json!(1);
                });
                path
            }
            Self::DeleteDocType => {
                let path = sandbox.path(MEMBER);
                let _ = fs::remove_file(&path);
                path
            }
            Self::RestoreDocType => {
                let path = sandbox.path(MEMBER);
                fs::create_dir_all(path.parent().expect("a directory")).expect("directory");
                fs::copy(fixture_bench().join(MEMBER), &path).expect("restore");
                path
            }
            Self::Corrupt => {
                let path = sandbox.path(MEMBERSHIP);
                fs::write(&path, "{ this is not json").expect("write");
                path
            }
            Self::Repair => {
                let path = sandbox.path(MEMBERSHIP);
                fs::copy(fixture_bench().join(MEMBERSHIP), &path).expect("restore");
                path
            }
            Self::AddDocType => {
                let path = sandbox.path(NEW_DOCTYPE);
                fs::create_dir_all(path.parent().expect("a directory")).expect("directory");
                write_json(
                    &path,
                    &json!({
                        "doctype": "DocType",
                        "name": format!("Library Shelf {step}"),
                        "module": "Library",
                        "field_order": ["code"],
                        "fields": [{"fieldname": "code", "fieldtype": "Data", "label": "Code"}],
                        "permissions": [],
                        "links": [],
                        "states": [],
                    }),
                );
                path
            }
            Self::AddCustomField => {
                let path = sandbox.path(CUSTOM_FIELDS);
                edit_json(&path, |value| {
                    let name = format!("extra_{step}");
                    if let Some(array) = value.as_array_mut() {
                        array.push(json!({
                            "doctype": "Custom Field",
                            "dt": "Library Member",
                            "fieldname": name,
                            "fieldtype": "Data",
                            "insert_after": "loans",
                            "label": "Extra",
                            "name": format!("Library Member-{name}"),
                        }));
                    }
                });
                path
            }
            Self::RemoveCustomField => {
                let path = sandbox.path(CUSTOM_FIELDS);
                edit_json(&path, |value| {
                    if let Some(array) = value.as_array_mut() {
                        array.retain(|record| {
                            record.get("fieldname").and_then(Value::as_str)
                                != Some("loyalty_points")
                        });
                    }
                });
                path
            }
            Self::ChangePropertySetter => {
                let path = sandbox.path(PROPERTY_SETTERS);
                edit_json(&path, |value| {
                    if let Some(record) = value.get_mut(1) {
                        record["value"] = json!(format!("member_name,email_id,x{step}"));
                    }
                });
                path
            }
            Self::DeleteFixtureFile => {
                let path = sandbox.path(CUSTOM_FIELDS);
                let _ = fs::remove_file(&path);
                path
            }
            Self::AddFixtureFile => {
                let path = sandbox.path(NEW_FIXTURE);
                write_json(
                    &path,
                    &json!([{
                        "doctype": "Custom Field",
                        "dt": "Library Settings",
                        "fieldname": format!("late_fee_{step}"),
                        "fieldtype": "Currency",
                        "insert_after": "max_loans",
                        "label": "Late Fee",
                        "name": "Library Settings-late_fee",
                    }]),
                );
                path
            }
            Self::AddWhitelistedFunction => {
                let path = sandbox.path(API);
                let mut source = fs::read_to_string(&path).expect("readable");
                source.push_str(&format!(
                    "\n\n@frappe.whitelist(allow_guest=True)\ndef generated_{step}(member: str) -> int:\n\treturn 0\n"
                ));
                fs::write(&path, source).expect("write");
                path
            }
            Self::DropWhitelistDecorator => {
                let path = sandbox.path(CONTROLLER);
                let source = fs::read_to_string(&path)
                    .expect("readable")
                    .replace("\t@frappe.whitelist()\n", "");
                fs::write(&path, source).expect("write");
                path
            }
            Self::RemoveDuplicateDocType => {
                let path = sandbox.path(AVAILABILITY);
                let _ = fs::remove_file(&path);
                path
            }
            Self::RewriteControllerClass => {
                let path = sandbox.path(CONTROLLER);
                fs::write(
                    &path,
                    format!(
                        "import frappe\nfrom frappe.model.document import Document\n\n\nclass Rewritten{step}(Document):\n\tdef validate(self):\n\t\tpass\n"
                    ),
                )
                .expect("write");
                path
            }
        }
    }
}

fn edit_json(path: &Path, mutate: impl FnOnce(&mut Value)) {
    let Ok(source) = fs::read_to_string(path) else {
        return;
    };
    let Ok(mut value) = serde_json::from_str::<Value>(&source) else {
        return;
    };
    mutate(&mut value);
    write_json(path, &value);
}

fn write_json(path: &Path, value: &Value) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("directory");
    }
    fs::write(
        path,
        serde_json::to_string_pretty(value).expect("serializes"),
    )
    .expect("write");
}

fn push(value: &mut Value, key: &str, item: Value) {
    if let Some(array) = value.get_mut(key).and_then(Value::as_array_mut) {
        array.push(item);
    }
}

fn drop_field(value: &mut Value, fieldname: &str) {
    if let Some(array) = value.get_mut("fields").and_then(Value::as_array_mut) {
        array.retain(|field| field.get("fieldname").and_then(Value::as_str) != Some(fieldname));
    }
    if let Some(array) = value.get_mut("field_order").and_then(Value::as_array_mut) {
        array.retain(|name| name.as_str() != Some(fieldname));
    }
}

// --- The property ------------------------------------------------------------

#[test]
fn every_single_edit_re_indexes_to_the_same_graph_as_a_rebuild() {
    for edit in EVERY_EDIT {
        let sandbox = Sandbox::new(&format!("single-{edit:?}"));
        let mut index = Index::build(&sandbox.root);
        assert!(index.status().doctypes > 0, "the copy indexed at all");

        let touched = edit.apply(&sandbox, 1);
        sandbox.assert_incremental_equals_rebuild(&mut index, &touched, &format!("{edit:?}"));
    }
}

#[test]
fn a_long_sequence_of_edits_stays_equal_to_a_rebuild() {
    let sandbox = Sandbox::new("sequence");
    let mut index = Index::build(&sandbox.root);

    // A fixed multiplicative generator, so a failure is reproducible from the
    // step number alone. Cumulative: every edit lands on a tree the previous
    // edits already changed, which is where a stale-cache bug shows up.
    let mut state: u64 = 0x2545_f491_4f6c_dd1d;
    for step in 1..=120 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let edit = EVERY_EDIT[(state >> 33) as usize % EVERY_EDIT.len()];

        let touched = edit.apply(&sandbox, step);
        sandbox.assert_incremental_equals_rebuild(
            &mut index,
            &touched,
            &format!("step {step} ({edit:?})"),
        );
    }
}

#[test]
fn re_indexing_an_unchanged_file_changes_nothing() {
    let sandbox = Sandbox::new("noop");
    let mut index = Index::build(&sandbox.root);
    let before = index.fingerprint();

    for relative in [MEMBER, CUSTOM_FIELDS, PROPERTY_SETTERS, CONTROLLER, API] {
        let outcome = index.reindex(&sandbox.path(relative));
        assert!(outcome.known, "{relative} is a file the index reads");
    }

    assert_eq!(index.fingerprint(), before);
}

#[test]
fn a_file_the_index_does_not_read_is_reported_as_unknown() {
    let sandbox = Sandbox::new("unknown");
    let mut index = Index::build(&sandbox.root);
    let before = index.fingerprint();

    // Outside every app package.
    let outside = sandbox.path("sites/apps.txt");
    assert!(!index.reindex(&outside).known);

    // Inside a package, but neither schema, fixture nor Python.
    let asset = sandbox.path("apps/library/library/public/js/library.js");
    assert!(!index.reindex(&asset).known);

    // A JSON beside a DocType that the record rules of spec 03 turn away.
    let rejected =
        sandbox.path("apps/library/library/library/doctype/library_member/test_records.json");
    let outcome = index.reindex(&rejected);
    assert!(outcome.known, "it is a JSON in a record position");
    assert!(outcome.doctypes.is_empty(), "but it defines no DocType");

    assert_eq!(index.fingerprint(), before);
}

#[test]
fn a_re_index_touches_only_the_doctypes_the_file_names() {
    let sandbox = Sandbox::new("scope");
    let mut index = Index::build(&sandbox.root);

    let touched = Edit::AddField.apply(&sandbox, 1);
    assert_eq!(index.reindex(&touched).doctypes, ["Library Member"]);

    // A fixtures file names two DocTypes, so both rebuild.
    let touched = Edit::AddCustomField.apply(&sandbox, 2);
    assert_eq!(
        index.reindex(&touched).doctypes,
        ["Library Member", "Library Membership"]
    );

    // A rename rebuilds both the name that left and the name that arrived.
    let touched = Edit::RenameDocType.apply(&sandbox, 3);
    assert_eq!(
        index.reindex(&touched).doctypes,
        ["Library Membership", "Library Membership 3"]
    );
}

#[test]
fn the_revision_moves_only_when_the_index_can_have_changed() {
    // A long computation on a snapshot installs its result only if the
    // revision held. A path in no app must not move it, or a watcher event
    // for some other directory throws away seconds of work.
    let sandbox = Sandbox::new("revision");
    let mut index = Index::build(&sandbox.root);
    let before = index.revision();

    index.reindex(Path::new("/nonexistent/app/hooks.py"));
    assert_eq!(index.revision(), before, "a path outside the bench");

    index.reindex(&sandbox.path("apps/library/library/api.py"));
    assert_ne!(index.revision(), before, "a file in an app");
}
