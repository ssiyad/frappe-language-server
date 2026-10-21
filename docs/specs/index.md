# Specifications

Normative design for frappe-lsp. Numbered for stable cross-referencing.

## Foundations

| # | Spec | Covers |
| --- | --- | --- |
| 00 | [Overview](00-overview.md) | Problem, goals, non-goals, constraints |
| 01 | [Glossary](01-glossary.md) | Frappe and project terminology |
| 02 | [Architecture](02-architecture.md) | Process model, crates, concurrency, failure policy |
| 03 | [Workspace model](03-workspace-model.md) | Bench/app detection, artifact discovery |
| 04 | [Metadata index](04-metadata-index.md) | Entities, field semantics, layer merge, cache |
| 05 | [Live bridge](05-live-bridge.md) | Optional site connection, credentials, degradation |
| 06 | [Analysis pipeline](06-analysis-pipeline.md) | Grammars, recognition limits, local inference |
| 07 | [Dotted paths](07-dotted-paths.md) | Path resolution, whitelist detection, patches |

## Surfaces

| # | Spec | Covers |
| --- | --- | --- |
| 10 | [Python](10-surface-python.md) | Recognized call shapes in `.py` |
| 11 | [JavaScript](11-surface-javascript.md) | Form scripts, `frm`, client APIs |
| 12 | [Hooks](12-surface-hooks.md) | `hooks.py` keys and value schemas |
| 13 | [Jinja](13-surface-jinja.md) | Templates — deferred to phase 4 |

## Features

| # | Spec | Covers |
| --- | --- | --- |
| 20 | [Completion](20-completion.md) | Providers, ranking, item shape |
| 21 | [Navigation](21-navigation.md) | Definition, references, symbols |
| 22 | [Hover](22-hover.md) | Content per entity kind |
| 23 | [Diagnostics](23-diagnostics.md) | Checks, confidence gating, quick fixes |
| 24 | [Type checker integration](24-type-integration.md) | Generated overloads for pyright — experimental |
| 25 | [Test running](25-test-runner.md) | Code lenses over `bench run-tests` |
| 26 | [Command line](26-command-line.md) | `check`, `describe`, `references` for agents and scripts |

## Operations

| # | Spec | Covers |
| --- | --- | --- |
| 30 | [Configuration](30-configuration.md) | Settings, capabilities, custom methods |
| 31 | [Performance](31-performance.md) | Budgets, techniques, vendored constants |
| 32 | [Testing](32-testing.md) | Levels, fixtures, golden tests, invariants |

## Conventions

- RFC 2119 keywords are normative.
- Cross-reference by number and link: `[04](04-metadata-index.md)`.
- A spec describes intent and contracts, not implementation detail that the code
  states more precisely.
