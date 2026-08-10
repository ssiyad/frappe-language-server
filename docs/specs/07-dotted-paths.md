# 07 — Dotted paths

`app.module.file.symbol` strings appear in hooks, patches, `frappe.call`,
`frappe.enqueue`, reports and fixtures. One resolver serves them all.

## Resolution

Given `a.b.c.d` and the app roots from [03](03-workspace-model.md):

1. Longest prefix matching an app package directory wins. The first segment is
   the **package directory** name, not `app_name` — on the fixture bench
   `circulation.api.before_loan` resolves and `circulation_app.api.before_loan`
   does not ([03](03-workspace-model.md)).
2. Walk remaining segments as directories while they exist.
3. The next segment resolves to `<segment>.py`, else `<segment>/__init__.py`.
4. Remaining segments are symbols inside that module: a top-level `def`/`class`,
   then an attribute of a class (`Class.method`).

Return every candidate rather than guessing — go-to-definition can present a
list. Unresolvable paths are reported per [23](23-diagnostics.md).

`frappe.*` paths resolve against the `frappe` app like any other; no special case.

## Kinds

Callers declare what they expect, so the resolver can validate more than
existence.

| Kind | Requires | Used by |
| --- | --- | --- |
| `Callable` | module-level `def` or `class` | most hooks |
| `Whitelisted` | `Callable` **and** `@frappe.whitelist()` | `frappe.call`, `override_whitelisted_methods` |
| `Module` | module, no symbol segment | `patches.txt`, `jinja.methods` |
| `Class` | `class` statement | `override_doctype_class`, `extend_doctype_class` |
| `Method` | `Class.method` on a resolved class | `doc_events` values may take this form |

## Whitelist detection

A function is whitelisted when decorated with `frappe.whitelist(...)` or a
`whitelist(...)` imported from `frappe`. Recorded per
[04](04-metadata-index.md): dotted path, `allow_guest`, `methods`, parameter
list, range.

Aliased imports (`from frappe import whitelist as w`) are recognized within a
file. Decorator factories behind further indirection are not — they yield
"unknown", never "not whitelisted", so no false diagnostic.

`override_whitelisted_methods` in `hooks.py` remaps a path; the index records the
override so `frappe.call` navigation lands on the effective target and hover
mentions the original.

## Patches

`patches.txt` is line-oriented, `#` comments, with a `[post_model_sync]` section
marker. Each non-empty line is a `Module` path, optionally
`path #comment` or `execute:python_code`.

- `execute:` lines are opaque — no resolution, no diagnostic.
- Module paths resolve for go-to-definition and MUST expose a `execute()`.
- A patch listed twice, or missing on disk, is a diagnostic.

## Non-Python targets

`doctype_js`, `app_include_js`, `base_template` and friends hold **file** paths,
not dotted paths. They resolve differently:

| Form | Resolves to |
| --- | --- |
| `public/js/x.js` | `<package>/public/js/x.js` ([03](03-workspace-model.md)) |
| `/assets/<app>/js/x.js` | same, via the `assets` symlink |
| `<app>/public/js/x.bundle.js` | bundle entry; `.bundle.js` may be generated |

Generated bundle outputs under `sites/assets/` MUST NOT be indexed as sources —
navigation targets the app source.
