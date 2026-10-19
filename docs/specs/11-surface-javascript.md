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

## When a handler fires

A key in a `frappe.ui.form.on` block is live if any of these holds. A form
missed here is an `unknown-form-event` on a handler that works, which is the
failure [23](23-diagnostics.md) forbids; the corpus check in
[32](32-testing.md) found 630 of them before these rules existed.

| Form | Example |
| --- | --- |
| Lifecycle event | `refresh`, `validate`, `onload_post_render` |
| Fieldname | fires when that field changes |
| Grid row event | `<table_fieldname>` + `_add`, `_remove`, `_delete`, `_move`, `_on_form_rendered`, and `before_<f>_remove` |
| Called by name | `frm.trigger("x")`, `frm.script_manager.trigger("x")`, `frm.events.x(frm)` |

Grid row events are registered against the **child** DocType, with the
*parent's* table fieldname: `grid.js` fires
`script_manager.trigger(fieldname + "_add", row.doctype, row.name)`, so
`items_add` written under `frappe.ui.form.on("Sales Order Item", …)` is the
documented shape even though `items` is a field of `Sales Order`.

Handlers called by name are collected from every `.js` under an app package,
workspace-wide: erpnext triggers handlers across files, and a name that
something calls is a real handler wherever it was written.

The legacy three-argument form `frappe.ui.form.on(D, event, fn)` takes an
**event**, not a fieldname — the same namespace as an object key. Reading it as
a fieldname reported `("Payment Request", "refresh", …)` as a missing field.

`locals[":Company"]` is frappe's own notation for a Single's cached record; the
DocType is what follows the colon.

`frm.doc` is the **parent** document even inside a child DocType's handler
block, because `frm` is always the parent form. Which parent is a schema
question: a child held by one DocType's `Table` field resolves `frm` to that
DocType, and a child held by several resolves `frm` to nothing. Resolving
against the child itself reported `frm.refresh_field("items")` in erpnext's
`Purchase Invoice Item` block as a field missing from the row.

A file bound by `doctype_js` anchors `frm` to the DocType the hook names, for
the whole file. A bound script need not contain a `frappe.ui.form.on` block at
all — a helper taking `frm` is ordinary — and the hook is then the only record
of which form it is for ([12](12-surface-hooks.md)). The binding is a fallback:
any `frappe.ui.form.on` block in the same file wins inside its own body, since
the innermost scope decides.
