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

A DocType's form scripts are both the `<x>.js` beside its JSON and anything
`doctype_js` bound to it from elsewhere in the workspace — including from
another app. The directory cannot show the second, and the hook is the only
record ([12](12-surface-hooks.md)).

## References

`textDocument/references` on a DocType or field is a workspace-wide reverse
lookup. It MUST cover every surface — a DocType is referenced from Python, JS,
other DocTypes' `options`, hooks, and fixtures, and a result set that silently
omits one is worse than none.

This requires a reverse index (name → occurrence sites) built by a scan, not an
on-demand grep. Occurrences are stored per file and invalidated with that file.

The scan is a full extract-and-resolve pass over every source file and costs
about as much as the index build itself — 1.9 s against 1.4 s on `frappe` plus
`erpnext` — so it runs **after** the index is published rather than inside the
cold-index budget of [31](31-performance.md), and the cache carries the result
so a warm start pays nothing. `frappe-lsp/indexStatus` reports `occurrences`
only once the table exists, so a client can tell "no references" from "not
yet"; until then `references` answers nothing rather than answering wrongly.

Measured on that bench: 89 523 sites, 18 MiB of cache, and a `references`
request on `Item` — 821 sites across 300 files — in 18.7 ms against a 100 ms
target.

Sites come from these surfaces, and each was added because leaving it out made
the result wrong rather than merely short:

| Surface | What contributes |
| --- | --- |
| Python, JavaScript | Every resolved DocType and field position |
| DocType JSON | `Link` / `Table` / `Table MultiSelect` `options`, each field's own declaration, and each `field_order` entry naming a declared field |
| Controller | Each `fieldname: DF.<Type>` line in its `# begin: auto-generated types` block |
| `hooks.py` | Map keys of the hooks written `Map<DocTypeName, …>` in [12](12-surface-hooks.md) |
| Fixtures | A Custom Field's `dt` and fieldname, a Property Setter's `doc_type` |

A rename that misses a `field_order` entry drops the field from the form, and
one that misses the generated types leaves them stale, so both are sites. The
generated block yields sites only, never findings: regenerating it is `bench`'s
job, and a block behind its JSON is ordinary between saves.

The answer to `frappe-lsp/references` ([30](30-configuration.md)) says what it
cannot hold: code outside an app package, such as a frappe-ui dashboard, and
attribute reads on a name [06](06-analysis-pipeline.md) does not bind.

A field's declaration is itself a reference: a list that omits where the field
is defined sends the reader nowhere. It is also a place to ask from: a request
on a site the scan recorded — a fieldname or `options` in DocType JSON, a
fixture — answers for that site's key, though no analysis covers the file.

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

Every `location` MUST carry a range. LSP 3.17 allows a URI alone only for a
client that advertises `workspaceSymbol/resolve`, and Claude Code fails on one.
A DocType, report, page or web form points at the top of its file. A method
points at its name, converted after the cap, so one query reads at most 200
files.

Fields are **not** workspace symbols — 100k+ entries would drown every other
result. They are reachable through their DocType.
