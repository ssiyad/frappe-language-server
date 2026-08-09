# Tasks

Open work only. Delete a file when its task is done.

## Phase 1 — Design

| # | Task | Depends on |
| --- | --- | --- |
| 001 | [Resolve open design questions](001-resolve-open-questions.md) | — |

## Phase 2 — Index and read-only features

| # | Task | Depends on |
| --- | --- | --- |
| 002 | [Repository bootstrap](002-repo-bootstrap.md) | 001 |
| 003 | [Fixture bench](003-fixture-bench.md) | 002 |
| 004 | [Workspace discovery](004-workspace-discovery.md) | 003 |
| 005 | [DocType index](005-doctype-index.md) | 004 |
| 006 | [Syntax extraction](006-syntax-extraction.md) | 003 |
| 007 | [Python surface](007-python-surface.md) | 005, 006 |
| 008 | [Completion, hover, definition](008-completion-hover-definition.md) | 007 |
| 009 | [Index cache and file watching](009-index-cache.md) | 008 |

## Phase 3 — Hooks, JavaScript, diagnostics

| # | Task | Depends on |
| --- | --- | --- |
| 010 | [Hooks surface](010-hooks-surface.md) | 009 |
| 011 | [JavaScript surface](011-javascript-surface.md) | 010 |
| 012 | [Diagnostics](012-diagnostics.md) | 011 |
| 013 | [References and symbols](013-references-symbols.md) | 011 |

## Phase 4 — Bridge and templates

| # | Task | Depends on |
| --- | --- | --- |
| 014 | [Live bridge](014-live-bridge.md) | 012 |
| 015 | [Jinja surface](015-jinja-surface.md) | 014 |
| 017 | [Type checker integration](017-type-integration.md) | 009 · experimental |

## Phase 5 — Distribution

| # | Task | Depends on |
| --- | --- | --- |
| 016 | [Distribution](016-distribution.md) | 014 |

## Critical path

`001 → 002 → 003 → 004 → 005 → 007 → 008` — 006 runs parallel to 004/005.
Phases in [roadmap](../roadmap.md).
