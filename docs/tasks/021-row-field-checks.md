# 021 — Checks on query rows

**Phase** 6 · **Depends on** — · **Blocks** nothing

[08](../specs/08-query-rows.md) makes a read on a query row a reference and
never a finding, because a row holds what the query selected, aliases
included. A read of a field the query did not select is still a real bug, and
`row.x` where `x` is neither a field nor a selected name is often a typo.

## Scope

- Carry the literal `fields` list of the query into the row binding, with each
  entry's alias (`"name as site"`, `"count(*) as total"`, `"items.item_code"`).
- Decide in [08](../specs/08-query-rows.md) which of these is a finding: a
  field of the DocType that the query did not select, or a name that is
  neither a field nor an alias. Only when `fields` is a literal list, and never
  for `fields=["*"]`.

## Done when

- Over `frappe`, `erpnext` and press, every new finding is a real bug, checked
  by hand, and the count is recorded in [08](../specs/08-query-rows.md).
- The fixture bench holds one of each, beside an alias read that stays clean.
