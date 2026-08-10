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

Phases 1 and 2 complete. Go-to-definition, hover and completion answer on the
Python and JavaScript surfaces and inside `hooks.py`; the index caches, follows
the filesystem, and is measured against a real bench and driven from Neovim.

Phase 3 is under way. Diagnostics publish: the checks decided by a vendored
table or a parse failure are on, and the two absence checks ship off until they
can be quiet on correct code. `cargo xtask smoke` measures this over `frappe`
and `erpnext` on every run — 7 079 files, 54 findings, each one checked against
the source and allowlisted with a reason. References and document symbols are
next.

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
