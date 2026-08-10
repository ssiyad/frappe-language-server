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

The code column is the registry: it is the complete set, and every one is
configurable with the `frappe-lsp/` prefix dropped ([30](30-configuration.md)).
Two checks share a code only where they mean the same thing at the same
severity — `unknown-doctype` covers every code position naming a DocType, while
a `Link` target in schema JSON gets its own code because it is an Error.

### Hooks — [12](12-surface-hooks.md)

| Check | Code | Severity |
| --- | --- | --- |
| Unknown hook key | `unknown-hook` | Warning |
| Unknown doc event name | `unknown-doc-event` | Error |
| Submit-only event on a non-submittable DocType | `non-submittable-event` | Warning |
| Invalid `scheduler_events` frequency | `unknown-frequency` | Error |
| Invalid cron expression | `invalid-cron` | Error |
| Unresolvable dotted path | `unresolvable-path` | Warning |
| Wrong path kind (class expected, function found) | `wrong-path-kind` | Warning |
| Unknown DocType as a map key | `unknown-doctype` | Warning |
| Missing asset file (`doctype_js`, `app_include_js`) | `missing-asset` | Warning |
| Duplicate assignment to one hook key | `duplicate-hook` | Warning |
| `app_name` not matching the package directory | `app-name-mismatch` | Warning |
| `required_apps` naming an app absent from the workspace | `unknown-required-app` | Warning |

The last two are the app-level checks of [12](12-surface-hooks.md); they read
`hooks.py` but describe the app, not a hook value.

### Cross-language — [11](11-surface-javascript.md)

| Check | Code | Severity |
| --- | --- | --- |
| `frappe.call` to a resolvable method lacking `@frappe.whitelist()` | `non-whitelisted-call` | Error |
| `frappe.ui.form.on` key matching no event and no fieldname | `unknown-form-event` | Warning |
| `frm.set_value` on a nonexistent field | `unknown-field` | Warning |

The first is the flagship check: it is a guaranteed runtime `PermissionError`,
provably wrong, and invisible to every other tool.

### Schema — [04](04-metadata-index.md)

| Check | Code | Severity |
| --- | --- | --- |
| `Link`/`Table` `options` naming an unknown DocType | `unknown-link-target` | Error |
| `Table` `options` naming a non-child DocType | `table-not-child` | Error |
| `Dynamic Link` `options` naming a nonexistent sibling field | `dangling-dynamic-link` | Error |
| `fetch_from` with an unresolvable half | `unresolvable-fetch-from` | Warning |
| `depends_on` referencing a nonexistent sibling field | `dangling-depends-on` | Warning |
| `field_order` disagreeing with `fields` | `field-order-mismatch` | Warning |
| Duplicate `fieldname` | `duplicate-fieldname` | Error |
| `title_field` / `sort_field` / `image_field` nonexistent | `unknown-meta-field` | Warning |
| Same DocType defined in two apps | `duplicate-doctype` | Warning, on both |
| Custom Field colliding with a schema field | `custom-field-collision` | Warning |
| DocType JSON that does not parse | `malformed-json` | Error |

`malformed-json` is the file-level diagnostic of [02](02-architecture.md): the
DocType is dropped, the rest of the app still indexes.

### Python — [10](10-surface-python.md)

| Check | Code | Severity |
| --- | --- | --- |
| Unknown DocType in a literal position | `unknown-doctype` | Warning |
| Unknown field for a resolved DocType | `unknown-field` | Warning |
| Parent-only field used on a child DocType binding | `parent-field-on-child` | Warning |
| `get_single_value` on a non-Single DocType | `not-single-doctype` | Warning |

When the generated stub in [24](24-type-integration.md) is active,
`unknown-field` MUST be suppressed for **attribute access** on bound names —
pyright reports it already, and more accurately. Field positions inside string
literals are unaffected and remain ours. Two servers reporting the same error on
one range is the fastest way to get both of them disabled.

### Translations

Carried over from [`frappe/intellisense`](https://github.com/frappe/intellisense),
which had it right: `_()` and `__()` must receive a literal, because strings are
extracted statically.

| Check | Code | Severity |
| --- | --- | --- |
| f-string inside `_()` | `translation-literal` | Error |
| `.format()` applied inside `_()` rather than outside | `translation-literal` | Error |
| Concatenation inside `_()` | `translation-literal` | Error |
| Non-literal argument to `_()` / `__()` | `translation-dynamic` | Warning |

### Patches — [07](07-dotted-paths.md)

| Check | Code | Severity |
| --- | --- | --- |
| Unresolvable patch module | `unresolvable-patch` | Error |
| Patch listed twice | `duplicate-patch` | Warning |
| Patch module without `execute()` | `patch-without-execute` | Warning |

## Suppression

- `# frappe-lsp: ignore` on the line or the line above.
- `# frappe-lsp: ignore <code>` for one code.
- Per-rule severity, including `off`, in configuration.

Every diagnostic MUST carry its `code` from the tables above, prefixed
(`frappe-lsp/unknown-doctype`), and a `codeDescription` URL. Without a code,
suppression is unusable. Codes are stable once published: renaming one silently
disables every suppression comment that names it.

## Quick fixes

`textDocument/codeAction` where the fix is unambiguous:

- Unknown DocType/field with a close match → replace (edit distance ≤ 2, single
  candidate only).
- Missing `@frappe.whitelist()` → insert the decorator.
- Unknown hook key with a close match → replace.

No bulk "fix all". A wrong bulk fix across a bench is unrecoverable.
