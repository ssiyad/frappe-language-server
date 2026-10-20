# 24 — Type checker integration

**Experimental.** Opt-in, off by default.

Makes pyright resolve `frappe.get_doc("Sales Invoice", …)` to the controller
class instead of `Document`. We do not override pyright — we generate input for
it.

## Why this is not just a completion gap

`Document` and `BaseDocument` define no `__getattr__`. Pyright therefore emits a
hard error on ordinary Frappe code:

```
error: Cannot access attribute "customer" for class "Document"
```

The additive features in [20](20-completion.md)–[22](22-hover.md) cannot fix
this; only the type checker's own view of the return type can. That makes this
the one place where feeding another server beats serving the request ourselves.

## Mechanism A — generated overloads

`frappe.get_doc` already carries five `@overload`s in `frappe/model/document.py`,
one of which returns `Document` for `(doctype: str, name: str)`. Literal
overloads inserted ahead of it dispatch exactly:

```python
@overload
def get_doc(doctype: Literal["Sales Invoice"], name: str, /) -> SalesInvoice: ...
```

Confirmed against pyright: the call reveals as `SalesInvoice`, field access
types correctly, and a misspelled field becomes a real error. Pyright also
narrows `dt = "Sales Invoice"` to a literal on its own, so indirection through a
local variable still resolves.

### Covered functions

`get_doc`, `get_cached_doc`, `get_lazy_doc`, `get_last_doc`, `new_doc`,
`get_single`. Each gets one overload per in-scope DocType, emitted **before** the
existing generic overloads, which are preserved as the fallback.

The target class is the effective controller from [04](04-metadata-index.md),
including `override_doctype_class`. A DocType with no controller class emits no
overload and falls through to `Document`.

### Scope

Pyright scans overloads in declaration order, so cost is ~O(N × call-sites).
Measured, 200 call sites:

| Overloads | Wall | vs baseline |
| --- | --- | --- |
| 0 | 531 ms | 1× |
| 100 | 694 ms | 1.3× |
| 500 | 3,317 ms | 6× |
| 1,000 | 7,368 ms | 14× |
| 2,500 | 32,523 ms | 60× |

`scope = "referenced"` (default) emits an overload for a DocType only on the
calls the workspace makes with it: a DocType only ever passed to `get_doc` gets
no `new_doc` overload, and frappe's `get_doc(doctype, /)` form, which reads a
Single, is copied for Singles only. The occurrence table records each
`(call, DocType)` pair per file, so a save keeps the set exact. On the eight-app
reference bench this is 517 DocTypes and 912 overloads; one overload per
DocType per function would have been 3,619, and pyright ran out of memory
checking `frappe` against those.

`scope = "all"` exists as an escape hatch and MUST be documented with the 2,500
row above. It is not a supported configuration.

Measured on the reference bench, pyright 1.1, stub against frappe `c7a96ecc99`:

| Checked | Without | With | Errors without → with |
| --- | --- | --- | --- |
| one controller, `sales_order.py` | 3.0 s | 5.2 s | 52 → 44 |
| erpnext `selling/`, 161 files | 9.1 s | 18.3 s | 2,819 → 2,034 |
| all of `frappe`, 1,727 files | 39 s | 131 s, 2.5 GB peak | 14,553 → 12,529 |

Checking all of `frappe` with the stub needs more than node's default heap
(`NODE_OPTIONS=--max-old-space-size=4096`): the stub imports every covered
controller, so a check of one app loads the controllers of all of them. An
editor checks the open files, which is the first row.

Beyond scoping, we accept the remaining cost. Lazy stub splitting, per-file
overload subsets and incremental stub emission are explicitly out of scope while
this is experimental.

### Placement

A `.pyi` replaces a module's types wholesale, and there is no partial-stub
mechanism for a package without `py.typed`. So the generated
`frappe/model/document.pyi` MUST be complete: every public symbol of
`document.py`, with the Literal overloads injected.

