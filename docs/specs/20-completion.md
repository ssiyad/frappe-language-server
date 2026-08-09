# 20 — Completion

`textDocument/completion`. Candidates come from [04](04-metadata-index.md),
triggered at positions defined by [10](10-surface-python.md)–[13](13-surface-jinja.md).

## Triggers

Characters: `"`, `'`, `.`, `[`. Also invoked explicitly, and re-filtered
incrementally while typing inside a string.

Completion inside a string literal MUST replace the string **contents**, not the
quotes. `TextEdit` range is the literal's interior.

## Providers

| Context | Items |
| --- | --- |
| `D` position | DocType names |
| `F` position, anchor resolved | Fields of that DocType + implicit fields ([04](04-metadata-index.md)) |
| `doc.<attr>`, `frm.doc.<attr>` | Same, as members |
| `frappe.ui.form.on` object key | Lifecycle events + fieldnames + `<table>_add`/`_remove` ([11](11-surface-javascript.md)) |
| `hooks.py` dict key at module level | Hook names ([12](12-surface-hooks.md)) |
| `doc_events` inner key | Doc event names |
| `scheduler_events` key | Frequencies |
| `method:` / `frappe.call` | Whitelisted method paths |
| `patches.txt` line | Module paths under the current app |
| DocType JSON `"fieldtype"` | Fieldtype names |
| DocType JSON `"options"` where fieldtype is `Link`/`Table` | DocType names |
| DocType JSON `"fieldname"` in `depends_on`, `fetch_from`, `field_order` | Sibling fieldnames |

## Ranking

Editors sort by `sortText`; the index is large enough that ordering decides
usability. Descending priority:

1. Exact prefix match on the typed text.
2. Same app as the current file, then same module.
3. For `D`: non-child DocTypes before child DocTypes — child DocTypes are rarely
   the intended argument to `get_doc`.
4. For `F`: `in_list_view` and `reqd` fields first, then declared `field_order`,
   then implicit fields last.
5. Alphabetical.

`filterText` MUST be the raw name so editor fuzzy matching works on
`"sales invoice"` and `"salinv"` alike.

## Item shape

| Field | Content |
| --- | --- |
| `label` | DocType name / fieldname, unmodified |
| `kind` | `Class` for DocType, `Field` for field, `Function` for method, `Property` for hook key |
| `detail` | DocType: `<module> · <app>`. Field: `<fieldtype>` and, for `Link`, `→ <target>` |
| `documentation` | Markdown, same body as hover ([22](22-hover.md)) |
| `labelDetails` | Preferred over `detail` where the client supports it |

Resolve `documentation` lazily via `completionItem/resolve` — building markdown
for 2,000 DocTypes per keystroke blows the budget in [31](31-performance.md).

## Volume

A full ERPNext bench has ~2,500 DocTypes and some DocTypes exceed 200 fields.

- Return everything and set `isIncomplete: false` when the candidate set is
  filtered by a non-empty prefix.
- With an empty prefix, cap at 1,000 items and set `isIncomplete: true` so the
  client re-queries as the user types.

## Non-goals

No snippet completion for DocType boilerplate, no auto-import, no postfix
templates. Those belong in editor snippets, not in the index.
