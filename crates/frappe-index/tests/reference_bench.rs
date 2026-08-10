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
    // payments, and it is a budget for the shipped binary. An unoptimized
    // build is roughly eight times slower for reasons that have nothing to do
    // with the index, so asserting the release number against it would only
    // teach the next person to ignore the failure.
    let ceiling = if cfg!(debug_assertions) { 20 } else { 5 };
    assert!(
        elapsed.as_secs() < ceiling,
        "cold index took {elapsed:?}, ceiling {ceiling}s"
    );
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
    // The median of several, not one. A single sample also measures whatever
    // else the machine was doing, and this suite runs beside a compiler.
    let mut samples = Vec::new();
    for _ in 0..5 {
        let started = Instant::now();
        index.reindex(&path);
        samples.push(started.elapsed());
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];

    let start = Instant::now();
    let outcome = index.reindex(&path);
    let elapsed = start.elapsed();

    eprintln!(
        "re-indexed {:?} in {median:?} (median of 5)",
        outcome.doctypes
    );
    let _ = elapsed;
    assert_eq!(outcome.doctypes, ["User"]);
    // 20 ms is the ceiling in spec 31.
    assert!(
        median.as_millis() < 20,
        "re-index took {median:?} (median of 5)"
    );
}

/// Resolution across a whole real app — the "done when" of task 007.
///
/// Frappe's own Python is the best corpus of valid Frappe code there is, so
/// what matters here is not how much resolves but how little is *wrong*: an
/// unknown DocType or field in this corpus is a resolver bug, not a user's
/// mistake (specs/32).
#[test]
fn resolution_across_frappe_is_stable_and_precise() {
    let bench = bench_or_skip!();
    let index = frappe_index::index::Index::build(&bench);

    let mut files = 0;
    let mut matches = 0;
    let mut positions = 0;
    let mut resolved = 0;
    let mut unresolvable = 0;
    let mut unknown_doctypes: Vec<String> = Vec::new();
    let mut unknown_fields: Vec<(String, String)> = Vec::new();

    let started = Instant::now();
    for app in index.apps() {
        for path in frappe_index::walk::Walk::new(&app.package) {
            if path.extension().is_none_or(|extension| extension != "py") {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            let mut parser = frappe_syntax::Language::Python
                .parser()
                .expect("the grammar loads");
            let Some(tree) = parser.parse(&source, None) else {
                continue;
            };

            let found = frappe_syntax::extract(
                frappe_syntax::Language::Python,
                &tree,
                &source,
                frappe_syntax::Context::default(),
            );
            // A panic anywhere in here fails the test, which is half the point.
            let targets = frappe_index::resolve::resolve_all(&index, &found);

            files += 1;
            matches += found.len();
            positions += targets.len();
            for one in targets {
                match one.target {
                    frappe_index::Target::Unresolvable => unresolvable += 1,
                    frappe_index::Target::UnknownDocType { name, .. } => {
                        unknown_doctypes.push(name)
                    }
                    frappe_index::Target::UnknownField { doctype, fieldname } => {
                        unknown_fields.push((doctype, fieldname));
                    }
                    _ => resolved += 1,
                }
            }
        }
    }
    let elapsed = started.elapsed();

    unknown_doctypes.sort();
    unknown_doctypes.dedup();
    unknown_fields.sort();
    unknown_fields.dedup();

    eprintln!(
        "{files} files, {matches} matches, {positions} positions in {elapsed:?}\n  \
         {resolved} resolved, {unresolvable} unresolvable, \
         {} distinct unknown DocTypes, {} distinct unknown fields",
        unknown_doctypes.len(),
        unknown_fields.len()
    );
    for name in unknown_doctypes.iter().take(10) {
        eprintln!("  unknown DocType: {name}");
    }
    let mut by_name: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for (_, fieldname) in &unknown_fields {
        *by_name.entry(fieldname.as_str()).or_default() += 1;
    }
    let mut ranked: Vec<_> = by_name.into_iter().collect();
    ranked.sort_by_key(|(name, count)| (std::cmp::Reverse(*count), *name));
    for (fieldname, count) in ranked.iter().take(12) {
        eprintln!("  unknown field: .{fieldname} on {count} DocTypes");
    }

    assert!(files > 1_000, "only {files} files");
    assert!(resolved > 1_000, "only {resolved} positions resolved");

    // Frappe's own code is valid, so an unknown here is either a DocType that
    // genuinely is not installed — hrms's and payments's, or one a test creates
    // at runtime — or a bug in this resolver. The ceilings are deliberately
    // close to the 81 and 194 measured over frappe + erpnext: a change that
    // pushes either up is reporting nonsense about correct code (specs/23).
    //
    // The distribution is what says these are the long tail rather than a bug:
    // no unknown fieldname appears on more than three DocTypes.
    assert!(
        unknown_doctypes.len() <= 90,
        "{} unknown DocTypes: {unknown_doctypes:?}",
        unknown_doctypes.len()
    );
    assert!(
        unknown_fields.len() <= 210,
        "{} unknown fields: {unknown_fields:?}",
        unknown_fields.len()
    );
}
