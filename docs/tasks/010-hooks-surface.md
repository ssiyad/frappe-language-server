# 010 — Hooks surface

**Phase** 3 · **Depends on** 009, 021 · **Blocks** 011, 012

[12](../specs/12-surface-hooks.md). Highest value per line of code on the
project — and it feeds the index, so it comes before the JS surface.

## Scope

- Module-level assignment extraction; non-literal values indexed as opaque.
- Vendored hook registry (name → value schema) via `xtask codegen`.
- Value validation for every schema: `Path(kind)`, `File`, `DocTypeName`, `Map`.
- Doc event names, including the `"*"` wildcard DocType key.
- Scheduler frequencies and cron expression validation.
- Asset path resolution ([07](../specs/07-dotted-paths.md)), excluding generated
  bundles under `sites/assets/`.
- Cross-hook effects into the index: `override_doctype_class`,
  `extend_doctype_class`, `override_whitelisted_methods`, `doctype_js`, `jinja`.
- Ordering: `hooks.py` indexed before any JS or template in the same app.
- Completion and hover for hook keys, doc events, frequencies.

## Done when

Every value schema is exercised by the fixture bench, and `frappe` + `erpnext`
`hooks.py` files validate clean.
