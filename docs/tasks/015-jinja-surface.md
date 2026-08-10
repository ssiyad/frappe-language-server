# 015 — Jinja surface

**Phase** 4 · **Depends on** 014

[13](../specs/13-surface-jinja.md). Grammar strategy must be settled first — it
is the half of the design that was deferred to phase 4.

## Scope

- Grammar decision, recorded in [13](../specs/13-surface-jinja.md) before coding.
- Template discovery: `templates/`, `www/`, print formats, web template blocks.
- `include` / `extends` / `import` path resolution → definition.
- `frappe.*` call patterns inside expressions, reusing
  [10](../specs/10-surface-python.md) resolution.
- `doc` anchoring where the DocType is known; `Unknown` elsewhere — no guessing.
- Whitelisted Jinja globals plus `jinja` hook additions: completion only, no
  diagnostics until the vendored global list is proven exact.

## Done when

Template includes navigate across apps, and `{{ doc.field }}` completes in a
print format. Zero diagnostics on `frappe` and `erpnext` templates.
