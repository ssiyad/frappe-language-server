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
- **Debounce.** Watcher events coalesce at 250 ms per path; diagnostics recompute
  at most every 150 ms per buffer.
- **Cancellation.** Honour `$/cancelRequest`. A completion superseded by a newer
  keystroke MUST stop early.
- **No allocation in the hot path.** Completion filtering reuses a scratch buffer.

## Cache

`<bench>/.frappe-lsp/index.v<N>.bin`, `bincode` or `rkyv`. Invalidated by schema
version or any `(path, mtime, len)` mismatch; only changed files are re-parsed.
Corruption falls back to a full scan and MUST NOT surface an error. Add
`.frappe-lsp/` to `.gitignore` guidance in [development](../development.md).

## Vendored constants

Several specs depend on values copied out of `frappe`: fieldtype tables
([04](04-metadata-index.md)), the hook registry ([12](12-surface-hooks.md)), form
events ([11](11-surface-javascript.md)), doc events and scheduler frequencies
([12](12-surface-hooks.md)), Jinja globals ([13](13-surface-jinja.md)).

These drift. Mitigation:

1. Generate them with a script that reads a `frappe` checkout; commit the
   generated file with the source commit hash recorded.
2. A CI job re-runs generation against `develop` and fails on a diff, so drift is
   a visible failure rather than a silent wrong answer.
3. Unknown values from a *newer* Frappe degrade to "unknown", never to "invalid" —
   the diagnostics in [23](23-diagnostics.md) that assert invalidity MUST be
   derived from a version the workspace actually pins where that is detectable.

## Measurement

Criterion benchmarks for index build, completion, and reference lookup, over a
committed synthetic bench plus an optional real ERPNext clone in CI. Budget
regressions fail the build. Numbers above are targets to validate, not measured
results — no implementation exists yet.
