---
name: frappe-lsp
description: Use the LSP tool with frappe-lsp when working on Frappe or ERPNext code inside a bench — DocType fields, controllers, hooks.py, frappe.get_doc / frappe.db.get_value calls, form scripts, Jinja templates. Triggers - "which fields does X have", "where is X used", "add a field to", editing a controller, hooks.py, a form script or a print format.
---

# frappe-lsp

Frappe connects its parts with strings: `frappe.get_doc("Sales Invoice")`,
`frm.set_value("customer")`, a dotted path in `hooks.py`. Grep finds the words;
it cannot tell `customer` on Sales Invoice from `customer` on Sales Order, and
it cannot say whether a field exists at all. frappe-lsp indexes the bench, and
the LSP tool asks it. It works on any file inside a bench (a directory with
`sites/apps.txt` above it), beside the Python and JavaScript servers.

## Before writing code that names fields

- Hover on a DocType name in a string, such as `"Sales Invoice"` in a
  `frappe.get_doc` call: module, app, Single or child table, field count.
- Go to definition on it lands on the controller class, then the DocType JSON.
- List the symbols of that DocType JSON: every field, with its type, grouped
  by section. That is the schema after Custom Fields and Property Setters. Use
  those fieldnames exactly.

## Finding where something is used

- Find references on a DocType or fieldname string returns every call, Link,
  `fetch_from`, hook, fixture and template that names it — only that DocType's
  field, not every field of the same name.
- Workspace symbol search finds DocTypes, whitelisted methods, reports and
  pages by part of their name.
- Go to definition on a dotted path in `hooks.py` or `frappe.call` lands on the
  function; on `{% include "…" %}` in a template, on the template.

Prefer these over grep for DocTypes, fields and hook paths. Use grep for text.

## After editing

frappe-lsp's findings arrive with every edit to a file in the bench: an
unknown DocType or field, a form handler that never fires, a hook or patch that
resolves nowhere, a concatenated translation string. Fix what it reports before
moving on; it does not report on correct code.

## When the LSP tool says the server is missing

Tell the user. frappe-lsp is a single binary from
https://github.com/ssiyad/frappe-language-server/releases, and Claude Code
needs it on the `PATH` of the shell `claude` starts from.
