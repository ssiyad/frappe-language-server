# 01 — Glossary

Terms used normatively across specs.

## Frappe

| Term | Meaning |
| --- | --- |
| Bench | Workspace root holding `apps/`, `sites/`, `env/` |
| App | Python package under `apps/<dir>/`, located by its `hooks.py`; named by `app_name`, not by `<dir>` |
| Module | Grouping dir inside an app, listed in `modules.txt` |
| DocType | Model+view schema; `<module>/doctype/<scrubbed>/<scrubbed>.json` |
| Controller | `Document` subclass in `<scrubbed>.py` beside the JSON |
| Child DocType | `istable: 1`; rows owned by a parent via `Table` field |
| Single | `issingle: 1`; one row, stored in `tabSingles` |
| Virtual | `is_virtual: 1`; no table, controller supplies data |
| Field | Entry in `fields[]`, keyed by `fieldname` |
| Link field | `fieldtype: "Link"`, `options` = target DocType |
| Dynamic Link | `options` names a sibling field holding the target DocType |
| Custom Field | DB-stored field added to an existing DocType |
| Property Setter | DB-stored override of one DocType/field property |
| Scrub | `frappe.scrub()` — lowercase, `[ -]` → `_` |
| Unscrub | Inverse, used for display; not round-trip safe |
| Dotted path | `app.module.file.function`, resolved by import |
| Whitelisted method | `@frappe.whitelist()`, callable over HTTP |
| Hook | Module-level assignment in `hooks.py` |
| Doc event | Lifecycle callback registered via `doc_events` |
| Fixture | DocType records exported to `<app>/fixtures/*.json` |
| Patch | Migration entry in `patches.txt` |

## Project

| Term | Meaning |
| --- | --- |
| Index | In-memory metadata graph — [04](04-metadata-index.md) |
| Layer | One contributor of metadata, merged by precedence — [04](04-metadata-index.md) |
| Bridge | Optional live site connection — [05](05-live-bridge.md) |
| Surface | A syntactic pattern set for one language — [10](10-surface-python.md)–[13](13-surface-jinja.md) |
| Anchor | Resolved DocType at a cursor position, driving field completion |
| Confidence | Whether the index is complete enough to assert absence — [23](23-diagnostics.md) |

## Notation

Specs use RFC 2119 keywords: MUST, SHOULD, MAY.
