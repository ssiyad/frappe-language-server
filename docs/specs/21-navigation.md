# 21 — Navigation

`textDocument/definition`, `references`, `documentSymbol`, `workspaceSymbol`.

## Definition

Return `LocationLink[]` — most Frappe targets are genuinely plural, and a list
lets the client disambiguate instead of the server guessing.

| At | Targets, in order |
| --- | --- |
| `D` literal | Controller `<x>.py` class, DocType JSON, form script `<x>.js` |
| `F` literal / `doc.<attr>` | The field's object inside its DocType JSON; Custom Fields resolve to their fixture or report as Layer 3 ([04](04-metadata-index.md)) |
| Link/Table `options` in JSON | Target DocType |
| `fetch_from` value | Both halves: the link field, and the field on the target |
| `depends_on` `doc.x` reference | Sibling field |
| Dotted path anywhere | Resolved symbol ([07](07-dotted-paths.md)) |
| `frappe.call` `method` | Whitelisted def; if remapped by `override_whitelisted_methods`, the effective target first, original second |
| `patches.txt` line | Patch module |
| `doctype_js` / `app_include_js` value | Asset file |
| Hook key | External docs URL via `documentation` on hover, not a location |

`originSelectionRange` MUST cover the string interior only, so the editor
highlights the name and not the quotes.

`textDocument/typeDefinition` on a bound name ([06](06-analysis-pipeline.md))
goes to the DocType JSON. `implementation` on a controller class lists
`override_doctype_class` and `extend_doctype_class` targets.

## References

`textDocument/references` on a DocType or field is a workspace-wide reverse
lookup. It MUST cover every surface — a DocType is referenced from Python, JS,
other DocTypes' `options`, hooks, and fixtures, and a result set that silently
omits one is worse than none.

This requires a reverse index (name → occurrence sites) built during the initial
scan, not an on-demand grep. Occurrences are stored per file and invalidated with
that file. Budget in [31](31-performance.md).

Field references are scoped to their DocType: `customer` on `Sales Invoice` MUST
NOT return `customer` occurrences anchored to `Sales Order`. Occurrences whose
anchor is `Unknown` are excluded — including them would flood the result.

## Document symbols

| File | Symbols |
| --- | --- |
| DocType JSON | DocType as root; fields as children, `Field` kind, `detail` = fieldtype. Section/Column/Tab Break fields nest the fields that follow them |
| `hooks.py` | One symbol per hook assignment |
| `patches.txt` | One per patch, sectioned by `[post_model_sync]` |
| Controller `.py`, `.js` | None — `documentSymbol` is owned by the general language server |

The DocType JSON case is the valuable one: a 200-field JSON file is otherwise
unnavigable.

## Workspace symbols

Query matches DocTypes, whitelisted methods, reports, pages, and web forms.
Fuzzy, case-insensitive, capped at 200 results. `containerName` is
`<app> · <module>`.

Fields are **not** workspace symbols — 100k+ entries would drown every other
result. They are reachable through their DocType.
