# Tasks

Open work only. Delete a file when its task is done.

## Phase 2 — Index and navigation

| # | Task | Depends on |
| --- | --- | --- |
| 003 | [Fixture bench](003-fixture-bench.md) | — |
| 004 | [Workspace discovery](004-workspace-discovery.md) | 003 |
| 005 | [DocType index](005-doctype-index.md) | 004 |
| 006 | [Syntax extraction](006-syntax-extraction.md) | 003 |
| 007 | [Python surface](007-python-surface.md) | 005, 006 |
| 008 | [Definition, hover, completion](008-definition-hover-completion.md) | 007 |
| 009 | [Index cache and file watching](009-index-cache.md) | 008 |
| 010 | [Hooks surface](010-hooks-surface.md) | 009 |

## Phase 3 — JavaScript, references, diagnostics

| # | Task | Depends on |
| --- | --- | --- |
| 011 | [JavaScript surface](011-javascript-surface.md) | 010 |
| 013 | [References and symbols](013-references-symbols.md) | 011 |
| 012 | [Diagnostics](012-diagnostics.md) | 011 |
| 018 | [Test running](018-test-runner.md) | 004 · independent |

## Phase 4 — Bridge and templates

| # | Task | Depends on |
| --- | --- | --- |
| 014 | [Live bridge](014-live-bridge.md) | 012 |
| 015 | [Jinja surface](015-jinja-surface.md) | 014 |
| 017 | [Type checker integration](017-type-integration.md) | 009 · experimental |

## Phase 5 — Distribution and upstream

| # | Task | Depends on |
| --- | --- | --- |
| 016 | [Distribution](016-distribution.md) | 014 |
| 019 | [Upstream into Frappe](019-upstream.md) | 016 |

## Critical path

`003 → 004 → 005 → 007 → 008` — 006 runs parallel to 004/005.

`010` (hooks) sits in phase 2 rather than 3: dotted-path navigation is a large
share of the grepping this project exists to remove, and hooks feed the index
that later surfaces depend on.

`018` needs only 004 and can be pulled forward whenever it is wanted. Phases in
[roadmap](../roadmap.md).
