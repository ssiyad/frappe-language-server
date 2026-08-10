# frappe-lsp

Language server for [Frappe Framework](https://docs.frappe.io/framework) apps.

Frappe resolves its own structure at runtime. `frappe.get_doc("Sales Invoice")`,
`doc_events`, `frappe.ui.form.on("Task")` — the relationships are all strings, so
the editor cannot follow them and the answer is grep. frappe-lsp indexes the
bench and turns those strings into links.

## Contents

- [Roadmap](roadmap.md) — phases and milestones
- [Development](development.md) — toolchain, workflow, conventions
- [Guidelines](guidelines.md) — how these docs are written
- [Specifications](specs/index.md) — normative design
- [Tasks](tasks/index.md) — pending and in-progress work

## Status

Phase 1 (design) complete. Phase 2 done: go-to-definition, hover and completion
answer on the Python and JavaScript surfaces and inside `hooks.py`, the index
caches and follows the filesystem, and it is all measured against a real bench
and driven from Neovim. Phase 3 continues with references, symbols and
diagnostics.

## Decisions

| Decision | Choice |
| --- | --- |
| Language | Rust — `tower-lsp-server` + `tree-sitter` |
| Index source | Static bench files, optional live site bridge |
| Editor target | Generic stdio LSP (Neovim first) |
| Scope | Frappe-specific semantics; defers to pyright/ruff/ts-ls |
| Priority | Navigation first — grep is the problem being solved |
| Frappe version | Branch-agnostic; detected, never assumed |
| License | GPLv3, as `erpnext` — see [019](tasks/019-upstream.md) for the upstream consequence |
| Upstream | Intended, but not before phase 5 ([019](tasks/019-upstream.md)) |

## Reference bench

`~/sources/frappe-dev` — `frappe` 17.0.0-dev, plus custom apps. Used to validate
discovery rules; several specs cite it directly.
