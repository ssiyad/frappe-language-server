# Tasks

Open work only. Delete a file when its task is done.

Phases 1 and 2 are complete: discovery, the metadata index, extraction,
resolution, the cache and watcher, and definition / hover / completion on the
Python and JavaScript surfaces and inside `hooks.py`. Phases in
[roadmap](../roadmap.md).

## Phase 3 — References, diagnostics, test running

**Complete.** Diagnostics (`012`), references and symbols (`013`), the hooks
surface (`010`) and test running (`018`) are all done and their files deleted.

## Phase 4 — Bridge and templates

| # | Task | Depends on |
| --- | --- | --- |
| 017 | [Type checker integration](017-type-integration.md) | — · experimental |

## Phase 5 — Distribution and upstream

| # | Task | Depends on |
| --- | --- | --- |
| 016 | [Distribution](016-distribution.md) | — |
| 019 | [Upstream into Frappe](019-upstream.md) | 016 |

## Critical path

The live bridge (`014`) is done and its file deleted: Custom Field, Property
Setter, UI-created DocTypes and the site's scripts all reach the index. The
Jinja surface (`015`) is done too: grammar, includes, expressions, `doc` in
print formats, globals and diagnostics. Phase 4 has `017` left, which is
independent and experimental.

What phase 3 left behind:

- `012`'s one open item — `Degraded` confidence — landed with `014`, the task
  that gives that state a way to occur.
- `018` shipped the client-run path; the Neovim glue for
  `frappe-lsp.runTest` is documented by [016](016-distribution.md).

## Carried forward

Gaps that outlive the tasks that created them, recorded here so they are not
rediscovered:

- **The vendored tables are still written by hand.** `Document` members, the
  hook registry and form events cannot be generated whole: a hook's schema is a
  judgement. Drift is caught instead — `cargo xtask codegen --check` fails on
  any name frappe reads that a table lacks, and CI runs it weekly against
  frappe's `develop`. Its first run found 23 hooks and four events missing,
  and the `Document` member check, added after, six attributes.
- **No benchmarks.** `cargo bench` has nothing to run; the budgets in
  [31](../specs/31-performance.md) are asserted by tests that skip without a
  bench, and `cargo xtask stress` reports latencies without tracking them over
  time.
- **CI has no real bench.** CI drives the fixture bench through
  `cargo xtask stress` on every push, but `cargo xtask smoke` and the full
  stress run over `frappe` and `erpnext` still need a developer's bench, so the
  allowlist is only as current as the last local run.
