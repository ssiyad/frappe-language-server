//! Every `expect:` annotation in the fixture bench, checked against what the
//! server actually reports — [32](../../../docs/specs/32-testing.md).
//!
//! The bench carries its expectations inline: `# expect: unknown-field` on the
//! line that must produce one, `# expect: clean` on the near miss that must
//! not. Spec 32 requires both directions for every check, and this is what
//! makes the annotations load-bearing rather than decorative — a check that
//! stops firing, or starts firing on the line beside it, fails here.

use std::path::{Path, PathBuf};

use frappe_index::Index;
use frappe_lsp::config::Config;
use frappe_lsp::diagnostics;

fn fixture_bench() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("tests/fixtures/bench")
}

/// `expect: <code>` or `expect: clean`, and the line it sits on.
fn annotations(source: &str) -> Vec<(u32, String)> {
    source
        .lines()
        .enumerate()
        .filter_map(|(number, line)| {
            let rest = line.split("expect:").nth(1)?.trim_start();
            // `expect: clean, a fieldname handler` and `expect: unknown-hook
            // (why)` both carry prose for the reader; the code is the run of
            // kebab-case that starts the annotation.
            let code: String = rest
                .chars()
                .take_while(|c| c.is_ascii_lowercase() || *c == '-')
                .collect();
            (!code.is_empty()).then(|| {
                (
                    u32::try_from(number).expect("a fixture under 4 billion lines"),
                    code,
                )
            })
        })
        .collect()
}

/// Every source file in the bench that carries an annotation.
fn annotated(bench: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = frappe_index::walk::Walk::new(bench.join("apps"))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "py" || extension == "js")
        })
        .filter(|path| std::fs::read_to_string(path).is_ok_and(|source| source.contains("expect:")))
        .collect();
    files.sort();
    files
}

#[test]
fn every_expectation_in_the_bench_holds() {
    let bench = fixture_bench();
    let index = Index::build(&bench);
    // The annotations are written against the shipping defaults. A check that
    // ships off is still expected to fire when it is asked for, so those lines
    // are run against a config that turns it on — anything else would leave
    // the off-by-default checks with no positive case at all.
    let config = Config::default();
    let mut requested = Config::default();
    requested.diagnostics.unknown_attribute = frappe_lsp::config::Level::Warning;
    const OFF_BY_DEFAULT: [&str; 1] = ["unknown-attribute"];

    // Three codes in the registry have no implementation yet: the extraction
    // arm each needs is the open half of task 012. They are named here rather
    // than deleted from the fixture, so the day one lands its expectation is
    // already written ([23](../../../docs/specs/23-diagnostics.md)).
    const PENDING: [&str; 3] = [
        "not-single-doctype",
        "parent-field-on-child",
        "translation-dynamic",
    ];

    let files = annotated(&bench);
    assert!(files.len() > 3, "only {} annotated files", files.len());

    let mut wrong = Vec::new();
    for path in files {
        let source = std::fs::read_to_string(&path).expect("readable");
        let shipped = diagnostics::for_buffer(&index, &config, &path, &source);
        let asked = diagnostics::for_buffer(&index, &requested, &path, &source);
        let relative = path.strip_prefix(&bench).unwrap_or(&path).display();

        for (line, expected) in annotations(&source) {
            if PENDING.contains(&expected.as_str()) {
                continue;
            }
            let against = if OFF_BY_DEFAULT.contains(&expected.as_str()) {
                &asked
            } else {
                &shipped
            };
            let reported: Vec<String> = against
                .iter()
                .filter(|diagnostic| diagnostic.range.start.line == line)
                .filter_map(|diagnostic| match diagnostic.code.as_ref()? {
                    tower_lsp_server::ls_types::NumberOrString::String(code) => {
                        Some(code.trim_start_matches("frappe-lsp/").to_string())
                    }
                    tower_lsp_server::ls_types::NumberOrString::Number(_) => None,
                })
                .collect();

            let held = if expected == "clean" {
                reported.is_empty()
            } else {
                reported.iter().any(|code| code == &expected)
            };
            if !held {
                wrong.push(format!(
                    "{relative}:{} expected {expected}, got {reported:?}",
                    line + 1
                ));
            }
        }
    }

    assert!(
        wrong.is_empty(),
        "{} unmet:\n  {}",
        wrong.len(),
        wrong.join("\n  ")
    );
}
