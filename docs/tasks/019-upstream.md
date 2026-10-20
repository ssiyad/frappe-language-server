# 019 — Upstream into Frappe

**Phase** 5 · **Depends on** 016 (done) · Deliberately last

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

## Licensing

frappe-lsp is GPLv3; `frappe/frappe` is MIT. The server itself is unaffected —
GPLv3 is well precedented in this ecosystem, since `frappe/erpnext` is GPLv3 too,
and living under the `frappe` org does not require matching `frappe/frappe`.

What it does affect is this task specifically. The candidates above are changes
*inside* `frappe/frappe`, and GPLv3 source cannot be merged into an MIT project.
Each one must therefore be contributed as an independent MIT patch authored
against `frappe`, not lifted from this repository. That is a normal thing to do —
the ideas and the measurements transfer freely, only the text does not — but it
means "upstream it" is a rewrite, not a copy. Budget for that.

## Done when

Each candidate is either an upstream issue/PR, or recorded as declined with the
reason. Anything declined stays working in frappe-lsp — upstreaming is the
preferred home, never the only one.
