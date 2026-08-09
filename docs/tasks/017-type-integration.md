# 017 — Type checker integration

**Phase** 4 · **Depends on** 009 · **Experimental**

[24](../specs/24-type-integration.md). Opt-in, off by default. Does not block any
other task.

## Scope

### Mechanism B first

The cast code action ([24](../specs/24-type-integration.md)) — cheap, no
performance cost, useful with the stub disabled. Ship it before the generator.

- Code action on `get_doc`-family calls with a literal DocType.
- Insert controller import + `typing.cast`; respect existing imports.
- `cast` only — a bare annotation is a type error, `isinstance` breaks under
  `override_doctype_class`.

### Mechanism A

- Emit a **complete** `frappe/model/document.pyi` from the real `document.py`,
  with Literal overloads injected ahead of the existing generic ones.
- Cover `get_doc`, `get_cached_doc`, `get_lazy_doc`, `get_last_doc`, `new_doc`,
  `get_single`.
- Effective controller class per [04](../specs/04-metadata-index.md), including
  `override_doctype_class`; no controller → no overload.
- `scope = "referenced"` (default) from the [10](../specs/10-surface-python.md)
  extraction set; `scope = "all"` behind a documented warning.
- Write only to `<bench>/.frappe-lsp/stubs/`. Never into `apps/`.
- Header with generator version + `frappe` commit + do-not-edit notice.
- Regenerate on referenced-set change, controller rename, or `frappe` move.
- Suppress `unknown-field` on attribute access while active
  ([23](../specs/23-diagnostics.md)).

## Done when

- Cast action produces code pyright accepts, with no unused-import residue.
- Generated stub makes `frappe.get_doc("User", …).full_name` resolve, and a
  misspelled field error, under stock pyright.
- Stub generated against `frappe` `develop` produces **zero new pyright
  diagnostics** on `frappe` itself — the completeness check from
  [32](../specs/32-testing.md). Incomplete generation fails loudly.
- Measured pyright cost on a real app is recorded in
  [24](../specs/24-type-integration.md), replacing the interpolated estimate.

## Risks

- Stub completeness drifts with every `frappe` release; the smoke test is the
  only thing standing between that and silent false errors.
- Teams may reject a generated `document.pyi` on principle. Mechanism B is the
  fallback for them, which is why it ships first.
