# 005 — DocType index

**Phase** 2 · **Depends on** 004 · **Blocks** 006, 007, 008

The metadata graph from [04](../specs/04-metadata-index.md).

## Scope

- Entity types; parse DocType JSON into them, retaining field ranges for
  [21](../specs/21-navigation.md).
- Implicit fields (standard, child, optional) — completable, never "unknown".
- Field `options` semantics per fieldtype, including `Dynamic Link` sibling
  resolution and `fetch_from` splitting.
- Layer 1 and Layer 2 merge, with `provenance` on every entity.
- Conflict detection: duplicate DocType across apps, Custom Field colliding with
  a schema field.
- Controller discovery and method extraction.
- Whitelisted method registry ([07](../specs/07-dotted-paths.md)).
- Dotted path resolver, all five kinds.
- Parallel scan with `rayon`; string interning ([31](../specs/31-performance.md)).
- Incremental single-file re-index.
- `frappe-lsp/indexStatus`.

## Done when

Indexes `frappe` + `erpnext` inside the cold-index budget, and an incremental
re-index produces a graph byte-identical to a full rebuild (property test,
[32](../specs/32-testing.md)).
