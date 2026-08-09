# 23 — Diagnostics

`textDocument/publishDiagnostics`. The riskiest feature in the project: Frappe is
dynamic enough that a naive checker fires constantly on correct code, and users
disable the server rather than the rule.

## Governing rule

**Report only what is certainly wrong.** Every check MUST justify why it cannot
fire on valid code. When in doubt, emit nothing. Recall is negotiable; precision
is not.

## Confidence gating

From [05](05-live-bridge.md). Checks that assert the *absence* of something are
gated on the index being complete enough to know.

| Confidence | When | Absence checks |
| --- | --- | --- |
| `Full` | Bridge enabled and synced | Enabled |
| `Static` | No bridge configured | Enabled at `Warning`; customizations invisible by user's choice |
| `Degraded` | Bridge configured but unreachable | **Suppressed** |
| `Partial` | Initial index incomplete | **Suppressed** |

Presence checks — a wrong event name, an invalid cron expression — are never
gated, because no amount of extra metadata could make them valid.

### Version gating

Separately, any check that asserts invalidity from a **vendored list** (hook
keys, doc events, scheduler frequencies, form events, fieldtypes) fires only when
the detected Frappe version exactly matches a vendored snapshot
([31](31-performance.md)). Otherwise the value degrades to "unknown" and reports
nothing. Cron syntax, duplicate keys and path resolution are unaffected — they
are checkable without a version.

## Checks

Severity defaults; all individually configurable ([30](30-configuration.md)).

### Hooks — [12](12-surface-hooks.md)

| Check | Severity |
| --- | --- |
| Unknown hook key | Warning |
| Unknown doc event name | Error |
| Submit-only event on a non-submittable DocType | Warning |
| Invalid `scheduler_events` frequency | Error |
| Invalid cron expression | Error |
| Unresolvable dotted path | Warning |
| Wrong path kind (class expected, function found) | Warning |
| Unknown DocType as a map key | Warning |
| Missing asset file (`doctype_js`, `app_include_js`) | Warning |
| Duplicate assignment to one hook key | Warning |

### Cross-language — [11](11-surface-javascript.md)

| Check | Severity |
| --- | --- |
| `frappe.call` to a resolvable method lacking `@frappe.whitelist()` | Error |
| `frappe.ui.form.on` key matching no event and no fieldname | Warning |
| `frm.set_value` on a nonexistent field | Warning |

The first is the flagship check: it is a guaranteed runtime `PermissionError`,
provably wrong, and invisible to every other tool.

### Schema — [04](04-metadata-index.md)

| Check | Severity |
| --- | --- |
| `Link`/`Table` `options` naming an unknown DocType | Error |
| `Table` `options` naming a non-child DocType | Error |
| `Dynamic Link` `options` naming a nonexistent sibling field | Error |
| `fetch_from` with an unresolvable half | Warning |
| `depends_on` referencing a nonexistent sibling field | Warning |
| `field_order` disagreeing with `fields` | Warning |
| Duplicate `fieldname` | Error |
| `title_field` / `sort_field` / `image_field` nonexistent | Warning |
| Same DocType defined in two apps | Warning, on both |
| Custom Field colliding with a schema field | Warning |

### Python — [10](10-surface-python.md)

| Check | Severity |
| --- | --- |
| Unknown DocType in a literal position | Warning |
| Unknown field for a resolved DocType | Warning |
| Parent-only field used on a child DocType binding | Warning |
| `get_single_value` on a non-Single DocType | Warning |

When the generated stub in [24](24-type-integration.md) is active,
`unknown-field` MUST be suppressed for **attribute access** on bound names —
pyright reports it already, and more accurately. Field positions inside string
literals are unaffected and remain ours. Two servers reporting the same error on
one range is the fastest way to get both of them disabled.

### Translations

Carried over from [`frappe/intellisense`](https://github.com/frappe/intellisense),
which had it right: `_()` and `__()` must receive a literal, because strings are
extracted statically.

| Check | Severity |
| --- | --- |
| f-string inside `_()` | Error |
| `.format()` applied inside `_()` rather than outside | Error |
| Concatenation inside `_()` | Error |
| Non-literal argument to `_()` / `__()` | Warning |

### Patches — [07](07-dotted-paths.md)

Unresolvable patch module (Error), duplicate patch entry (Warning), patch module
without `execute()` (Warning).

## Suppression

- `# frappe-lsp: ignore` on the line or the line above.
- `# frappe-lsp: ignore <code>` for one code.
- Per-rule severity, including `off`, in configuration.

Every diagnostic MUST carry a stable `code` (`frappe-lsp/unknown-doctype`) and a
`codeDescription` URL. Without a code, suppression is unusable.

## Quick fixes

`textDocument/codeAction` where the fix is unambiguous:

- Unknown DocType/field with a close match → replace (edit distance ≤ 2, single
  candidate only).
- Missing `@frappe.whitelist()` → insert the decorator.
- Unknown hook key with a close match → replace.

No bulk "fix all". A wrong bulk fix across a bench is unrecoverable.
