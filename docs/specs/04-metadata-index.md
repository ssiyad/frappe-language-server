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
  range: Range,           // byte offsets into the owning file
  provenance: Provenance,
}

Provenance { layer: Layer, app: AppName, source: Option<Path> }
```

`Range` is a byte offset pair into the file named by `provenance.source`. Line
and character conversion happens at the protocol edge, which is the only place
that knows the session's `positionEncoding` ([30](30-configuration.md)).

A layer alone cannot answer "where did this field come from" — the app and the
file are what hover ([22](22-hover.md)) needs. `source` is `None` only for
Layer 3, which has no file.

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

- Standard: `doctype`, `name`, `owner`, `creation`, `modified`, `modified_by`,
  `docstatus`, `idx`
- Child DocTypes additionally: `parent`, `parentfield`, `parenttype`
- Optional/meta: `_user_tags`, `_comments`, `_assign`, `_liked_by`, `_seen`

Source of truth: `frappe/model/__init__.py`, whose `default_fields` leads with
`doctype`. Fieldtype tables (`data_fieldtypes`, `no_value_fields`,
`table_fields`, `numeric_fieldtypes`) are vendored from the same module and
version-pinned per [31](31-performance.md).

Implicit fields carry no range and no provenance: there is no file to point at,
and a synthetic range would send go-to-definition to byte 0. They are therefore
queryable but absent from `fields`.

### Document members

`doc.<attr>` is Python attribute access, and most of what it reaches is not a
field. `Document` and `BaseDocument` are vendored the same way as the fieldtype
tables, and a name matching one of them — or any dunder — resolves as a member,
never as an unknown field. The controller's own methods are checked too: a
`validate` a subclass defines is on no base class.

Order matters, and it is fields first: a DocType whose schema declares `status`
or `title` keeps it. Members lose ties.

Measured on `frappe` 17.0.0-dev, skipping this costs 118 distinct unknown
fields against code that is correct — `flags` alone on 24 DocTypes — which is
what [23](23-diagnostics.md)'s governing rule forbids. With it, 38 remain, all
attributes assigned at runtime.

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
| 2 | `<app>/fixtures/*.json`, `<module>/custom/*.json` | Custom Field, Property Setter records |
| 3 | Live site ([05](05-live-bridge.md)) | Same, plus DB-only DocTypes |

Rules:

- A `<module>/custom/<x>.json` is what Export Customizations writes: one object
  whose `custom_fields` and `property_setters` hold the records. Every such file
  counts, whatever its `sync_on_migrate`: Frappe also applies it when its app
  is installed.
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
changed files. Adding or removing an **app** is not incremental — install order
decides the merge, so a change to the installed set is a full rebuild. Cache is an optimisation — a corrupt or absent cache MUST fall
back to a full scan. Budgets in [31](31-performance.md).

## Controller attributes

A controller is an ordinary Python class and hangs working state off `self`:
`self._attachments`, a cached total, a flag set in `validate` and read in
`on_submit`. None of those is a field, and every one is read back later, so the
index records them beside the methods.

Collected from the controller class, from every class `hooks.py` mixes into it
(`extend_doctype_class`, `override_doctype_class`), and from every class it
derives from — erpnext's controllers inherit `AccountsController` and
`StockController`, which set most of what the leaf classes read. Bases are
matched by bare class name rather than resolved through imports: the cost of a
collision is one suppressed diagnostic, never a wrong one, which is the
direction [23](23-diagnostics.md) says to err in.

`self` binds to the DocType only inside the **controller class**. A controller
file holds other classes too — erpnext's `stock_entry_type.py` carries a plain
`ManufactureEntry` beside the `Document` subclass — and their `self` is not a
document at all.

Frappe also queries several of its own tables by name: `Singles`, `Series`,
`Sessions`, `__global_search`, `__Auth`, `__UserSettings`. They have no DocType
JSON anywhere and resolve to nothing rather than to an unknown DocType.
