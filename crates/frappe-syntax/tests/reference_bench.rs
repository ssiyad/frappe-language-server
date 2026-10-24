//! Extraction over a real app — the corpus that catches what snippets cannot.
//!
//! Extraction itself needs no bench, which is the point of
//! [02](../../../docs/specs/02-architecture.md); this is a scale and stability
//! check, so it skips when the bench is absent. Point
//! `FRAPPE_LSP_REFERENCE_BENCH` at one to run it elsewhere; the default is
//! `~/sources/frappe-dev`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use frappe_syntax::{Anchor, Context, Language, Match, extract};

fn reference_app() -> Option<PathBuf> {
    let bench = std::env::var_os("FRAPPE_LSP_REFERENCE_BENCH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("sources/frappe-dev"))
        })?;
    let app = bench.join("apps/frappe/frappe");
    app.is_dir().then_some(app)
}

fn sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                let name = entry.file_name();
                if name != "node_modules" && name != "dist" && name != "__pycache__" {
                    stack.push(path);
                }
            } else if Language::from_path(&path).is_some_and(|language| language != Language::Json)
            {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

struct Totals {
    files: usize,
    bytes: usize,
    matches: usize,
    slots: usize,
    anchored: usize,
    unknown: usize,
    patterns: BTreeMap<String, usize>,
    parse: std::time::Duration,
    extract: std::time::Duration,
}

fn run() -> Option<Totals> {
    let app = reference_app()?;
    let mut totals = Totals {
        files: 0,
        bytes: 0,
        matches: 0,
        slots: 0,
        anchored: 0,
        unknown: 0,
        patterns: BTreeMap::new(),
        parse: std::time::Duration::ZERO,
        extract: std::time::Duration::ZERO,
    };

    for path in sources(&app) {
        let Some(language) = Language::from_path(&path) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        let mut parser = language.parser().expect("the grammar loads");

        let started = Instant::now();
        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };
        totals.parse += started.elapsed();

        let started = Instant::now();
        let found: Vec<Match> = extract(language, &tree, &source, Context::default());
        totals.extract += started.elapsed();

        totals.files += 1;
        totals.bytes += source.len();
        totals.matches += found.len();
        for one in &found {
            *totals.patterns.entry(one.pattern.to_string()).or_default() += 1;
            totals.slots += one.slots.len();
            for slot in &one.slots {
                if slot.anchor == Anchor::Unknown {
                    totals.unknown += 1;
                } else {
                    totals.anchored += 1;
                }
            }
        }
    }

    Some(totals)
}

#[test]
fn extraction_over_frappe_is_stable_and_finds_every_pattern() {
    let Some(totals) = run() else {
        eprintln!("skipped: no reference bench (set FRAPPE_LSP_REFERENCE_BENCH)");
        return;
    };

    eprintln!(
        "{} files, {} bytes, {} matches, {} slots ({} anchored, {} unknown)",
        totals.files, totals.bytes, totals.matches, totals.slots, totals.anchored, totals.unknown
    );
    eprintln!("parse {:?}, extract {:?}", totals.parse, totals.extract);
    let mut ranked: Vec<_> = totals.patterns.iter().collect();
    ranked.sort_by_key(|(name, count)| (std::cmp::Reverse(**count), (*name).clone()));
    for (name, count) in ranked.iter().take(10) {
        eprintln!("  {count:6}  {name}");
    }

    // The oracle for any change to how sites are located: the shape tables did
    // not move, so neither may these. Measured over `apps/frappe/frappe` at
    // 17.0.0-dev `c7a96ecc99` — a different corpus gives different totals, so
    // a moved checkout fails here first. Re-measure at the commit before your
    // change to tell the corpus from the change.
    // Templates joined the walk with spec 13: 223 files, 391 matches and 404
    // slots, 382 of them unknown, mostly `_()` in markup.
    // 470 joined with query rows (specs/08): a `row.attribute` match each, one
    // anchored slot each, and nothing that matched before moved.
    assert_eq!(totals.matches, 22_577, "match count moved");
    // 37 slots joined when `frappe.get_value` took `frappe.db.get_value`'s
    // shape: the fields of frappe's 32 calls, 34 of them anchored.
    // 12 left when `pluck` beside `fields` stopped naming a field: it reads a
    // key of the selected rows ([10](../../../docs/specs/10-surface-python.md)).
    // 598 joined when `frappe.db.exists`, `count` and `delete` took a filter
    // dict second, 561 of them anchored.
    assert_eq!(totals.slots, 30_711, "slot count moved");
    // 1 325 slots moved from anchored to unknown when `frm` inside
    // `frappe.ui.form.on(D, …)` became a binding the resolver settles: `D`
    // may be a child, whose block runs on its parent's form (specs/11).
    assert_eq!(totals.anchored, 17_685, "anchored slot count moved");
    assert_eq!(totals.unknown, 13_026, "unknown slot count moved");

    // A pattern that never fires on the reference corpus is a pattern nothing
    // is testing at scale.
    assert!(
        totals.patterns.len() >= 50,
        "only {} patterns fired",
        totals.patterns.len()
    );
}
