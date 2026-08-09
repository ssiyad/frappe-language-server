# 13 — Jinja surface

Lowest priority of the four surfaces; deferred to phase 4. Specified now because
it constrains grammar choice in [02](02-architecture.md).

## Scope

Templates under `<app>/templates/`, `<app>/www/`, `<app>/**/templates/`, plus
Print Format HTML and Web Template blocks stored in DocType JSON fields.

## Grammar problem

No mature tree-sitter grammar covers HTML with embedded Jinja. Options:

| Option | Assessment |
| --- | --- |
| `tree-sitter-html` + regex for `{{ }}` / `{% %}` | Cheap, adequate for path and name extraction, no expression structure |
| Existing community Jinja grammars | Incomplete and unmaintained; risk outweighs benefit |
| Hand-written scanner for Jinja delimiters, HTML ignored | Most control; delimiters are simple and stable |

Leaning toward the third: we need statement heads and expression text, not a DOM.
Decide when phase 4 starts — recorded as a task, not settled here.

## Positions

| Pattern | Effect |
| --- | --- |
| `{% include "app/templates/x.html" %}` | File path → go-to-definition |
| `{% extends "…" %}`, `{% import … %}` | Same |
| `frappe.get_doc(D, …)`, `frappe.db.get_value(D, …, F)` | As [10](10-surface-python.md), inside expressions |
| `frappe.get_all(D, …)` | Same |
| `_("…")` | Translation |
| `{{ doc.<attr> }}` | `F`, when the template's DocType is known |

## Anchoring `doc`

`doc` is in scope for Print Formats and Web Forms, where the DocType is known
from the containing record. For `www/` pages and generic templates it is not —
those yield `Anchor::Unknown` and offer nothing. Do not guess.

## Whitelisted globals

Jinja templates see a restricted namespace built in `frappe/utils/jinja.py`, plus
whatever the `jinja` hook adds ([12](12-surface-hooks.md)). Completion offers
that set; calling anything outside it fails at render. Reporting non-whitelisted
calls is valuable but requires the vendored global list to be exact — until it
is, completion only, no diagnostics.
