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

## Phase 2 — Index and navigation

Workspace discovery, DocType index, Python surface, hooks surface, go-to-
definition, hover, completion, cache. Static layers only; no diagnostics.

Diagnostics are deliberately absent. Resolution must be right against real benches
before anything can accuse correct code of being wrong
([23](specs/23-diagnostics.md)).

**Done when** go-to-definition works on the reference bench within the budgets in
[31](specs/31-performance.md).

## Phase 3 — JavaScript, references, diagnostics

Client-script surface, workspace references, symbols, the diagnostic set, quick
fixes. Test running ([25](specs/25-test-runner.md)) rides along — it depends only
on workspace discovery and can be pulled forward.

**Done when** the nightly real-bench smoke test reports zero diagnostics across
`frappe` and a custom app ([32](specs/32-testing.md)).

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