It is complete by construction: the stub is `document.py` itself, with the
overloads inserted ahead of frappe's own and every module-level import
rewritten as `from x import y as y`. A `.pyi` re-exports only that form, and
without it every `from frappe.model.document import get_controller` elsewhere
broke. Each new overload copies one of frappe's own signatures, so they follow
the installed version. The controllers are imported under private aliases, so
no class name collides with one `document.py` defines. A DocType replaced by
`override_doctype_class` returns the override.

Pyright reads `stubPath` module by module, so a stub directory holding only
`frappe/model/document.pyi` leaves the rest of `frappe` to the real package.

Set `types.stub = true` and point pyright at the directory:

```json
{ "stubPath": ".frappe-lsp/stubs" }
```

Never write into `apps/`. The stub is a build artifact, not source, and
`.frappe-lsp/` is already gitignored ([31](31-performance.md)).

### Generation and staleness

The server writes the stub once the occurrence table exists, and again after
any save or watched-file change that alters it; an unchanged stub is not
rewritten. The stub carries a header naming the generator version, the
`frappe` commit it was derived from, and a do-not-edit notice. A stub that does
not parse is never written. `cargo xtask stub [BENCH]` writes one without an
editor.

Completeness is the drift risk: `document.py` gains a symbol, the stub omits it,
and pyright reports it missing. Copying the file removes that risk. The check
of [32](32-testing.md) — generate, run pyright over `frappe`, compare — found
zero missing symbols once imports were re-exported. "Zero new diagnostics" is
not reachable and is not the check: better types expose real errors. Of the 575
new errors on `frappe`, 398 repeat an argument error the line already had (with
many overloads pyright adds "no overloads match"), and the rest are optional
fields — `Data | None`, `Link | None` — passed where `str` is required, which
`Document` used to hide. Against that, 2,385 `Cannot access attribute … for
class "Document"` errors disappear.

While the stub is on, `unknown-attribute` is off whatever its configured level:
pyright reports `doc.x` from the real types.

## Mechanism B — cast code action

Independent of A, no performance cost, and correct even with the stub disabled.
On a `get_doc` call with a literal DocType, offer:

```python
doc = cast("SalesInvoice", frappe.get_doc("Sales Invoice", name))
```

inserting `typing.cast` and the controller import. The import goes under
`if TYPE_CHECKING:` and the class is named as a string: erpnext's controllers
import each other, and a runtime import made only for a type is a circular
import waiting to happen. An existing `from typing import` line is extended and
an existing `TYPE_CHECKING` block reused. Checked with pyright against
erpnext's `SalesInvoice`: real fields resolve, a misspelled one is reported, and
no import is left unused. A bare annotation
(`doc: SalesInvoice = frappe.get_doc(...)`) is **not** valid — `Document` is not
assignable to a subclass — so `cast` is the only correct single-site form.
`assert isinstance(...)` is rejected: it costs runtime and breaks under
`override_doctype_class`.

## Interaction with local inference

[06](06-analysis-pipeline.md)'s local binding inference overlaps mechanism A, and
pyright's literal narrowing is strictly better where the stub is active. The
inference still earns its place: it drives our own hover and diagnostics, it
works with the stub disabled, and it anchors the fieldname-in-string positions
(`fields=[…]`, `filters={…}`) that no stub can reach.

To avoid double-reporting, when the stub is active the `unknown-field` check in
[23](23-diagnostics.md) MUST be suppressed for **attribute access** on bound
names — pyright already reports it, more accurately. String-literal field
positions are unaffected and remain ours.

## Upstream

This arguably belongs in Frappe's type exporter rather than here: `bench` already
generates `DF` annotations per controller and could emit scoped overloads per
app, benefiting CI and every editor. Adding `__getattr__` to `BaseDocument` would
also silence the errors at zero cost, at the price of all field checking. Both
are maintainer decisions. Treat this spec as the experiment that informs them,
not as the permanent home.
