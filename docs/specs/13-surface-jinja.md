# 13 — Jinja surface

Lowest priority of the four surfaces; deferred to phase 4. Specified now because
it constrains grammar choice in [02](02-architecture.md).

## Scope

Templates under `<app>/templates/`, `<app>/www/`, `<app>/**/templates/`, plus
Print Format HTML and Web Template blocks stored in DocType JSON fields.

## Grammar

No mature tree-sitter grammar covers HTML with embedded Jinja. The options
weighed:

| Option | Assessment |
| --- | --- |
| `tree-sitter-html` + regex for `{{ }}` / `{% %}` | Cheap, adequate for path and name extraction, no expression structure |
| Existing community Jinja grammars | Incomplete and unmaintained; risk outweighs benefit |
| Hand-written scanner for Jinja delimiters, HTML ignored | Most control; delimiters are simple and stable |
| **Delimiter scanner + the Python grammar** | **Chosen** |

Jinja expressions are Python expressions with a few additions, and the calls
that matter here — `frappe.get_doc(D)`, `_("…")` — are plain Python inside
them. A scanner finds the blocks, and the template is rewritten into a Python
document of exactly the same bytes: markup becomes spaces, newlines stay,
`{{ e }}` becomes `( e ),`, an `if`/`for`/`set` statement keeps its expression,
and the whole is one list literal. The Python grammar parses that and the
Python extractor ([10](10-surface-python.md)) reads it, so every span it
reports is a span in the template. No second extractor exists to drift.

What Jinja reads differently is rewritten in place, keeping the length: `~`
becomes `+`, an `if` without `else` becomes `or`, and a variable named like a
Python keyword (`class`, `from`) gets `_` for its first letter. frappe's
client-side micro-templates share the delimiters but hold JavaScript
(`{% if (x) { %}`, `{%= e %}`, `||`); they are recognized and read as text.

Measured on `frappe` and `erpnext`: of 324 HTML files with Jinja blocks, 320
parse with no error node. The other four are DocType boilerplate written as
Python format strings, which are not templates.

## Positions

| Pattern | Effect |
| --- | --- |
| `{% include "app/templates/x.html" %}` | File path → go-to-definition |
| `{% extends "…" %}`, `{% import … %}` | Same |
| `frappe.get_doc(D, …)`, `frappe.db.get_value(D, …, F)` | As [10](10-surface-python.md), inside expressions |
| `frappe.get_all(D, …)` | Same |
| `_("…")` | Translation |
| `{{ doc.<attr> }}` | `F`, when the template's DocType is known |

## Template paths

`include`, `extends`, `import` and `from` name a template the way frappe's
loader finds it, never relative to the file. `frappe/utils/jinja.py` chains a
prefix loader — `app/templates/x.html` is read from that app's package — and
then every app's package, most recently installed first, for
`templates/x.html`. Definition follows that order; hover shows the file, or
says no installed app has it.

## Diagnostics

None yet. Navigation, hover, completion and references read templates; the
checks of [23](23-diagnostics.md) wait until they are measured quiet on the
`frappe` and `erpnext` templates, the same bar every other surface met.

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
