# 013 — References and symbols

**Phase** 3 · **Depends on** —

[21](../specs/21-navigation.md). The reverse index is the only structural work
left in phase 3; everything else here is presentation over the graph that
exists.

## Scope

- Reverse occurrence index (name → sites) built during the scan, invalidated per
  file. Memory cost counts against [31](../specs/31-performance.md).
- `textDocument/references` across all surfaces; field references scoped to their
  DocType; `Unknown` anchors excluded.
- `documentSymbol` for DocType JSON with Section/Column/Tab Break nesting, plus
  `hooks.py` and `patches.txt`. Nothing for controller `.py` / `.js`.
- `workspaceSymbol` over DocTypes, whitelisted methods, reports, pages, web
  forms. Fuzzy, capped at 200. Fields excluded.

`typeDefinition` and `implementation` already answer, from 008 and the hooks
surface respectively.

## Done when

References on a widely used DocType (`Item`) returns hits from Python, JS, other
DocTypes' `options`, hooks and fixtures in one result set, within budget.
