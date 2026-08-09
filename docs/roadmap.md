# Roadmap

Phases are gated on usefulness, not on feature count. Each ships something a
developer would keep enabled.

## Phase 1 — Design (current)

Specs and task breakdown. No code.

**Done when** [specs](specs/index.md) cover every surface and feature, and the
[task list](tasks/index.md) is ordered with dependencies stated.

## Phase 2 — Index and read-only features

Workspace discovery, DocType index, Python surface, completion, hover,
definition. Static layers only; no bridge, no diagnostics.

Diagnostics are deliberately last. Getting resolution right against real benches
must come before anything that can accuse correct code of being wrong
([23](specs/23-diagnostics.md)).

**Done when** completion and go-to-definition work on an ERPNext bench within the
budgets in [31](specs/31-performance.md).

## Phase 3 — Hooks, JavaScript, diagnostics

`hooks.py` validation, client-script surface, the diagnostic set, quick fixes,
references, workspace symbols.

**Done when** the nightly real-bench smoke test reports zero diagnostics across
`frappe` and `erpnext` ([32](specs/32-testing.md)).

## Phase 4 — Bridge and templates

Live site layer, Custom Field and Property Setter resolution, in-DB scripts as
virtual documents, Jinja surface. Type checker integration
([24](specs/24-type-integration.md)) rides along as an opt-in experiment — it
depends only on phase 2 and blocks nothing.

**Done when** a field added through Customize Form completes without a restart.

## Phase 5 — Distribution

Release binaries, editor setup docs beyond Neovim, packaging.

## Deferred

Field rename across a workspace, DocType scaffolding, semantic tokens, inlay
hints, a debug adapter. Revisit only after phase 4 and only with a concrete
request behind it.
