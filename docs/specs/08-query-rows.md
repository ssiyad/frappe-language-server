# 08 — Query rows

Binding the rows a query returns, so a read such as `row.status` is a
reference to the field. Extends the binding of [06](06-analysis-pipeline.md),
which stops at documents, under the same bounds.

## Why

`frappe.get_all` returns rows, not documents, and code reads them as
attributes:

```python
updates = frappe.get_all("Site Update", fields=["status", "update_job"])
update = updates[0]
if update.status == "Recovered":
    job = update.update_job
```

Without a binding, neither read is a reference. On press, these reads were
most of what references to Site Update's `update_job` and `status` missed,
and a rename that trusts the list breaks them.

## Rules

Within one function body, as in [06](06-analysis-pipeline.md):

| Written | Binds the name to |
| --- | --- |
| `x = frappe.get_all(D, …)`, also `get_list`, `db.get_all`, `db.get_list` | Rows of `D` |
| `x = frappe.db.get_values(D, …, as_dict=True)` | Rows of `D` |
| `x = frappe.db.get_value(D, …, as_dict=True)`, also `frappe.get_value` | A row of `D` |
| `y = x[<integer>]`, `x` rows of `D` | A row of `D` |
| `for y in x:`, `x` rows of `D` | A row of `D`, in the loop body |

- `D` MUST be a literal, as for a constructor.
- `as_dict` MUST be a literal `True` or `1`. A call with `pluck` or a true
  `as_list` returns values or tuples, not rows, and binds nothing.
- `x = x[0]` rebinds `x` to a row: the right side reads the binding the
  statement replaces. It is the one reassignment that does not drop a binding.
- `x[<integer>].f` reads a row without a name.
- Any other reassignment drops the binding, as in [06](06-analysis-pipeline.md).

`row.f` is a reference to field `f` of `D`. Hover and go-to-definition treat
it as they treat `doc.f`.

## Not a finding

A read on a row MUST NOT produce a diagnostic, not even `unknown-attribute`
when that is on. A row holds what the query selected, under the names it
selected: `"name as site"`, `"count(*) as total"`, `"items.item_code"`. A
field the DocType lacks is often a correct alias, and checking the read against
the `fields` list is a dataflow problem this spec does not take on.

## Non-goals

- `row["f"]` and `row.get("f")`. Both are common, but a string key on a dict is
  read the same way as any other dict, and nothing marks this one as a row.
- Rows passed to or returned from another function, comprehensions, and rows
  from `frappe.qb` or `frappe.db.sql`.

## Rejected

Binding a row as a document, so the existing attribute checks applied: every
alias in a `fields` list would become a false `unknown-field`, which is what
[23](23-diagnostics.md) exists to prevent.
