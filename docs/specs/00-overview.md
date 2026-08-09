# 00 — Overview

## Problem

Frappe encodes most cross-file relationships as strings resolved at runtime:

```python
frappe.get_all("Sales Invoice", filters={"customer": x}, fields=["grand_total"])
```

`"Sales Invoice"`, `"customer"`, `"grand_total"` are opaque to pyright. The same
holds for `hooks.py` dotted paths, `frappe.ui.form.on()` handlers, `patches.txt`
entries, and Jinja templates. Errors surface at runtime, and navigation degrades
to grep.

## What is already covered

Frappe generates `DF.*` annotations into controllers via
[`frappe/types/exporter.py`](https://github.com/frappe/frappe/blob/develop/frappe/types/exporter.py).
Its stated non-goals define our opportunity:

> - `get_doc` outside of same file without explicit annotation is out of scope.
> - Customizations like change of fieldtype and addition of fields are out of scope.

So `self.fieldname` inside a controller already works. Everything reached through
a string literal does not.

## Goals

1. Resolve DocType and fieldname string literals across Python, JavaScript, JSON
   and Jinja.
2. Navigate the artifacts Frappe wires by convention — controllers, hooks,
   patches, reports, whitelisted methods.
3. Report Frappe-specific mistakes that no general linter can detect.
4. Run offline against a bench checkout, with no dependency on a running site.
5. Where another server can be made correct instead of duplicated, feed it —
   see [24](24-type-integration.md).

## Non-goals

| Not doing | Because |
| --- | --- |
| Python/JS language features | pyright, ruff, ts-ls already do this |
| Type inference across module boundaries | Bounded local inference only — [06](06-analysis-pipeline.md) |
| Regenerating `DF.*` blocks | Owned by `bench` / the type exporter |
| Replacing pyright's view of a type | We generate stubs for it, never shadow it — [24](24-type-integration.md) |
| Cross-file field rename | Needs a `rename_field` patch, not a text edit |
| Formatting | `ruff format`, `prettier` |
| Editor extensions | Phase 1 targets generic stdio LSP — [30](30-configuration.md) |

## Design constraints

- **No runtime import of `frappe`.** The server must work on a checkout with no
  virtualenv, no database, and no site.
- **Syntactic recognition, not semantic evaluation.** We match call shapes; we do
  not emulate the interpreter. Consequences in [06](06-analysis-pipeline.md).
- **Silence over noise.** A diagnostic that fires on valid code is worse than a
  missing one. Confidence gating in [23](23-diagnostics.md).
- **Degrade, don't fail.** A malformed DocType JSON removes one entry from the
  index; it does not stop the server.

## References

- [01 Glossary](01-glossary.md)
- [02 Architecture](02-architecture.md)
