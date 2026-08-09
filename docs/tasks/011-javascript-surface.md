# 011 — JavaScript surface

**Phase** 3 · **Depends on** 010

[11](../specs/11-surface-javascript.md).

## Scope

- `frappe.ui.form.on` — DocType anchor including child-DocType registration; the
  3-argument legacy form.
- Event keyspace: vendored lifecycle names, fieldnames, `<table>_add` /
  `_remove` / `before_<table>_remove`.
- `frm` / `cur_frm` anchoring, including files bound via `doctype_js`.
- `frm.*` patterns; `set_query` 2- and 3-argument forms; `add_child` dict keys on
  the child DocType.
- Client APIs: `frappe.db.*`, `frappe.call` / `xcall`, `frappe.new_doc`,
  `set_route`, `listview_settings`, `locals`.
- Completion, hover, definition on all of the above.

## Done when

Form event completion offers lifecycle names and fieldnames on a real DocType,
and `frappe.call` method strings navigate to their Python definitions across
`frappe` and `erpnext`.
