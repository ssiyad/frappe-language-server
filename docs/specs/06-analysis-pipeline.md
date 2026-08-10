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

Matching is two layers, and the split is where the query DSL stops paying:

1. **Location** — tree-sitter queries find call sites and decorators, and
   nothing else. Queries are what the grammar is good at, and they keep node
   kinds out of the rest of the pipeline.
2. **Shape** — the argument reading is a table of call shapes in Rust. Which
   kwarg is a fieldname list, which positional re-anchors, which dict keys are
   child fields: none of it is expressible as a pattern over the tree, and a
   query that captured every call would leave all of it in Rust anyway.

The server does **not** implement dataflow, imports, or constant folding.

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

`Anchor::Unknown` covers two cases that must not be confused, and extraction
distinguishes them by whether the position carries text:

| Case | Text | Downstream |
| --- | --- | --- |
| Nothing was statically knowable — a variable, a call, a f-string | absent | Suppress everything |
| The literal read fine, but naming its DocType needs the index | present | Resolve, then treat as a normal position |

The second case is every child-field position (`doc.append`, `frm.add_child`,
the 3-argument `set_query`), form event keys, hook keys and field properties.
Extraction cannot finish them without becoming an index, so it hands over what
it knows: the match carries the receiver's binding — a DocType name, or a table
row as `(parent doctype, table fieldname)` — and the server layer completes the
join. Table-iteration binding is therefore extraction's half of a two-part
answer, not a complete one.
