---
name: frappe-lsp
description: Use frappe-lsp's tools when working on Frappe or ERPNext code inside a bench — DocType fields, controllers, hooks.py, frappe.get_doc / frappe.db.get_value / frappe.get_all calls, form scripts, Jinja templates. Triggers - "which fields does X have", "where is X used", "rename a field", "add a field to", reviewing a diff, auditing for dead fields or broken hook paths, editing a controller, hooks.py, a form script or a print format.
---

# frappe-lsp

Frappe connects its parts with strings: `frappe.get_doc("Sales Invoice")`,
`frm.set_value("customer")`, a dotted path in `hooks.py`. Grep finds the words;
it cannot tell `customer` on Sales Invoice from `customer` on Sales Order, and
it cannot say whether a field exists at all. frappe-lsp indexes the bench and
answers exactly.

The tools find the bench (a directory with `sites/apps.txt` above it) from the
working directory. When the session runs outside a bench, pass `path`: any
path inside it.

## Reviewing or auditing code

Call `frappe_check` with a directory, such as an app's `api/` folder, or with
each file a diff touches. It reports an unknown DocType or field, a hook or
patch path that resolves nowhere, a form handler that never fires, and a
translation string that is never translated. It reports only what is certainly
wrong against the code on disk, and grep cannot find any of it. An
`unknown-field` can still be a Custom Field that exists only in a site's
database, so say so when you report one.

Never edit a file to make findings appear. `frappe_check` answers for any file.

## Before writing code that names fields

Call `frappe_doctype` with the DocType name. It returns every field with its
type, label, link target and whether it is required, the controller class and
file, and the DocType JSON: the schema after Custom Fields and Property
Setters, not just the JSON on disk. Use it instead of reading the JSON, and use
the fieldnames it returns exactly.

## Finding where something is used

- `frappe_references` with a DocType, or a DocType and a fieldname, lists every
  call, filter, Link, `fetch_from`, hook, fixture and template that names it,
  only for that DocType.
- `frappe_find` finds DocTypes, whitelisted methods, reports and pages by part
  of their name, with their files.

Use `frappe_references` instead of grep for a DocType or a field. Each answer
ends with what it does not cover: code outside the app package, such as a
frappe-ui dashboard, and attribute reads on a variable not tied to the
DocType. Before a rename, grep those too.

## After editing

Every file you edit or write inside a bench is checked, and any problem is
appended to the edit's result. Fix what it reports before moving on; it says
nothing on correct code.

## When a tool says frappe-lsp did not start

The package downloads the server from
https://github.com/ssiyad/frappe-language-server/releases on first use. Tell
the user the error: the usual causes are no network, or no `curl` or `wget`.
`FRAPPE_LSP` can name a binary they already have.
