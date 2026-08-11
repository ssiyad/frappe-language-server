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

## Phase 3 — References, diagnostics

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
on every run rather than by hand: over `frappe` and `erpnext` — 7 079 files,
890 DocTypes — the shipping defaults produce 215 findings, every one of them
checked against the source and recorded with a reason in
`tests/smoke-allowlist.toml`. Anything else fails the job.

Diagnostics are **done**: every code in [23](specs/23-diagnostics.md) is
implemented, each with a positive and a negative case in the fixture bench,
both checked by `tests/annotations.rs` against the real server.

References and symbols are **done**. The reverse occurrence table holds 89 523
sites across `frappe` and `erpnext`, built after the index publishes and cached
with it; `references` on `Item` returns 821 hits from Python, JavaScript, other
DocTypes' `options`, `hooks.py` and fixtures in 18.7 ms
([21](specs/21-navigation.md)).

**Done when** that check stays clean ([32](specs/32-testing.md)) with every
check carrying both a positive and a negative case. Met.

## Phase 4 — Bridge and templates

Live site layer, Custom Field and Property Setter resolution, in-DB scripts as
virtual documents, Jinja surface. Type checker integration
([24](specs/24-type-integration.md)) rides along as an opt-in experiment — it
depends only on phase 2 and blocks nothing.

**Done when** a field added through Customize Form completes without a restart.

## Phase 5 — Distribution and upstream

Release binaries, editor setup docs, packaging. Then the upstream conversation
([019](tasks/019-upstream.md)) — deliberately last, so it is argued from a
working server and measured evidence rather than a proposal.

## Deferred

Field rename across a workspace, DocType scaffolding, semantic tokens, inlay
hints, test result trees, a debug adapter. Revisit only after phase 4 and only
with a concrete request behind it.
