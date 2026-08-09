# Documentation guidelines

Rules for everything under `docs/`. This file follows them.

## Audience

Readers know Python, JavaScript, LSP and Frappe. Do not explain what a DocType,
a hook or a language server is. Link instead of explaining:
[Hooks](https://docs.frappe.io/framework/user/en/basics/hooks).

Explain only what is specific to this project, or genuinely non-obvious.

## Layout

| Path | Holds |
| --- | --- |
| `docs/*.md` | Project level — index, roadmap, development, guidelines |
| `docs/specs/` | Normative design, numbered |
| `docs/tasks/` | Open work only |

`specs/index.md` and `tasks/index.md` are tables of contents. Nothing else lives
in them.

## Specs

Numbers are stable and are how specs cite each other:

| Range | Kind |
| --- | --- |
| 00–09 | Foundations |
| 10–19 | Surfaces |
| 20–29 | Features |
| 30–39 | Operations |

- One concern per file. A title needing "and" is two files.
- Composable: state contracts, not implementation. Another spec must be able to
  cite yours without inheriting your internals.
- Cross-reference as `[04](04-metadata-index.md)` — number and link together.
  Never "see the metadata index spec".
- MUST / SHOULD / MAY are normative. Use them only where a real constraint
  exists; prose elsewhere.
- State non-goals. What is deliberately excluded is as load-bearing as what is
  included, and it is what stops scope drifting back in.
- Record rejected alternatives with the reason. A decision without its trade-off
  gets relitigated in six months.
- Target 40–90 lines. Longer usually means it should be split.

## Tasks

Header carries phase, dependencies and what it blocks. Body is **Scope** then
**Done when**.

- "Done when" must be checkable by someone else. "Works well" is not a
  criterion; "288 DocTypes from 316 candidate files" is.
- Cite the specs the task implements. Do not restate them — the spec is the
  source of truth and duplication is how the two diverge.
- Delete the file when the task is done. Finished work lives in git history.
- An open question is a task. Resolving it means editing the spec and deleting
  the task, not annotating it.

## Style

- Tables for anything enumerable. Prose only for reasoning.
- Code blocks for shapes, signatures and layouts. Show rather than describe.
- No preamble. The first line after a heading is content.
- Say a thing once. Everywhere else, link to it.
- Code comments explain why, never what.

## Evidence

- Ground claims in something checkable and name the source.
- Distinguish measured from estimated. "32.5 s measured" and "~1.2 s
  interpolated" are different claims and must not read alike.
- Prefer numbers to adjectives: "41 of 41 DocTypes duplicated" beats "many
  conflicts".
- When a real workspace contradicts a spec, the spec is wrong. Fix it, and
  record what caught it so the case becomes a test.

## Maintenance

- Spec first: a behaviour change is a spec diff in the same PR as the code.
- A spec that lags the code is worse than no spec, because the next reader
  trusts it.
- Adding a file means editing the index that lists it. Neither index may have
  gaps.
