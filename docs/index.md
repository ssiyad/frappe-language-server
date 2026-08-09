# frappe-lsp

Language server for [Frappe Framework](https://docs.frappe.io/framework) apps.

Frappe's semantics live in string literals — `frappe.get_doc("Sales Invoice")`,
`fields=["customer"]`, `doc_events` keys, `frappe.ui.form.on("Task")`. General
language servers cannot see through them. frappe-lsp indexes the bench and makes
those strings navigable, completable, and checkable.

## Contents

- [Roadmap](roadmap.md) — phases and milestones
- [Development](development.md) — toolchain, workflow, conventions
- [Specifications](specs/index.md) — normative design
- [Tasks](tasks/index.md) — pending and in-progress work

## Status

Phase 1 (design). No implementation yet.

## Decisions

| Decision | Choice |
| --- | --- |
| Language | Rust — `tower-lsp` + `tree-sitter` |
| Index source | Static bench files, optional live site bridge |
| Editor target | Generic stdio LSP (Neovim first) |
| Scope | Frappe-specific semantics only; defers to pyright/ruff/ts-ls |
