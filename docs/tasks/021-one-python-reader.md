# 021 — One Python reader

**Phase** 2 · **Depends on** — · **Blocks** 010

`frappe-index` reads Python with a hand-rolled scanner because the dependency
edge to `frappe-syntax` was thought to be forbidden.
[02](../specs/02-architecture.md) now allows it, and two readers that disagree
about what a decorator is will disagree as a diagnostic on correct code.

## Scope

- `frappe-index` depends on `frappe-syntax`.
- Controller class and method extraction, and whitelist detection with its
  tri-state ([07](../specs/07-dotted-paths.md)), move onto the tree-sitter
  reader.
- Delete `frappe-index/src/python.rs`.
- Keep the parallel scan: a parser per rayon thread, not one shared.

## Done when

The reference bench yields the same registry it does today — 331 controllers,
528 whitelisted methods — with the cold index still inside the budget in
[31](../specs/31-performance.md), and the aliased-import and
decorator-indirection cases in [07](../specs/07-dotted-paths.md) keep their
`Unknown` answers.
