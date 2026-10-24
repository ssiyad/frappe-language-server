# 03 — Workspace model

How the server locates and interprets the workspace. Consumed by
[04](04-metadata-index.md).

## Detection

Walk up from the LSP root URI until a directory matches, first match wins:

| Mode | Test | Indexes |
| --- | --- | --- |
| `Bench` | contains `apps/` and `sites/` | every app in `apps/` |
| `App` | contains `<name>/hooks.py` with `pyproject.toml`/`setup.py` | this app; siblings via `../` if that parent is a bench |
| `None` | neither | nothing Frappe-specific |

Stop at the filesystem root or a `.git` boundary that fails both tests.

## App enumeration

The directory name under `apps/` is **not** the app name, and the package inside
is **not** named after it. Both assumptions fail on real benches.

Find the package by globbing `apps/*/*/hooks.py`; its parent directory is the
package, and the app name is the `app_name` assignment inside. Observed on a
development bench:

| Directory | Package | `app_name` |
| --- | --- | --- |
| `apps/frappe/` | `frappe/` | `frappe` |
| `apps/resource-management/` | `frappe_sre/` | `frappe_sre` |
| `apps/resource_management/` | `resource_management/` | `resource_management` |

### Installed apps are authoritative

An app on disk is indexed only if it is installed. Two sources, preferred in
order:

1. `sites/apps.json` — richest: per-app `idx` (install order), `version`,
   and `resolution.branch` / `commit_hash`. Feeds version detection in
   [31](31-performance.md).
2. `sites/apps.txt` — newline-delimited, install order. May lack a trailing
   newline; the parser MUST tolerate that.

Apps on disk but absent from both are recorded as `installed: false` and
**excluded from the index**. They MUST NOT contribute DocTypes, and MUST NOT
raise the duplicate-definition diagnostic in [04](04-metadata-index.md).

This is not hygiene. On the reference bench, `apps/resource-management/`
(package `frappe_sre`) is a stale rename of `apps/resource_management/`, and the
two define **41 of 41 identical DocType names**. Indexing both would emit 41
false conflicts before the user typed anything. `apps.txt` lists only the current
one.

Order matters beyond exclusion — it decides override precedence in
[04](04-metadata-index.md).

Modules come from `<package>/modules.txt`; a module's directory is its scrubbed
name.

## DocType discovery

Glob `<package>/**/doctype/*/*.json`, then require **all** of:

1. basename stem equals its parent directory name;
2. parses as a JSON object;
3. `doctype == "DocType"`.

This rejects `test_records.json` and the report/fixture JSON that share the
directory. Measured against `frappe` 17.0.0-dev: 314 candidate files, 286
DocTypes, 28 rejected — every one of them on rule 1 alone.

The three rules generalize: every exported record lives at
`<kind>/<name>/<name>.json` where `<kind>` is `scrub` of the record's own
DocType, so one filter finds DocTypes, Reports, Pages and the rest. Rule 3 for a
DocType is that rule with `<kind>` = `doctype`.

Sibling files, all optional:

| File | Role |
| --- | --- |
| `<x>.py` | Controller — `Document` subclass |
| `<x>.js` | Form script |
| `<x>_list.js` | List view settings |
| `<x>_calendar.js`, `<x>_dashboard.py`, `<x>_tree.js` | View extensions |
| `test_<x>.py`, `test_records.json` | Tests |

The module is taken from the JSON `module` key, not the path — apps do nest
DocTypes below their module directory.

## Other artifacts

| Artifact | Path | Notes |
| --- | --- | --- |
| Hooks | `<app>/hooks.py` | Module-level assignments — [12](12-surface-hooks.md) |
| Patches | `<app>/patches.txt` | Dotted paths, `[post_model_sync]` sections |
| Report | `<module>/report/<x>/<x>.{json,py,js}` | `report_type` decides which files exist |
| Page | `<module>/page/<x>/<x>.{json,js,css}` | |
| Web form / Print format / Workspace / Dashboard chart | `<module>/<kind>/<x>/<x>.json` | Fixture-shaped |
| Any other exported record | `<module>/<kind>/<x>/<x>.json` | Same rule; `<kind>` is `scrub` of the record's own DocType |
| Fixtures | `<app>/fixtures/*.json` | Layer 2 — [04](04-metadata-index.md) |
| Customizations | `<module>/custom/*.json` | Layer 2 — [04](04-metadata-index.md) |
| Templates | `<app>/templates/`, `<app>/**/templates/`, `<app>/www/` | [13](13-surface-jinja.md) |
| Public assets | `<app>/public/js/**`, `*.bundle.js` | Targets of `app_include_js` |

## Exclusions

Never traverse: `node_modules/`, `.git/`, `env/`, `.venv/`, `sites/assets/`,
`__pycache__/`, `*.egg-info/`, `dist/`, `sites/*/private/`,
`sites/*/public/files/`.

`dist/` is esbuild's output — frappe's own `.gitignore` names it. The bundles
under it are minified copies of sources already indexed, and reading them
reports the minifier's rewrites as the author's mistakes ([32](32-testing.md)).

`sites/` is traversed only for `apps.txt` and `*/site_config.json`.

## Site configuration

`sites/<site>/site_config.json` supplies the bridge target ([05](05-live-bridge.md)).
`sites/currentsite.txt` names the default. Credentials MUST NOT be read from
these files by default — see [30](30-configuration.md).
