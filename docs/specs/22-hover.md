# 22 — Hover

`textDocument/hover`. Markdown, rendered from the index. The value of hover here
is that the answer otherwise requires opening a JSON file and reading 200 fields.

## Rules

- Range MUST be the string interior or identifier, matching
  [21](21-navigation.md)'s `originSelectionRange`.
- Never exceed ~15 lines. Editors render hover in a small popup; anything longer
  is scrolled past.
- Omit empty properties entirely rather than printing `None`.
- Return `None` for `Anchor::Unknown`. Do not fall back to a guess.

## DocType

```markdown
**Sales Invoice** — Accounts · erpnext

Submittable · 214 fields · naming: `naming_series:`
Title field: `customer_name`

`accounts/doctype/sales_invoice/sales_invoice.json`
```

Flags shown only when true: Submittable, Single, Child Table, Virtual, Tree,
Custom. Field count includes Layer 3 additions, annotated when present:
`214 fields (+3 custom)`.

## Field

```markdown
**customer** — Link → Customer

Required · In list view
Fetches from `party.customer_name`
Depends on `doctype == "Sales Invoice"`

Sales Invoice · accounts
```

| Fieldtype | Extra line |
| --- | --- |
| `Link`, `Table`, `Table MultiSelect` | Target DocType, linked |
| `Dynamic Link` | The sibling field that supplies the target |
| `Select` | Options, truncated after 8 with `…` |
| `Currency`, `Float`, `Percent` | Precision if set |
| Any | `default`, when set |

Provenance MUST be stated for anything not from Layer 1 — a Custom Field reads
`Custom Field · <site>` so the reader knows it is absent from the codebase.

## Whitelisted method

```markdown
**erpnext.stock.get_item_details.get_item_details**(args, doc, for_validate)

Whitelisted · GET, POST
Guest access: no
```

If remapped by `override_whitelisted_methods`, add the effective target.

## Hook key

```markdown
**doc_events** — `Map<DocType, Map<Event, Path>>`

Handlers invoked around document lifecycle methods.
[Docs](https://docs.frappe.io/framework/user/en/basics/hooks)
```

Schema line comes from the vendored registry ([12](12-surface-hooks.md)).

## Doc event and form event

Name, when it fires, and its position relative to neighbours — the ordering of
`before_save` / `validate` / `on_update` is the single most common source of
confusion, and hover is the right place to settle it.
