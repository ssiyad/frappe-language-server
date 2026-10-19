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

/// One test at a time. Each indexes the whole bench across every core, so
/// run side by side they measure each other: the cold budget failed at 20 s
/// beside nine other builds and passes in 5 s alone.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

macro_rules! bench_or_skip {
    ($bench:ident) => {
        let _serial = SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let $bench = match reference_bench() {
            Some(bench) => bench,
            None => {
                eprintln!("skipped: no reference bench (set FRAPPE_LSP_REFERENCE_BENCH)");
                return;
            }
        };
    };
}

#[test]
fn frappe_yields_its_doctypes_and_rejects_the_rest() {
    bench_or_skip!(bench);
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
fn an_app_on_disk_but_not_in_apps_txt_costs_nothing() {
    bench_or_skip!(bench);
    let workspace = Workspace::discover(&bench);

    // Whether this bench has such an app is the developer's business, not the
    // repository's: the rule itself is asserted on the fixture bench, where
    // `library-old` is a committed duplicate of `library`. Here it is checked
    // against whatever a real bench happens to hold, and holding nothing is a
    // fine answer.
    let Some(excluded) = workspace.excluded.first().cloned() else {
        eprintln!("no excluded app on this bench; nothing to check");
        return;
    };
    eprintln!("excluded: {}", excluded.display());

    let duplicates = workspace.duplicate_doctypes();
    assert!(
        duplicates.is_empty(),
        "{} duplicate DocTypes across installed apps: {:?}",
        duplicates.len(),
        duplicates.iter().map(|(name, _)| name).collect::<Vec<_>>()
    );

    // What the rule is worth: every DocType the excluded app would have
    // contributed is one the index did not see.
    for package in std::fs::read_dir(&excluded).into_iter().flatten().flatten() {
        let package = package.path();
        if !package.is_dir() {
            continue;
        }
        let (artifacts, _) = artifacts::scan(&package);
        let stale: BTreeSet<&str> = artifacts
            .doctypes()
            .map(|record| record.name.as_str())
            .collect();
        if stale.is_empty() {
            continue;
        }
        let live: BTreeSet<&str> = workspace
            .doctypes()
            .map(|(_, record)| record.name.as_str())
            .collect();
        eprintln!(
            "{} holds {} DocTypes, {} of which would collide",
            package.display(),
            stale.len(),
            stale.intersection(&live).count()
        );
    }
}

#[test]
fn the_frappe_version_is_detected() {
    bench_or_skip!(bench);
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
    bench_or_skip!(bench);
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
    bench_or_skip!(bench);

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
    bench_or_skip!(bench);

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
    bench_or_skip!(bench);
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

    bench_or_skip!(bench);
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
    bench_or_skip!(bench);
    let mut index = frappe_index::Index::build(&bench);

    let path = index
        .doctype("User")
        .expect("frappe defines User")
        .path
        .to_path_buf();

    // Warm the page cache, then measure the steady state.
    index.reindex(&path);

    // The *fastest* of several, not the median. The budget asks whether this
    // work is small enough to be done in 20 ms; the quickest sample is the one
    // least polluted by whatever else the machine is running, and this suite
    // runs beside a compiler. A median still fails when four of five samples
    // land during someone's `cargo build`, which is a fact about the machine
    // and not about the index.
    let mut samples = Vec::new();
    let mut outcome = index.reindex(&path);
    for _ in 0..9 {
        let started = Instant::now();
        outcome = index.reindex(&path);
        samples.push(started.elapsed());
    }
    let fastest = samples.iter().min().copied().expect("ten samples");
    samples.sort_unstable();

    eprintln!(
        "re-indexed {:?} in {fastest:?} (fastest of 9, median {:?})",
        outcome.doctypes,
        samples[samples.len() / 2]
    );
    assert_eq!(outcome.doctypes, ["User"]);
    // 20 ms is the ceiling in spec 31.
    assert!(
        fastest.as_millis() < 20,
        "re-index took {fastest:?} at its fastest of 9"
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
    bench_or_skip!(bench);
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

    // No ceiling on the unknown counts. They are a function of which apps the
    // developer happens to have installed — an app that is not there is a
    // DocType that does not resolve — so an absolute number here fails for
    // reasons that have nothing to do with the resolver.
    //
    // Precision is enforced by `cargo xtask smoke`, which checks `frappe` and
    // `erpnext` alone against a committed allowlist and fails on anything not
    // on it (specs/32). What this test is worth is the walk itself: 5 000
    // files of real code through extraction and resolution without a panic,
    // and the report above, which is where a regression shows up as a
    // fieldname suddenly appearing on thirty DocTypes instead of three.
}
