# 015 — Jinja surface

**Phase** 4 · **Depends on** 014

[13](../specs/13-surface-jinja.md). Grammar strategy must be settled first — it
is the half of the design that was deferred to phase 4.

## Done already

The grammar: templates are rewritten to a Python document of the same bytes and
read by the Python extractor; 320 of the 324 templates in `frappe` and
`erpnext` parse with no error, and the other four are not templates
([13](../specs/13-surface-jinja.md)). `include` / `extends` / `import` / `from`
navigate and hover through frappe's loader order. `frappe.*` calls inside
expressions resolve, hover and complete like Python, and `references` finds
DocTypes and fields named in templates.

## Scope

- `doc` anchoring where the DocType is known — print formats and web forms,
  whose HTML lives in a DocType JSON field rather than a file. `Unknown`
  elsewhere — no guessing.
- Completion of template paths in `include` and `extends`.
- Whitelisted Jinja globals plus `jinja` hook additions: completion only, no
  diagnostics until the vendored global list is proven exact.
- The checks of [23](../specs/23-diagnostics.md) on templates, once measured
  quiet on the corpus.

## Done when

Template includes navigate across apps, and `{{ doc.field }}` completes in a
print format. Zero diagnostics on `frappe` and `erpnext` templates.
