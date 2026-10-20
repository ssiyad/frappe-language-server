---
name: frappe-lsp
description: Use frappe-lsp's tools when working on Frappe or ERPNext code inside a bench — DocType fields, controllers, hooks, frappe.get_doc / frappe.db.get_value calls, form scripts, Jinja templates. Triggers - "which fields does X have", "where is X used", "add a field to", "fix this DocType", editing a controller, hooks.py, a form script or a template.
---

# frappe-lsp

Frappe connects its parts with strings: `frappe.get_doc("Sales Invoice")`,
`frm.set_value("customer")`, a dotted path in `hooks.py`. Grep finds the words;
it cannot tell `customer` on Sales Invoice from `customer` on Sales Order, and
it cannot say whether a field exists at all. frappe-lsp indexes the bench and
answers those questions exactly.

The tools work in any directory inside a bench (a directory with
`sites/apps.txt` above it).

## Before writing code that names fields

Call `frappe_doctype` with the DocType name. It returns every field with its
type, label, link target and whether it is required, the controller class and
file, and the DocType JSON — the schema after Custom Fields and Property
Setters, not just the JSON on disk. Use it instead of reading the JSON by hand,
and use the fieldnames it returns exactly.

## Finding where something is used

- `frappe_references` with a DocType, or a DocType and a fieldname, lists every
  call, Link, `fetch_from`, hook, fixture and template that names it.
- `frappe_find` finds DocTypes, whitelisted methods, reports and pages by part
  of their name, with their files.

Prefer these over grep for DocTypes and fields. Use grep for plain text.

## After editing

Every file you edit or write inside a bench is checked, and any problem is
appended to the edit's result: an unknown DocType or field, a form handler that
never fires, a hook or patch that resolves nowhere, a concatenated translation
string. Fix what it reports before moving on; it does not report anything on
correct code. `frappe_check` checks a file you did not edit.

## When a tool says frappe-lsp did not start

The package downloads the server from
https://github.com/ssiyad/frappe-language-server/releases on first use. Tell
the user the error: the usual causes are no network, or no `curl` or `wget`.
`FRAPPE_LSP` can name a binary they already have.
