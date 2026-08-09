# 019 — Upstream into Frappe

**Phase** 5 · **Depends on** 016 · Deliberately last

The project is aimed at landing in Frappe. Nothing before this task changes
`frappe` itself — this is where that conversation starts, once there is a working
server to argue from.

## Candidates

| Change | Spec | Why upstream |
| --- | --- | --- |
| Scoped `Literal` overloads for `get_doc` & co. | [24](../specs/24-type-integration.md) | `bench` already generates `DF` annotations; overloads belong beside them. Fixes CI and every editor, not just ours. |
| `__getattr__` on `BaseDocument` | [24](../specs/24-type-integration.md) | Silences the `Cannot access attribute` errors at zero cost — at the price of all field checking. Maintainer trade, not ours. |
| Vendored constants as exported data | [31](../specs/31-performance.md) | Hook registry, doc events, form events and fieldtypes are re-derived by every tool. Exporting them kills the drift problem at the source. |
| DocType JSON schema | [04](../specs/04-metadata-index.md) | A published schema would let any editor validate DocType files. |

## Preconditions

- Server is in real use across several apps ([roadmap](../roadmap.md) phase 5).
- Measured evidence for each proposal — the overload cost table in
  [24](../specs/24-type-integration.md) is the model.
- MIT throughout, matching `frappe/frappe`.

## Done when

Each candidate is either an upstream issue/PR, or recorded as declined with the
reason. Anything declined stays working in frappe-lsp — upstreaming is the
preferred home, never the only one.
