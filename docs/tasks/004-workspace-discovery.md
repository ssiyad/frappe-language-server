# 004 — Workspace discovery

**Phase** 2 · **Depends on** 003 · **Blocks** 005

Implements [03](../specs/03-workspace-model.md) in `frappe-index`.

## Scope

- Bench / App / None detection by upward walk.
- App enumeration from `sites/apps.txt`, fallback to directory scan, ordering
  preserved — [04](../specs/04-metadata-index.md) precedence depends on it.
- Module list from `modules.txt`.
- DocType file discovery with the three-part filter (stem = parent dir, parses as
  object, `doctype == "DocType"`).
- Built-in exclusions.
- Artifact discovery: reports, pages, web forms, templates, fixtures, patches.

## Done when

Against a real `frappe` checkout, discovery finds 288 DocTypes from 316 candidate
files and rejects the other 28. Against the fixture bench, counts match the
README. App-only mode works on a checkout with no `sites/`.
