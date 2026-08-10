# 004 — Workspace discovery

**Phase** 2 · **Depends on** — · **Blocks** 005

Implements [03](../specs/03-workspace-model.md) in `frappe-index`.

## Scope

- Bench / App / None detection by upward walk.
- App package location by globbing `apps/*/*/hooks.py`; app name from `app_name`,
  **not** from the directory name.
- Installed-app filtering from `sites/apps.json` (preferred — carries `idx`,
  `version`, `branch`) then `sites/apps.txt`. Uninstalled apps are excluded
  entirely, not merely flagged.
- Frappe version detection from `__version__` + `apps.json`
  ([31](../specs/31-performance.md)).
- Module list from `modules.txt`.
- DocType file discovery with the three-part filter (stem = parent dir, parses as
  object, `doctype == "DocType"`).
- Built-in exclusions.
- Artifact discovery: reports, pages, web forms, templates, fixtures, patches.

## Done when

Against the reference bench `~/sources/frappe-dev`:

- `frappe` yields 288 DocTypes from 316 candidate files, rejecting the other 28.
- `apps/resource-management/` (package `frappe_sre`, absent from `apps.txt`) is
  excluded, producing **zero** duplicate-DocType conflicts. Indexing it would
  emit 41 — this is the regression test for the whole rule.
- Detected Frappe version is `17.0.0-dev`.

Against the fixture bench, counts match its README. App-only mode works on a
checkout with no `sites/`.
