# 31 — Performance

## Reference workload

A bench with `frappe` + `erpnext` + `hrms` + `payments`: roughly 2,500 DocTypes,
100k fields, 40k Python/JS files. Budgets are stated against this, on a 2020-era
4-core laptop. Anything that holds here holds on a single-app bench.

## Budgets

| Operation | Target | Ceiling |
| --- | --- | --- |
| Cold index, no cache | 2 s | 5 s |
| Warm start from cache | 200 ms | 500 ms |
| Incremental re-index, one DocType JSON | 5 ms | 20 ms |
| Completion, resolved anchor | 20 ms | 50 ms |
| Hover | 10 ms | 30 ms |
| Definition | 10 ms | 30 ms |
| References, workspace-wide | 100 ms | 300 ms |
| Diagnostics after `didChange` | 100 ms | 250 ms |
| Resident memory | 200 MB | 400 MB |

The server MUST NOT block `initialize` on the index build. It responds
immediately and serves at `Partial` confidence ([23](23-diagnostics.md)) until
the build completes.

## Techniques

- **Parallel scan.** DocType JSON parsing is embarrassingly parallel; `rayon`
  over the file list. Merge into the graph single-threaded afterwards.
- **String interning.** DocType names and fieldnames repeat across 100k
  occurrences. Intern to `u32` symbols; store names once.
- **Lazy tree-sitter.** Only open buffers hold trees. Closed files contribute
  through the index and the reverse-occurrence table.
- **Memory is a cold-start cost.** A warm start from the cache parses nothing
  and holds 188 MB on the eight-app reference bench. A cold start holds about
  650 MB for a live index of about 60 MB: tree-sitter allocates through glibc,
  which keeps an arena per parsing thread and seldom returns freed pages. The
  cache makes the cold path rare, so it is accepted. Two remedies were measured
  and rejected. `MALLOC_ARENA_MAX=2` saved 180 MB but made the cold build 5.5 s
  instead of 1.4 s and the occurrence scan 11.7 s instead of 3.7 s, because 14
  threads then share two locks. Capping the parse at 4 threads held 348 MB at
  2.9 s and 7.6 s. `malloc_trim` would release the pages, but it needs
  `unsafe`, which the workspace forbids.
- **Deferred occurrence scan.** The reverse table costs a full extract pass and
  is built after the index is published, not inside the cold budget
  ([21](21-navigation.md)). It is cached with the index, so the cost is paid
  once per bench rather than once per session.
- **Debounce.** Watcher events coalesce at 250 ms per path; diagnostics recompute
  at most every 150 ms per buffer. Access events are ignored: the index reads
  every file it indexes, so acting on a read makes indexing trigger indexing.
- **Batch the re-index.** One write-lock acquisition per debounce batch, not
  per file. A `git checkout` readies hundreds of paths at once, and taking the
  lock for each starves readers for the whole sweep rather than once at its end.
- **Watch the packages, not the bench.** A recursive watch on a bench root also
  covers `sites/`, `node_modules` and `.git`, and establishing it costs about a
  second of startup for directories the index never reads. `sites/` itself is
  watched flat, for `apps.txt` and `apps.json` and nothing below them.
- **Cancellation.** Honour `$/cancelRequest`. A completion superseded by a newer
  keystroke MUST stop early.
- **No allocation in the hot path.** Completion filtering reuses a scratch buffer.

## Cache

`<bench>/.frappe-lsp/index.v<N>.bin`, `bincode`. Invalidated by schema version,
by a different bench root, or by any `(path, mtime, len)` mismatch; only changed
files are re-parsed, and past half the file set a rebuild is cheaper than a
patch. `sites/apps.txt`, `sites/apps.json` and every `modules.txt` are stamped
separately and invalidate the cache **whole**: they decide which apps exist and
in what order, and install order decides the merge, so no per-file patch can
express a change to one. Corruption falls back to a full scan and MUST NOT surface an error.

Discovery is re-run on load, before the stamps are compared. A file created
while the server was off has no stamp to fail *and* is absent from the artifact
lists the cache carries, so a cache that trusts what it stored serves an index
of a bench that no longer exists — silently, and for as long as that file is
never edited again. Discovery is the cheap half of a build (65–95 ms against
744 ms on `frappe` + `erpnext`); skipping it buys a faster start at the price of
a wrong answer.

