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
