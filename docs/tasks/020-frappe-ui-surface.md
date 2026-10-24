# 020 — frappe-ui surface

**Phase** 6 · **Depends on** — · **Blocks** nothing

Apps such as press, helpdesk and gameplan ship a frappe-ui dashboard outside
the app package, under `<app>/dashboard/` or `<app>/frontend/`. It names
DocTypes and fields as strings, as desk JavaScript does, and nothing reads it.
On press, 4 of the 32 sites that name Site Update's `update_job` are in
`dashboard/src`, and references say so but cannot list them.

## Scope

- A surface spec in the 10–19 range: which calls name a DocType and its fields
  (`createResource`, `createListResource`, `createDocumentResource`,
  `useList`, `useDoc`, and the `doctype`, `fields`, `filters` and `orderBy`
  keys of their options), and which files count.
- Vue single-file components: the `<script>` block only, read as JavaScript.
- `.ts` files, if the JavaScript grammar's reading of them holds up; otherwise
  a grammar decision recorded in [06](../specs/06-analysis-pipeline.md).
- References first. Diagnostics follow only where the precision bar of
  [23](../specs/23-diagnostics.md) holds on press and helpdesk.

## Done when

- `frappe-lsp references "Site Update" update_job` on press lists
  `dashboard/src/objects/site.js:755`, `:756` and
  `dashboard/src/pages/JobPage.vue:161`.
- `check` over press's and helpdesk's dashboards reports nothing that is not a
  real missing field, checked by hand.
- The `covers` note of [30](../specs/30-configuration.md) no longer names
  frappe-ui dashboards.
