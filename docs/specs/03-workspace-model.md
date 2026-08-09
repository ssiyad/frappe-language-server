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

Preferred order from `sites/apps.txt` (newline-delimited, install order). Apps
present on disk but absent from `apps.txt` are appended in directory order and
marked `installed: false`. Order matters — it decides override precedence in
[04](04-metadata-index.md).

An app's Python package is `apps/<app>/<app>/`. Modules come from
`apps/<app>/<app>/modules.txt`; a module's directory is its scrubbed name.

## DocType discovery

Glob `apps/<app>/<app>/**/doctype/*/*.json`, then require **all** of:

1. basename stem equals its parent directory name;
2. parses as a JSON object;
3. `doctype == "DocType"`.

This rejects `test_records.json` and the report/fixture JSON that share the
directory. Verified against `frappe` develop: 316 candidate files, 288 DocTypes,
28 correctly rejected.

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
| Fixtures | `<app>/fixtures/*.json` | Layer 2 — [04](04-metadata-index.md) |
| Templates | `<app>/templates/`, `<app>/**/templates/`, `<app>/www/` | [13](13-surface-jinja.md) |
| Public assets | `<app>/public/js/**`, `*.bundle.js` | Targets of `app_include_js` |

## Exclusions

Never traverse: `node_modules/`, `.git/`, `env/`, `sites/assets/`,
`__pycache__/`, `*.egg-info/`, `sites/*/private/`, `sites/*/public/files/`.

`sites/` is traversed only for `apps.txt` and `*/site_config.json`.

## Site configuration

`sites/<site>/site_config.json` supplies the bridge target ([05](05-live-bridge.md)).
`sites/currentsite.txt` names the default. Credentials MUST NOT be read from
these files by default — see [30](30-configuration.md).
