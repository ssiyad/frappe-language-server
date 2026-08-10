//! Discovery against a real bench — the "done when" of task 004.
//!
//! The bench is a developer's checkout, not something CI can clone cheaply, so
//! these skip when it is absent. Point `FRAPPE_LSP_REFERENCE_BENCH` at one to
//! run them elsewhere; the default is `~/sources/frappe-dev`.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Instant;

use frappe_index::artifacts::Rejection;
use frappe_index::{Mode, Workspace, artifacts};

fn reference_bench() -> Option<PathBuf> {
    let path = std::env::var_os("FRAPPE_LSP_REFERENCE_BENCH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("sources/frappe-dev"))
        })?;
    path.join("sites").is_dir().then_some(path)
}

macro_rules! bench_or_skip {
    () => {
        match reference_bench() {
            Some(bench) => bench,
            None => {
                eprintln!("skipped: no reference bench (set FRAPPE_LSP_REFERENCE_BENCH)");
                return;
            }
        }
    };
}

#[test]
fn frappe_yields_its_doctypes_and_rejects_the_rest() {
    let bench = bench_or_skip!();
    let workspace = Workspace::discover(&bench);
    assert_eq!(workspace.detection.mode, Mode::Bench);

    let frappe = workspace.app("frappe").expect("frappe is installed");
    let doctypes = frappe.artifacts.doctypes().count();
    let rejected: Vec<_> = frappe.artifacts.rejected_in("doctype").collect();
    let candidates = doctypes + rejected.len();

    eprintln!(
        "frappe: {candidates} candidates, {doctypes} DocTypes, {} rejected",
        rejected.len()
    );

    // Spec 03 measured 316/288/28 on an earlier checkout; the ratio is what is
    // stable across versions, not the totals.
    assert!(doctypes > 280, "only {doctypes} DocTypes");
    assert_eq!(
        rejected.len(),
        28,
        "the rejected set changed: {:?}",
        rejected.iter().map(|file| &file.path).collect::<Vec<_>>()
    );
    assert!(
        rejected
            .iter()
            .all(|file| file.rejection == Rejection::StemMismatch),
        "every rejection on this bench is a stem mismatch"
    );
    assert_eq!(candidates - rejected.len(), doctypes);
}

#[test]
fn the_stale_rename_is_excluded_and_costs_nothing() {
    let bench = bench_or_skip!();
    let workspace = Workspace::discover(&bench);

    // `apps/resource-management/` (package `frappe_sre`) is a stale rename of
    // `apps/resource_management/`, and apps.txt lists only the latter.
    assert!(
        workspace
            .excluded
            .iter()
            .any(|path| path.ends_with("resource-management")),
        "excluded: {:?}",
        workspace.excluded
    );

    let duplicates = workspace.duplicate_doctypes();
    assert!(
        duplicates.is_empty(),
        "{} duplicate DocTypes across installed apps: {:?}",
        duplicates.len(),
        duplicates.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );

    // What the rule is worth: the DocType names the excluded app would have
    // collided with, had it been indexed.
    let stale = bench.join("apps/resource-management/frappe_sre");
    if stale.is_dir() {
        let (artifacts, _) = artifacts::scan(&stale);
        let stale_names: BTreeSet<&str> = artifacts
            .doctypes()
            .map(|record| record.name.as_str())
            .collect();
        let live_names: BTreeSet<&str> = workspace
            .doctypes()
            .map(|(_, record)| record.name.as_str())
            .collect();
        let collisions = stale_names.intersection(&live_names).count();

        eprintln!(
            "stale app holds {} DocTypes, {collisions} of which would collide",
            stale_names.len()
        );
        assert_eq!(
            collisions,
            stale_names.len(),
            "the stale rename is supposed to be a full duplicate"
        );
        assert!(collisions >= 40, "only {collisions} collisions");
    }
}

#[test]
fn the_frappe_version_is_detected() {
    let bench = bench_or_skip!();
    let workspace = Workspace::discover(&bench);

    let version = workspace.frappe_version().expect("a version");
    eprintln!("frappe version: {version}");
    assert!(
        version.starts_with(char::is_numeric),
        "unexpected version {version:?}"
    );
}

#[test]
fn every_installed_app_is_named_by_hooks_not_by_its_directory() {
    let bench = bench_or_skip!();
    let workspace = Workspace::discover(&bench);

    for app in &workspace.apps {
        let directory = app.directory.file_name().and_then(|name| name.to_str());
        let package = app.package.file_name().and_then(|name| name.to_str());
        eprintln!(
            "{:24} package={:24} name={}",
            directory.unwrap_or("?"),
            package.unwrap_or("?"),
            app.name
        );
        assert!(!app.name.is_empty());
        assert!(app.artifacts.hooks.is_some());
    }

    assert!(workspace.apps.len() >= 2);
}

