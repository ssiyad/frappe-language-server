# 007 — Python surface

**Phase** 2 · **Depends on** — · **Blocks** 008

Queries for every pattern in [10](../specs/10-surface-python.md), plus anchor
resolution joining extraction to the index.

## Scope

- DocType-position patterns: the `frappe.*` and `frappe.db.*` tables.
- Fieldname positions: `fields`, `filters` in all three shapes, `pluck`,
  `order_by`, `group_by`, `set_value` dict form.
- The 4-tuple filter form re-anchoring to its own DocType.
- Document methods on `self` and bound names; `append` binding dict keys to the
  child DocType.
- `@frappe.whitelist()` registration; per-file `from frappe import …` aliasing.
- Skip rules: aggregates, dotted and backticked field entries, `frappe.db.sql`.

## Done when

Golden extraction tests cover every row of [10](../specs/10-surface-python.md),
each with a negative case that resolves to `Unknown`. Run across `frappe` and
`erpnext`, the recognized-call count is stable and no pattern panics.
