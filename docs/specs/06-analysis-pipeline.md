# 06 — Analysis pipeline

How source text becomes an anchored, resolvable match. Implemented in
`frappe-syntax`; consumed by the surfaces ([10](10-surface-python.md)–[13](13-surface-jinja.md)).

## Grammars

| Language | Grammar | Used for |
| --- | --- | --- |
| Python | `tree-sitter-python` | controllers, `hooks.py`, reports, patches |
| JavaScript | `tree-sitter-javascript` | form scripts, list views, bundles |
| JSON | `tree-sitter-json` | DocType and fixture files |
| HTML+Jinja | see [13](13-surface-jinja.md) | templates |

DocType JSON is parsed twice: `serde_json` for the index, tree-sitter for
in-buffer positions. Only open buffers pay the tree-sitter cost.

## Recognition, not evaluation

Matching is done with tree-sitter queries against call shapes. The server does
**not** implement dataflow, imports, or constant folding.

Recognized:

```python
frappe.get_all("Sales Invoice", fields=["customer"])
```

Not recognized — and MUST NOT produce a diagnostic:

```python
frappe.get_all(dt, fields=cols)              # non-literal
frappe.get_all(DOCTYPE_CONST, fields=[...])  # module constant
frappe.get_all(f"Sales {kind}", ...)         # interpolated
```

An unresolvable argument yields `Anchor::Unknown`, which suppresses both
completion and diagnostics for dependent positions. This is the core accuracy
tradeoff: we accept lower recall to keep precision near 1.0.

## Local binding inference

The one exception, deliberately bounded. Within a single function body, a direct
assignment from a recognized constructor binds a name to a DocType:

```python
def on_submit(self):
    inv = frappe.get_doc("Sales Invoice", self.invoice)
    inv.customer          # ← completion, hover, go-to-definition
```

Rules:

- Source MUST be a recognized constructor with a literal DocType
  (`get_doc`, `get_cached_doc`, `new_doc`, `get_last_doc`, `get_single`).
- Target MUST be a plain identifier.
- Scope is the enclosing function body; nested functions do not inherit.
- **Any** reassignment of the name, of any shape, drops the binding for the rest
  of the scope. No branch merging, no loop analysis.
- `self` in a controller file binds to the owning DocType.
- Iteration over a `Table` field binds the loop variable to the child DocType:
  `for row in self.items:` → `row` is the child.

Anything beyond this — parameters, returns, class attributes, comprehension
scoping — is out of scope. If it later proves necessary, it needs its own spec,
not an extension of these rules.

[24](24-type-integration.md) overlaps this for Python, and pyright's own literal
narrowing is strictly better where its stub is active. These rules still stand:
they drive our hover and diagnostics, they work with that stub disabled, and they
anchor the fieldname-in-string positions no type checker can reach. The
double-reporting rule is in [23](23-diagnostics.md).

## Incremental updates

| Event | Action |
| --- | --- |
| `didOpen` | Parse, cache tree + rope. |
| `didChange` | `Tree::edit` with the converted range, re-parse against the old tree. |
| `didClose` | Drop tree and rope; the on-disk index entry stays. |
| `didSave` on an indexed file | Re-index that file, patch the graph. |
| Watcher event | Same, debounced 250 ms, coalesced per path. |

Position conversion respects the negotiated `positionEncoding`; default UTF-16.
A rope keeps line-index and offset conversion O(log n).

## Anchor resolution

An extracted match is `(pattern_id, spans)`. Resolution turns it into:

```
Anchor::DocType(name)      // literal or inferred binding
Anchor::Field(doctype, fieldname)
Anchor::DottedPath(path)   // [07]
Anchor::Unknown            // suppress everything downstream
```

Field positions are resolved only when their DocType anchor resolved first —
`fields=[...]` on an unknown DocType contributes nothing.
