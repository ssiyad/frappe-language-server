# Roadmap

Phases are gated on usefulness, not on feature count. Each ships something a
developer would keep enabled.

The organising goal is removing grep. Frappe resolves its own structure at
runtime, so the questions that cost the most time — where is this DocType, who
handles this event, what does this hook point at — have no answer in the editor
today. Navigation therefore leads, and everything else follows from the index
built to support it.

## Phase 1 — Design ✅

Specs and task breakdown. Complete.

## Phase 2 — Index and navigation ✅

Workspace discovery, DocType index, Python surface, hooks surface, go-to-
definition, hover, completion, cache. Static layers only; no diagnostics.

Diagnostics are deliberately absent. Resolution must be right against real benches
before anything can accuse correct code of being wrong
([23](specs/23-diagnostics.md)).

**Done when** go-to-definition works on the reference bench within the budgets in
[31](specs/31-performance.md). Met: on `frappe` 17.0.0-dev plus two custom apps,
definition lands on the controller in 3 ms against a 10 ms target, hover and
completion in about 1 ms, cold index 155 ms and warm start 37 ms. With `erpnext`
added — 863 DocTypes, 4 650 modules — cold index 744 ms and warm start 133 ms,
both inside the budgets. Driven from Neovim, not only from tests.

The JavaScript surface arrived here rather than in phase 3: extraction covered
both languages from the start, so turning it on was resolution and presentation
rather than new machinery.

## Phase 3 — References, diagnostics, test running ✅

Workspace references, symbols, the diagnostic set, quick fixes. Test running
([25](specs/25-test-runner.md)) rides along — it depends only on workspace
discovery and can be pulled forward.

Diagnostics are the risk the whole phase turns on. Everything shipped so far can
only be absent or wrong where the user asked a question; a diagnostic speaks
unprompted about code somebody else wrote, and spec 23's precision rule is what
keeps that welcome.

Diagnostics publish today, with the check set split by what can be known: a
name decided by a vendored table or a file that does not parse is reported; a
name the index merely cannot find is not, until absence is knowable enough to
be quiet on correct code.

The absence checks are on, `doc.x` excepted. `cargo xtask smoke` measures this
on every run rather than by hand: over `frappe` and `erpnext` — 7 935 files,
templates included — the shipping defaults produce 197 findings, every one of them
checked against the source and recorded with a reason in
`tests/smoke-allowlist.toml`. Anything else fails the job.

Diagnostics are **done**: every code in [23](specs/23-diagnostics.md) is
implemented, each with a positive and a negative case in the fixture bench,
both checked by `tests/annotations.rs` against the real server.

Test running is **done**: code lenses on every runnable scope, resolved through
the workspace class graph so erpnext's `ERPNextTestSuite` subclasses are found
— 929 of 953 test files on the reference bench, 9 532 lenses. Execution is the
client's, through a `frappe-lsp.runTest` command carrying argv; the server-run
fallback is opt-in and logs rather than diagnosing
([25](specs/25-test-runner.md)).

The hooks surface is **done**: `doctype_js` binds a form script as a
navigation target and anchors `frm` inside it, and `hooks.py` completes hook
keys, doc events, frequencies and DocType map keys
([12](specs/12-surface-hooks.md)).

References and symbols are **done**. The reverse occurrence table holds 89 523
sites across `frappe` and `erpnext`, built after the index publishes and cached
with it; `references` on `Item` returns 821 hits from Python, JavaScript, other
DocTypes' `options`, `hooks.py` and fixtures in 18.7 ms
([21](specs/21-navigation.md)).

**Done when** that check stays clean ([32](specs/32-testing.md)) with every
check carrying both a positive and a negative case. Met.

## Phase 4 — Bridge and templates ✅

Live site layer, Custom Field and Property Setter resolution, in-DB scripts as
virtual documents, Jinja surface. Type checker integration
([24](specs/24-type-integration.md)) rides along as an opt-in experiment — it
depends only on phase 2 and blocks nothing.

**Done when** a field added through Customize Form completes without a restart.

The bridge reads. Custom Field and Property Setter from a live site merge as
Layer 3 and evict without a rebuild, confidence moves `Static` → `Full` →
`Degraded`, and a down site suppresses the absence checks instead of flooding
them ([05](specs/05-live-bridge.md)). Credentials come from the environment or
from any command that prints them, so no particular credential tool is a
dependency. DocTypes created through the UI join the graph from their meta,
and the site's Client and Server Scripts are written to read-only files that
definition lands on. Templates are read as Jinja: includes navigate, `frappe.*`
calls and `doc` in a print format resolve and complete, the globals complete,
and the checks run on them. With `types.stub` on, a generated `document.pyi`
makes pyright type `frappe.get_doc("Sales Invoice", …)` as `SalesInvoice`
([24](specs/24-type-integration.md)). Phase 4 is done.

### Hardening

Before the rest of phase 4, the server was driven the way an editor drives it.
`cargo xtask stress` opens every file of `frappe` and `erpnext`, asks about it
and types into it: 634 346 requests, with no crash, hang, error response or
panic. It also sends what a careless client sends, and CI runs it over the
fixture bench. `tests/editors/neovim.lua` checks every feature from stock
Neovim. What that found, and what changed:

- The occurrence scan held the index's write lock for seconds after every
  build, freezing every request. It now scans a snapshot.
- A panic in one request ended the session. It now answers that request with
  an error.
- Every range was in UTF-16 while Neovim negotiates UTF-8, so non-ASCII text
  moved diagnostics and jumps.
- Completion found nothing on a line still being typed, which is when it is
  asked. `references` answered "none" until its table existed.
- A warm start holds 188 MB on an eight-app bench. A cold start holds about
  650 MB, and the remedies measured cost more start time than they saved
  ([31](specs/31-performance.md)).

## Phase 5 — Distribution and upstream

Release binaries, editor setup docs, packaging. Then the upstream conversation
([019](tasks/019-upstream.md)) — deliberately last, so it is argued from a
working server and measured evidence rather than a proposal.

Distribution is **done**. Each release target builds on its own platform, and
the archive a user downloads is unpacked, checked against its checksum and
driven through `tests/editors/neovim.lua` before anything is published. 0.2.0
shipped that way on Linux and macOS, x86_64 and aarch64. The Neovim, pi and
Claude Code plugins download it by themselves through `bin/frappe-lsp`, so
each one installs with one command and completion works on the reference
bench with no build step. The VS Code setup was checked by hand. What is left is the upstream
conversation, which waits on real use.

## Deferred

Field rename across a workspace, DocType scaffolding, semantic tokens, inlay
hints, test result trees, a debug adapter. Revisit only after phase 4 and only
with a concrete request behind it.
