---
name: frappe-lsp
description: Use frappe-lsp when working on Frappe or ERPNext code inside a bench — DocType fields, controllers, hooks.py, frappe.get_doc / frappe.db.get_value / frappe.get_all calls, form scripts, Jinja templates. Triggers - "which fields does X have", "where is X used", "rename a field", "add a field to", reviewing a diff, auditing for dead fields or broken hook paths, editing a controller, hooks.py, a form script or a print format.
allowed-tools: Bash(frappe-lsp *)
---

# frappe-lsp

Frappe connects its parts with strings: `frappe.get_doc("Sales Invoice")`,
`frm.set_value("customer")`, a dotted path in `hooks.py`. Grep finds the words;
it cannot tell `customer` on Sales Invoice from `customer` on Sales Order, and
it cannot say whether a field exists at all. frappe-lsp indexes the bench and
answers exactly. It works anywhere inside a bench (a directory with `apps/` and
`sites/`), or inside an app checkout that sits in one.

`frappe-lsp` is on the Bash `PATH`. Run it from any directory in the bench.

## Before writing code that names fields

```sh
frappe-lsp describe "Sales Invoice"
```

Prints every field with its type, link target and flags, the controller and the
DocType JSON: the schema after Custom Fields and Property Setters. Use those
fieldnames exactly. Do this instead of reading the JSON.

## Finding where something is used

```sh
frappe-lsp references "Site Update"              # the DocType
frappe-lsp references "Site Update" update_job   # one of its fields
```

Prints `path:line: text` for every call, filter, Link, `fetch_from`, hook,
fixture and template that names it, only for that DocType. An unknown DocType
or field is said as such, with exit 1. Start a rename here, then grep for what
the list says it does not cover: code outside the app package, such as a
frappe-ui dashboard's `fields` and `filters`, and attribute reads on a variable
not tied to the DocType.

## Reviewing or auditing code

```sh
frappe-lsp check press/api                       # a directory
frappe-lsp check press/api/site.py press/hooks.py
git diff --name-only --relative --diff-filter=d | xargs -r frappe-lsp check
```

Prints `path:line:column: severity: message [code]` for each problem: an
unknown DocType or field, a hook or patch path that resolves nowhere, a form
handler that never fires, a translation string that is never translated. Exit
1 means findings, 0 means none. It reports only what is certainly wrong
against the code on disk; an `unknown-field` can still be a Custom Field that
exists only in a site's database, so say so when you report one.

Never edit a file to make diagnostics appear. `check` answers for any file.

## After editing

Findings for a file you edit arrive with the edit's result. Fix them before
moving on. Run `frappe-lsp check` on files you changed without the Edit tool.

## In the editor

The LSP tool also works on these files: hover on a DocType string, go to
definition on a dotted path in `hooks.py` or `frappe.call`, find references
from a DocType or fieldname string, or from a field in a DocType JSON.

## When frappe-lsp does not start

The plugin downloads the server from
https://github.com/ssiyad/frappe-language-server/releases on first use. Tell
the user the error: the usual causes are no network, or no `curl` or `wget`.
`FRAPPE_LSP` can name a binary they already have. Exit 2 with "no Frappe bench
or app above" means the current directory is outside a bench.
