# 12 — Hooks surface

`<app>/hooks.py` wires an app to the framework entirely through string keys and
dotted paths. Nothing else validates it, and mistakes fail silently at runtime.
This is the highest-value surface per line of code.

Reference: [Hooks](https://docs.frappe.io/framework/user/en/basics/hooks).

## Extraction

Module-level assignments only, right-hand side a literal (`str`, `list`, `dict`,
or a nested combination). Non-literal values — comprehensions, conditionals,
concatenation, `.update()` calls — are indexed as "present, opaque": the key is
known to be set, but no value validation runs.

The hook registry is vendored (name → value schema), pinned per
[31](31-performance.md), with `frappe/hooks.py` as the canonical example.

### Hooks an app invents

The vendored table covers the hooks *frappe* reads. An app may invent its own:
`hooks.py` writes the key, the app's own code reads it back, and nothing
registers it anywhere. erpnext defines and consumes `invoice_doctypes`,
`period_closing_doctypes` and a dozen more this way.

A key is therefore known if it is in the vendored table **or** any app in the
workspace reads it. Reads are collected from every `.py` under an app package,
in each form frappe itself uses:

| Form | Example |
| --- | --- |
| Call argument | `frappe.get_hooks("invoice_doctypes")` |
| `.get()` on the result | `frappe.get_hooks().get("additional_timeline_content")` |
| Subscript on the result | `frappe.get_hooks()["boot_session"]` |
| Attribute through a local | `hooks = frappe.get_hooks()` … `hooks.boot_session` |

The last one is why the binding is followed rather than only the call matched:
frappe reads a good fraction of its own hooks off the `_dict` that `get_hooks`
returns. A form missed here is an `unknown-hook` on a hook that works, which is
the failure [23](23-diagnostics.md) forbids — and the first run of the corpus
check in [32](32-testing.md) found exactly 33 of them.

## Value schemas

| Schema | Validation |
| --- | --- |
| `Path(kind)` | Resolve per [07](07-dotted-paths.md) with the given kind |
| `File` | Resolve as an asset path per [07](07-dotted-paths.md) |
| `DocTypeName` | Must exist in the index |
| `Map<DocTypeName, …>` | Keys are DocTypes; `"*"` is legal for `doc_events` |
| `Str` | Opaque |

Representative bindings:

| Hook | Shape |
| --- | --- |
| `doc_events` | `Map<DocTypeName, Map<DocEvent, Path(Callable) \| [Path]>>` |
| `override_doctype_class` | `Map<DocTypeName, Path(Class)>` |
| `extend_doctype_class` | `Map<DocTypeName, [Path(Class)]>` |
| `doctype_js`, `doctype_list_js`, `doctype_tree_js`, `doctype_calendar_js` | `Map<DocTypeName, File \| [File]>` |
| `permission_query_conditions`, `has_permission` | `Map<DocTypeName, Path(Callable)>` |
| `override_whitelisted_methods` | `Map<Path(Whitelisted), Path(Callable)>` |
| `scheduler_events` | `Map<Frequency, [Path(Callable)]>`; `cron` is `Map<CronExpr, [Path]>` |
| `app_include_js`, `web_include_css`, … | `File \| [File]` |
| `fixtures` | `[DocTypeName \| {dt: DocTypeName, filters: …}]` |
| `jinja` | `{methods: [Path], filters: [Path]}` — [13](13-surface-jinja.md) |
| `website_route_rules` | `[{from_route, to_route}]` |
| `required_apps` | `[AppName]` |
| lifecycle (`after_install`, `before_migrate`, `on_login`, …) | `Path(Callable)` |

## Doc events

Values are the controller lifecycle methods:

`before_validate`, `validate`, `before_save`, `autoname`, `before_insert`,
`after_insert`, `before_naming`, `on_update`, `before_submit`, `on_submit`,
`before_cancel`, `on_cancel`, `on_update_after_submit`, `before_save_after_submit`,
`on_change`, `before_rename`, `after_rename`, `before_print`, `on_trash`,
`after_delete`, `before_update_after_submit`

Checks:

- Unknown event name → diagnostic. Frappe never warns about these; the handler is
  simply never called.
- `on_submit` and friends registered on a DocType with `is_submittable: 0` →
  diagnostic. The handler is dead code.
- `"*"` as the DocType key is valid and applies to every DocType.

## Scheduler events

Keys: `all`, `hourly`, `daily`, `weekly`, `monthly`, `yearly`, `annual`, their
`_long` variants, `hourly_maintenance`, `daily_maintenance`, and `cron`.

Under `cron`, keys are cron expressions — validate with the same 5-field syntax
`croniter` accepts. An invalid expression raises at scheduler sync time, far from
the edit.

## Cross-hook effects

Hooks feed the rest of the index rather than only being validated:

- `override_doctype_class` / `extend_doctype_class` → controller resolution
  ([04](04-metadata-index.md)).
- `override_whitelisted_methods` → `frappe.call` navigation targets
  ([07](07-dotted-paths.md)).
- `doctype_js` → binds a JS file to a DocType so [11](11-surface-javascript.md)
  can anchor `frm` outside the DocType's own directory.
- `jinja.methods` / `jinja.filters` → template completion.

Because of the last two, `hooks.py` MUST be indexed before any JS or template
file in the same app.

## App-level checks

- Duplicate assignment to the same hook key in one file — the second silently wins.
- `app_name` mismatching the package directory.
- `required_apps` naming an app absent from the workspace (warning only — it may
  legitimately be installed elsewhere).
