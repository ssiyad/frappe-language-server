# 10 — Python surface

Call shapes recognized in `.py`. Extraction rules in [06](06-analysis-pipeline.md).

`D` marks a DocType-name position, `F` a fieldname position resolved against the
call's DocType anchor. Positions only count when the argument is a string
literal; anything else yields `Anchor::Unknown`.

## DocType positions

| Pattern | Notes |
| --- | --- |
| `frappe.get_doc(D, name)` | Also `frappe.get_doc({"doctype": D, F: ...})` — dict keys are `F` |
| `frappe.get_cached_doc(D, name)`, `frappe.new_doc(D)`, `frappe.get_last_doc(D)` | Bind a local per [06](06-analysis-pipeline.md) |
| `frappe.get_all(D, …)`, `frappe.get_list(D, …)` | Kwargs below |
| `frappe.db.get_all(D, …)`, `frappe.db.get_list(D, …)` | |
| `frappe.db.get_value(D, filters, F)`, `frappe.db.get_values(...)` | 3rd arg may be `F` or `list[F]` |
| `frappe.db.set_value(D, name, F, value)` | 3rd arg may be `{F: value}` |
| `frappe.db.exists(D, …)`, `frappe.db.count(D, …)`, `frappe.db.delete(D, …)` | |
| `frappe.db.get_single_value(D, F)`, `frappe.db.set_single_value(D, F, v)` | Single DocTypes only |
| `frappe.get_value(D, …)`, `frappe.get_cached_value(D, name, F)` | |
| `frappe.get_meta(D)`, `frappe.get_single(D)` | |
| `frappe.delete_doc(D, name)`, `frappe.rename_doc(D, old, new)` | |
| `frappe.has_permission(D, …)` | |
| `frappe.qb.DocType(D)` | Builder root; field access on the result is `F` |

`frappe.get_all` and `frappe.db.get_all` differ only in permission handling —
identical for our purposes.

## Fieldname positions

Resolved against the anchor of the enclosing call:

| Kwarg | Shape |
| --- | --- |
| `fields` | `[F, …]`, `"F"`, or `["count(name) as n"]` — aggregate strings are skipped |
| `filters`, `or_filters` | `{F: v}`, `[[F, op, v], …]`, `[[D, F, op, v], …]` — the 4-tuple form re-anchors to its own `D` |
| `pluck` | `F` |
| `order_by` | `"F asc"`, `"F desc"`, comma-separated |
| `group_by` | `F` |

Dotted (`"customer.name"`), backticked (`` "`tabItem`.name" ``) and
function-wrapped entries are skipped rather than reported. Skipped means **no
position at all**, not an unresolved one: a position that exists is a position
some later check can decide to report on.

A `(D, …)` row reads argument 0 and the kwargs named above. Positional arguments
past the first are not read — the signatures differ across the family, and a
wrong guess invents fieldname positions rather than missing them.

## Document methods

Applied to `self` in a controller, or to a name bound per
[06](06-analysis-pipeline.md):

| Pattern | Positions |
| --- | --- |
| `doc.get(F)`, `doc.set(F, v)`, `doc.get_value(F)` | `F` on the bound DocType |
| `doc.append(F, {…})` | `F` is a `Table` field; dict keys are fields of the **child** DocType |
| `doc.db_set(F, v)` | also `{F: v}` |
| `doc.get_formatted(F)`, `doc.get_doc_before_save()` | |
| `doc.<attr>` | Completion and hover from the bound DocType's fields |

## Definitions and paths

| Pattern | Effect |
| --- | --- |
| `@frappe.whitelist(...)` | Registers a whitelisted method — [07](07-dotted-paths.md) |
| `frappe.call(path, …)`, `frappe.get_attr(path)` | `Whitelisted` / `Callable` path |
| `frappe.enqueue(path, …)`, `frappe.enqueue_doc(D, name, method)` | `Callable`; `method` is a controller method |
| `frappe.get_hooks("key")` | Hook key — [12](12-surface-hooks.md) |

## Deliberately excluded

- `frappe.db.sql(...)` — raw SQL. `` `tabX` `` table names MAY be offered as
  navigation in a later phase; no fieldname analysis, no diagnostics.
- `frappe.db.multisql`, dynamic `getattr` dispatch, `**kwargs` forwarding.
- Anything requiring cross-function inference.

## Aliasing

`import frappe` is the overwhelming convention and is what the queries match.
`from frappe import get_doc` is recognized per-file for the top-level helpers.
`import frappe as f` is **not** recognized — rare enough that supporting it is
not worth the query complexity. Unrecognized means silent, never wrong.
