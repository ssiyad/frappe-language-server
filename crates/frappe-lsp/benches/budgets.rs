//! The budgets of [31](../../../docs/specs/31-performance.md), measured.
//!
//! Against the reference bench — `FRAPPE_LSP_REFERENCE_BENCH`, else
//! `~/sources/frappe-dev` — when there is one, and the fixture bench otherwise,
//! so `cargo bench` runs anywhere and means most where a real bench exists.
//! Each group is named after its budget row; criterion keeps the history in
//! `target/criterion` and reports a change against the last run.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::Duration;

use criterion::{Criterion, criterion_group, criterion_main};
use frappe_index::Index;
use frappe_lsp::config::Config;
use frappe_lsp::{analysis, capabilities::ClientSupport, completion, diagnostics, hover, symbols};
use frappe_syntax::{Buffer, Language, PositionEncoding};

/// A bench, and a file in it with the positions the request benchmarks ask
/// about: a DocType name, and a fieldname to complete.
struct Target {
    bench: PathBuf,
    file: PathBuf,
    doctype: (&'static str, usize),
    field: (&'static str, usize),
    /// A DocType JSON to re-index.
    json: PathBuf,
}

fn target() -> Target {
    let reference = std::env::var_os("FRAPPE_LSP_REFERENCE_BENCH")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("sources/frappe-dev"))
        })
        .filter(|bench| bench.join("sites").is_dir())
        .filter(|bench| {
            bench
                .join("apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.py")
                .is_file()
        });
    if let Some(bench) = reference {
        eprintln!("benchmarking the reference bench at {}", bench.display());
        return Target {
            file: bench.join("apps/erpnext/erpnext/selling/doctype/sales_order/sales_order.py"),
            doctype: ("frappe.get_doc(\"Quotation\"", 17),
            field: ("\"Sales Order Item\", item.name, \"supplier\"", 33),
            json: bench.join("apps/frappe/frappe/core/doctype/user/user.json"),
            bench,
        };
    }
    let bench = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .join("tests/fixtures/bench");
    eprintln!("no reference bench; benchmarking the fixture bench");
    Target {
        file: bench.join("apps/library/library/api.py"),
        doctype: ("frappe.get_doc(\"Library Member\"", 17),
        field: ("\"Library Member\", member, \"loyalty_points\"", 28),
        json: bench.join("apps/library/library/library/doctype/library_member/library_member.json"),
        bench,
    }
}

fn budgets(c: &mut Criterion) {
    let target = target();
    let mut index = Index::build(&target.bench);
    index.scan_occurrences();
    let source = std::fs::read_to_string(&target.file).expect("the target file");
    let buffer =
        Buffer::open(Language::Python, &source, 1, PositionEncoding::Utf16).expect("parses");
    let at =
        |(needle, offset): (&str, usize)| source.find(needle).expect("needle in the file") + offset;
    let context = analysis::context_for(&index, Some(&target.file));
    let support = ClientSupport::default();
    let config = Config::default();

    let mut slow = c.benchmark_group("31");
    slow.sample_size(10)
        .measurement_time(Duration::from_secs(20));
    slow.bench_function("cold index, no cache", |b| {
        b.iter(|| black_box(Index::build(&target.bench)))
    });
    let cache = std::env::temp_dir().join(format!("frappe-lsp-bench-{}", std::process::id()));
    frappe_index::cache::save(&index, &target.bench, &cache).expect("cache written");
    slow.bench_function("warm start from cache", |b| {
        b.iter(|| black_box(frappe_index::cache::load(&target.bench, &cache).expect("cache read")));
    });
    slow.finish();
    let _ = std::fs::remove_dir_all(&cache);

    let mut fast = c.benchmark_group("31");
    // Its own copy: the requests below hold the index borrowed.
    let mut patched = index.clone();
    fast.bench_function("incremental re-index, one DocType JSON", |b| {
        b.iter(|| black_box(patched.reindex(&target.json)));
    });
    let doctype = at(target.doctype);
    fast.bench_function("hover", |b| {
        b.iter(|| {
            let found = analysis::at(&index, &buffer, doctype, context).expect("a DocType");
            black_box(hover::markdown(&index, &found))
        });
    });
    fast.bench_function("definition", |b| {
        b.iter(|| {
            let found = analysis::at(&index, &buffer, doctype, context).expect("a DocType");
            black_box(frappe_lsp::definition::targets(&index, &found))
        });
    });
    let field = at(target.field);
    fast.bench_function("completion, resolved anchor", |b| {
        b.iter(|| {
            let found = analysis::at(&index, &buffer, field, context).expect("a field");
            black_box(completion::complete(
                &index, &buffer, &found, field, support, 1000,
            ))
        });
    });
    fast.bench_function("references, workspace-wide", |b| {
        b.iter(|| {
            let found = analysis::at(&index, &buffer, doctype, context).expect("a DocType");
            black_box(symbols::references(
                &index,
                &found,
                true,
                PositionEncoding::Utf16,
            ))
        });
    });
    fast.bench_function("diagnostics after didChange", |b| {
        b.iter(|| {
            black_box(diagnostics::for_buffer(
                &index,
                &config,
                &target.file,
                &source,
                PositionEncoding::Utf16,
            ))
        });
    });
    fast.finish();
}

criterion_group!(benches, budgets);
criterion_main!(benches);