#[test]
fn a_whole_bench_discovers_in_well_under_a_second() {
    let bench = bench_or_skip!();

    let start = Instant::now();
    let workspace = Workspace::discover(&bench);
    let elapsed = start.elapsed();

    eprintln!(
        "discovered {} apps, {} DocTypes in {elapsed:?}",
        workspace.apps.len(),
        workspace.doctypes().count()
    );
    // Discovery is the cheap half of the index; a budget this loose only
    // catches an accidental full-bench walk.
    assert!(elapsed.as_secs() < 5, "discovery took {elapsed:?}");
}

// --- The metadata graph, task 005 --------------------------------------------

#[test]
fn the_whole_bench_indexes_within_the_cold_budget() {
    let bench = bench_or_skip!();

    let start = Instant::now();
    let index = frappe_index::Index::build(&bench);
    let elapsed = start.elapsed();
    let status = index.status();

    eprintln!(
        "{} apps, {} DocTypes, {} fields, {} controllers, {} whitelisted methods, {} modules, {} interned names in {elapsed:?}",
        status.apps,
        status.doctypes,
        status.fields,
        status.controllers,
        status.whitelisted_methods,
        status.modules,
        index.interned(),
    );
    for conflict in index.conflicts() {
        eprintln!("conflict {} {}", conflict.code.as_str(), conflict.doctype);
    }

    // The 5 s ceiling of spec 31 is stated for frappe + erpnext + hrms +
    // payments; this bench is frappe plus two custom apps, so the ceiling is
    // the only honest assertion to make against it.
    assert!(elapsed.as_secs() < 5, "cold index took {elapsed:?}");
    assert!(status.doctypes > 280, "only {} DocTypes", status.doctypes);
    assert!(status.fields > 3000, "only {} fields", status.fields);
    assert!(
        status.whitelisted_methods > 300,
        "only {} whitelisted methods",
        status.whitelisted_methods
    );
}

#[test]
fn a_frappe_doctype_merges_with_its_controller_and_implicit_fields() {
    let bench = bench_or_skip!();
    let index = frappe_index::Index::build(&bench);

    let user = index.doctype("User").expect("frappe defines User");
    assert_eq!(&*user.app, "frappe");
    assert!(!user.fields.is_empty());
    assert!(user.controller.is_some(), "user.py is beside user.json");

    for fieldname in ["name", "owner", "creation", "modified", "docstatus", "idx"] {
        assert!(index.has_field("User", fieldname), "{fieldname}");
    }
    assert!(!index.has_field("User", "no_such_field_anywhere"));
}

#[test]
fn frappes_own_whitelisted_methods_resolve_as_whitelisted() {
    use frappe_index::paths::{Kind, Outcome};

    let bench = bench_or_skip!();
    let index = frappe_index::Index::build(&bench);

    let sample: Vec<&str> = index
        .whitelisted_methods()
        .map(|method| method.dotted.as_str())
        .take(5)
        .collect();
    eprintln!("first whitelisted paths: {sample:?}");

    for dotted in sample {
        assert_eq!(
            index.resolve(dotted, Kind::Whitelisted).outcome,
            Outcome::Resolved,
            "{dotted} is in the registry but does not resolve"
        );
    }

    // A path that cannot exist must not resolve, on a bench this size either.
    assert_eq!(
        index
            .resolve("frappe.no_such_module.no_such_function", Kind::Callable)
            .outcome,
        Outcome::Unresolved
    );
}

#[test]
fn re_indexing_one_doctype_json_stays_inside_the_incremental_budget() {
    let bench = bench_or_skip!();
    let mut index = frappe_index::Index::build(&bench);

    let path = index
        .doctype("User")
        .expect("frappe defines User")
        .path
        .to_path_buf();

    // Warm the page cache, then measure the steady state.
    index.reindex(&path);
    let start = Instant::now();
    let outcome = index.reindex(&path);
    let elapsed = start.elapsed();

    eprintln!("re-indexed {:?} in {elapsed:?}", outcome.doctypes);
    assert_eq!(outcome.doctypes, ["User"]);
    // 20 ms is the ceiling in spec 31.
    assert!(elapsed.as_millis() < 20, "re-index took {elapsed:?}");
}