Written *after* the index is published, never before: encoding a few MiB and
stamping every file read takes longer than the build, and doing it first would
hold back the answer the cache exists to deliver sooner.

Measured on `frappe` 17.0.0-dev plus `erpnext` and two custom apps — 863
DocTypes, 14 303 fields, 4 650 Python modules: **cold start 744 ms, warm start
133 ms** (medians of 5, release). With the reverse occurrence table
([21](21-navigation.md)) the cache grows to 18 MiB and warm start to 197 ms —
still inside the 500 ms ceiling, and the alternative is a 1.9 s scan on every
session. On `frappe` plus the two custom apps alone —
331 DocTypes, 2 055 files, a 2.2 MiB cache — cold 155 ms and warm 37 ms. Both
are inside the budgets above. Add `.frappe-lsp/` to `.gitignore` guidance in
[development](../development.md).

## Vendored constants

Several specs depend on values copied out of `frappe`: fieldtype tables and the
`Document` member list ([04](04-metadata-index.md)), the hook registry
([12](12-surface-hooks.md)), form events ([11](11-surface-javascript.md)), doc
events and scheduler frequencies ([12](12-surface-hooks.md)), Jinja globals
([13](13-surface-jinja.md)).

These drift, and the server is **branch-agnostic**: it must behave on v15, on
`develop`, and on whatever the workspace happens to be running.

### Version detection

Two sources, both on disk, neither requiring a running site:

| Source | Yields |
| --- | --- |
| `<frappe package>/__init__.py` → `__version__` | e.g. `17.0.0-dev` |
| `sites/apps.json` → `resolution.branch`, `commit_hash`, `version` | per app, incl. non-`frappe` apps |

### Policy

1. Generate constants with a script reading a `frappe` checkout; commit each
   snapshot tagged with the version and commit it came from.
2. CI re-runs generation against `develop` and fails on a diff, so drift is a
   visible failure rather than a silent wrong answer.
3. **Unknown never becomes invalid unless the detected version matches a
   vendored snapshot exactly.** On an exact match, a value outside the snapshot
   is genuinely wrong and [23](23-diagnostics.md) may report it. On any other
   version — a fork, an unreleased branch, a commit between snapshots — an
   unrecognized value degrades to "unknown" and reports nothing.

This is what branch-agnostic costs: on an unpinned bench, checks that assert
invalidity from a vendored list get quieter. That is the correct trade. A server
that calls a valid new hook key an error on the day it lands upstream is worse
than one that stays silent.

## Measurement

`cargo bench -p frappe-lsp` runs one criterion benchmark per budget row,
against the reference bench when there is one and the fixture bench otherwise;
criterion keeps the history and reports a change against the last run. On the
eight-app reference bench, release build:

| Operation | Target | Ceiling | Measured |
| --- | --- | --- | --- |
| Cold index, no cache | 2 s | 5 s | 1.70 s |
| Warm start from cache | 200 ms | 500 ms | 290–440 ms |
| Incremental re-index, one DocType JSON | 5 ms | 20 ms | 10 ms |
| Completion, resolved anchor | 20 ms | 50 ms | 5.2 ms |
| Hover | 10 ms | 30 ms | 6.5 ms |
| Definition | 10 ms | 30 ms | 3.8 ms |
| References, workspace-wide | 100 ms | 300 ms | 4.2 ms |
| Diagnostics after `didChange` | 100 ms | 250 ms | 12 ms |

The request rows measure the server's own work on `sales_order.py`; the stress
drive of [32](32-testing.md) measures them as an editor sees them, protocol
included. Warm start is over its target and inside its ceiling.

The re-index row is measured with the reference table built, as the server
holds it, and the first run found a save costing 65 ms: it rebuilt the schema
rows of every DocType linking to the saved one, and nearly every DocType links
to `User`. Those rows only change when a DocType appears or goes. Files naming a
DocType are re-read only when its shape changes — a field added, removed or
retyped, the DocType created, deleted or made a child — which a re-save of the
same schema never does.

Nothing fails the build on a regression yet: CI has no real bench, and timings
on a shared runner would fail on the runner rather than on the code.
