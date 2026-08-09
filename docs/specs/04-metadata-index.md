# 04 — Metadata index

The resolved graph every feature queries. Built from [03](03-workspace-model.md),
extended by [05](05-live-bridge.md).

## Entities

```rust
DocType {
  name, module, app, path,
  is_child, is_single, is_submittable, is_virtual, is_tree,
  autoname, naming_rule, title_field, sort_field, search_fields,
  fields: Vec<Field>, field_order: Vec<FieldName>,
  permissions, links, states,
  controller: Option<ControllerRef>,
  provenance: Layer,
}

Field {
  fieldname, fieldtype, label, options, default,
  reqd, hidden, read_only, unique, is_virtual, permlevel,
  depends_on, mandatory_depends_on, read_only_depends_on, collapsible_depends_on,
  fetch_from, link_filters,
  range: Range,           // location inside the owning JSON
  provenance: Layer,
}
```

Also indexed: `App`, `Module`, `ControllerClass` (methods + ranges),
`WhitelistedMethod` (dotted path, `allow_guest`, HTTP methods, range), `Report`,
`Page`, `WebForm`, `PrintFormat`, `Patch`, `HookAssignment`, `Template`.

## Field semantics

`options` is overloaded by `fieldtype`. Resolution table — everything else treats
`options` as opaque:

| Fieldtype | `options` means |
| --- | --- |
| `Link` | Target DocType name |
| `Table`, `Table MultiSelect` | Child DocType name |
| `Dynamic Link` | Sibling **fieldname** holding the target DocType name |
| `Select` | Newline-delimited choices (leading blank = optional) |
| `Data` | Format hint (`Email`, `Phone`, `URL`, …) or empty |
| `Currency`, `Float` | Fieldname or dotted path to a currency field |

`fetch_from` is `<link_fieldname>.<target_fieldname>` — both halves are
resolvable and both are checked in [23](23-diagnostics.md).

## Implicit fields

Every DocType carries fields absent from its JSON. They MUST be completable and
MUST NOT be reported as unknown.

- Standard: `name`, `owner`, `creation`, `modified`, `modified_by`, `docstatus`, `idx`
- Child DocTypes additionally: `parent`, `parentfield`, `parenttype`
- Optional/meta: `_user_tags`, `_comments`, `_assign`, `_liked_by`, `_seen`

Source of truth: `frappe/model/__init__.py`. Fieldtype tables (`data_fieldtypes`,
`no_value_fields`, `table_fields`, `numeric_fieldtypes`) are vendored from the
same module and version-pinned per [31](31-performance.md).

## Keys and lookup

| Key | Example | Use |
| --- | --- | --- |
| DocType name | `Sales Invoice` | Primary. Exact, case-sensitive. |
| Scrubbed | `sales_invoice` | Path ↔ name mapping |
| Dotted path | `erpnext.controllers.foo.bar` | [07](07-dotted-paths.md) |

Lookup MUST be exact. A case-insensitive match is offered only as a completion
suggestion or a diagnostic hint ("did you mean"), never as a silent resolution —
Frappe itself is case-sensitive here.

`scrub` is lossy: `Item-Group` and `Item Group` both scrub to `item_group`. Path
→ name resolution therefore goes through the JSON `name` key, never `unscrub`.

## Layer merge

Layers apply in ascending order. Within a layer, app order from
[03](03-workspace-model.md) breaks ties.

| Layer | Source | Contributes |
| --- | --- | --- |
| 1 | `<module>/doctype/<x>/<x>.json` | DocTypes and their fields |
| 2 | `<app>/fixtures/*.json` | Custom Field, Property Setter records |
| 3 | Live site ([05](05-live-bridge.md)) | Same, plus DB-only DocTypes |

Rules:

- **Custom Field** appends a field to an existing DocType. Collision with an
  existing `fieldname` is a conflict diagnostic, not an override.
- **Property Setter** overrides one property on a DocType or field. `doc_type` +
  `field_name` + `property` is the key; `field_name` empty targets the DocType.
- A DocType defined in two Layer-1 apps is a conflict: the later app wins, and a
  workspace diagnostic is published against both files.
- Every entity records `provenance` so hover ([22](22-hover.md)) can say where a
  field came from, and so dropping the bridge cleanly removes Layer 3.

## Controller resolution

`<x>.py` beside the JSON is the base controller. `hooks.py` alters it:

- `override_doctype_class` — replaces it; go-to-definition offers both.
- `extend_doctype_class` — adds mixins; their methods join the method set.

Both are indexed from [12](12-surface-hooks.md) and resolved via
[07](07-dotted-paths.md).

## Cache

Serialize to `<bench>/.frappe-lsp/index.v<N>.bin`. Validity key: schema version +
`(path, mtime, len)` for every indexed file. Any mismatch re-indexes only the
changed files. Cache is an optimisation — a corrupt or absent cache MUST fall
back to a full scan. Budgets in [31](31-performance.md).
