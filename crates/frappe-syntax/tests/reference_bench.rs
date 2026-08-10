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
    // 17.0.0-dev — a different corpus gives different totals, which is why the
    // walk is committed here rather than run ad hoc.
    assert_eq!(totals.matches, 16_291, "match count moved");
    assert_eq!(totals.slots, 21_199, "slot count moved");
    // Two slots moved from anchored to unknown when the legacy
    // `frappe.ui.form.on(D, event, fn)` form stopped being read as a fieldname:
    // an event name carries no anchor of its own (specs/11).
    assert_eq!(totals.anchored, 12_509, "anchored slot count moved");
    assert_eq!(totals.unknown, 8_690, "unknown slot count moved");

    // A pattern that never fires on the reference corpus is a pattern nothing
    // is testing at scale.
    assert!(
        totals.patterns.len() >= 50,
        "only {} patterns fired",
        totals.patterns.len()
    );
}
