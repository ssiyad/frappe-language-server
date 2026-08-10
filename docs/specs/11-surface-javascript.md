# 11 — JavaScript surface

Client scripts: `<x>.js`, `<x>_list.js`, files referenced by `doctype_js`, and
app bundles. Conventions as in [10](10-surface-python.md).

## Form scripts

```js
frappe.ui.form.on("Task", {
    refresh(frm) {},
    subject(frm) {},
});
```

- Argument 1 is `D`. It may name a **child** DocType, registering handlers for
  rows of a table on the parent form — resolve against the child.
- Object keys are events, drawn from two namespaces that share one keyspace:

| Event group | Names |
| --- | --- |
| Lifecycle | `setup`, `onload`, `before_load`, `refresh`, `onload_post_render`, `form_render`, `render_complete`, `validate`, `before_save`, `after_save`, `before_submit`, `on_submit`, `before_cancel`, `after_cancel`, `before_discard`, `after_discard`, `before_workflow_action`, `after_workflow_action`, `timeline_refresh`, `dashboard_update`, `on_hide`, `on_tab_change`, `rename`, `dirty` |
| Per-field | Any `fieldname` — fires on change |
| Per-table | `<table_fieldname>_add`, `<table_fieldname>_remove`, `before_<table_fieldname>_remove` |

Lifecycle names are vendored from `frappe/public/js/frappe/form/` and pinned per
[31](31-performance.md). A key matching neither group is a diagnostic — this
catches the classic typo'd fieldname handler that silently never fires.

The generator takes the union of `script_manager.trigger("…")` and the form
object's own `this.trigger("…")` sites, minus control-level events. Neither
call site alone is the list: scraping only `script_manager` drops `setup`,
`rename`, `dirty`, `dashboard_update`, `render_complete` and `timeline_refresh`,
while scraping every `.trigger(` picks up `click`, `focus` and `input` from
controls. Measured at frappe `5a94ce8`: `attachments_change` appears nowhere in
the tree and `on_hide` does (`form.js:101`).

`frappe.ui.form.on(D, fieldname, handler)` is a legacy 3-argument form; argument
2 is `F`.

## Form object

`frm` (and `cur_frm`) is anchored to the enclosing `frappe.ui.form.on` DocType.

| Pattern | Positions |
| --- | --- |
| `frm.doc.<attr>` | `F` |
| `frm.set_value(F, v)`, `frm.set_value({F: v})` | |
| `frm.get_field(F)`, `frm.refresh_field(F)`, `frm.get_docfield(F)` | |
| `frm.set_df_property(F, prop, v)` | `prop` completes from the field-property set |
| `frm.toggle_display(F, c)`, `toggle_enable`, `toggle_reqd` | `F` or `[F, …]` |
| `frm.set_query(F, fn)` | `F` on the form |
| `frm.set_query(child_F, table_F, fn)` | 3-arg form: `table_F` is on the form, `child_F` on its child DocType |
| `frm.add_child(F, {…})` | dict keys are child fields |
| `frm.add_custom_button(...)`, `frm.set_intro(...)` | no positions; hover only |
| `frm.call({ method, args })` | controller method on the anchored DocType |

`locals[D][name]` and `frappe.model.get_value(D, name, F)` follow the same rules.

## Client APIs

| Pattern | Positions |
| --- | --- |
| `frappe.db.get_value(D, filters, F)`, `set_value(D, name, F, v)` | |
| `frappe.db.get_list(D, {fields, filters})`, `get_doc(D, name)`, `exists(D, …)`, `count(D, …)`, `delete_doc(D, name)` | kwargs as [10](10-surface-python.md) |
| `frappe.new_doc(D)`, `frappe.set_route("Form", D, name)` | |
| `frappe.listview_settings[D] = {…}` | list view config |
| `frappe.call({ method: path, args })` | `Whitelisted` path — [07](07-dotted-paths.md) |
| `frappe.xcall(path, args)` | same |
| `__("…")` | translation — [23](23-diagnostics.md) |

## `frappe.call` validation

The highest-value check on this surface: a `method` string that resolves to a
Python function **without** `@frappe.whitelist()` is a runtime `PermissionError`.
Report it. A path that does not resolve at all is a separate, weaker diagnostic —
it may live in an app outside the workspace.

## Excluded

Vue/React sources under `<app>/public/js/**` that never touch `frm` or
`frappe.ui.form` are indexed for `frappe.call` paths only. No framework-aware
analysis; `ts-ls` and `volar` own those files.
