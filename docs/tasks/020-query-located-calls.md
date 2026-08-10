# 020 — Query-located call sites

**Phase** 2 · **Depends on** — · **Blocks** nothing

Extraction matches with a cursor walk. [06](../specs/06-analysis-pipeline.md)
now specifies two layers, and the first one is missing.

## Scope

- `tree_sitter::Query` per grammar locating call sites, decorators and
  `frappe.ui.form.on` handlers — location only.
- The shape tables in `frappe-syntax` keep the argument reading; they are layer
  two and do not change.
- Node-kind names confined to the query files and `read.rs`.

## Done when

Extraction produces the same matches it does today — the existing tests are the
oracle — and the walk no longer inspects node kinds to find a call. Match counts
over the reference bench are unchanged: 20,003 matches, 25,001 slots.
