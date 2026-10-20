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

A dotted path that does not resolve is an absence check: the module may not be
read yet. `unresolvable-path` and `unresolvable-patch` are gated with the rest.
Ungated, a `hooks.py` opened at startup got one warning per path until the
build finished.

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

`unknown-hook` fires on a key that is in neither the vendored table nor the set
an app reads back through `get_hooks` ([12](12-surface-hooks.md)), and never on
a name Python convention marks as a constant — a leading underscore or ALL_CAPS
says the author meant a helper, and reporting those would accuse every local in
the file. Edit distance is not the discriminator: `doctype_js` written as
`doctype_javascript` is eight edits away and is exactly the mistake worth
catching, so a near miss is offered as a suggestion rather than used as the
test.

### Cross-language — [11](11-surface-javascript.md)

| Check | Code | Severity |
| --- | --- | --- |
| `frappe.call` to a resolvable method lacking `@frappe.whitelist()` | `non-whitelisted-call` | Error |
| `frappe.ui.form.on` key matching no event, fieldname, grid event or trigger | `unknown-form-event` | Warning |
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
| Unknown field, written as a name | `unknown-field` | Warning |
| Unknown field, reached as `doc.x` | `unknown-attribute` | **Off** |
| Parent-only field named on a child DocType | `parent-field-on-child` | Warning |
| `get_single_value` on a non-Single DocType | `not-single-doctype` | Warning |

`parent-field-on-child` is `unknown-field` with a better message: the field
exists, on the DocType that holds this one as a `Table`. It is held to the same
line — a name in a query position is a claim, `row.company` is not. erpnext
copies parent values onto rows constantly, and 106 of these on the corpus are
that idiom rather than a bug, so attribute access keeps the message and takes
the quieter code.

`not-single-doctype` covers `frappe.get_single`, `frappe.db.get_single_value`
and `frappe.db.set_single_value`. It is not an absence check: `issingle` is in
the JSON, so a DocType that resolves at all answers the question.

#### A name is a claim; an attribute is not

The two field checks are split by how the fieldname was written, because the
two forms say different things.

A fieldname written as a **string** — `frappe.db.get_value("Loan", name, "x")`,
`fields=["x"]`, `frm.set_value("x", …)`, `set_query("x", …)` — is a claim
frappe acts on. It becomes a column in a query, and a name that is not a field
silently returns nothing. That is worth reporting, and `unknown-field` ships
**on**.

A key in `frappe.get_doc({"doctype": D, …})` is the exception. Frappe copies
every key onto the document, field or not, so a key that is not a field is an
attribute handed in. It is reported only when nothing reads it back: not the
controller, its overrides and extensions, nor any class they derive from, as
`self.x`, `hasattr(self, "x")` or `getattr(self, "x")`. press's `Site` takes
`app_plans` that way.

`doc.x` is Python being Python. A `Document` is an open object: controllers
hang working state off `self`, `update()` attaches whatever a dict holds, and
`as_dict` rows carry joined columns that belong to no schema. Deciding whether
`doc.x` is a typo or a runtime attribute needs type information this server
does not have, so `unknown-attribute` ships **off** and stays available for
anyone who wants it.

Measured over `frappe` and `erpnext` — 7 079 files — with everything else the
implementation already knows: 127 named-position findings, against 261 on
attribute access. Sampled against the schemas, the named ones are dead
references: `Web Form` has no `amount_field`, `Contact` no
`is_billing_contact`, `Item` no `page_name`. The attribute ones are dominated
by `Email Digest.future_to_date` and `Payment Entry.company_currency`, which
exist at runtime and in no JSON.

What the check has to know before it can be quiet is written down in
[04](04-metadata-index.md) and [11](11-surface-javascript.md): a controller's
own `self.x =` assignments and those of every class it derives from; grid row
events, which are registered against the child with the parent's fieldname;
handlers reached by `frm.trigger` and `frm.events`; frappe's own tables
(`Singles`, `Series`, `__global_search`), which are not DocTypes; and
`locals[":Company"]`, which is. Each of those was a false positive before it
was a rule.

Test files are excluded from absence checks — a test creates its DocTypes at
runtime, so absence is unknowable in a file whose job is to invent the thing
being asked about. Whether a file is a test is judged **inside the app**, never
on the absolute path: a bench can be cloned anywhere, and reading the prefix
silences a whole workspace for where its owner happened to put it.

When the generated stub in [24](24-type-integration.md) is active,
`unknown-attribute` MUST stay off regardless of configuration — pyright reports
attribute access already, and more accurately. `unknown-field` is unaffected
and remains ours. Two servers reporting the same error on one range is the
fastest way to get both of them disabled.

### Translations

Carried over from [`frappe/intellisense`](https://github.com/frappe/intellisense),
which had it right: `_()` and `__()` must receive a literal, because strings are
extracted statically.

`translation-dynamic` ships **off**, and is configurable like every other code.
Measured against `frappe` 17.0.0-dev it fires 721 times, almost all of it `_(variable)` in code that is doing nothing
wrong; `translation-literal` fires 8 times on the same corpus, and those 8 are
real. A check that fires 721 times on the reference corpus fails the governing
rule above, whatever its merits as advice — it stays defined and configurable,
and nobody has to disable the server to escape it.

Two forms look like violations and are not, and both cost false Errors on
`frappe` before they were excluded: Python implicit adjacent concatenation
(`_("one " "string")`) is a single literal to the language and to the
extractor, and a JavaScript fallback (`__(label || "ID")`) is not a
concatenation. Only `+` concatenates.

| Check | Code | Severity |
| --- | --- | --- |
| f-string inside `_()` | `translation-literal` | Error |
| `.format()` applied inside `_()` rather than outside | `translation-literal` | Error |
| Concatenation inside `_()` | `translation-literal` | Error |
| Non-literal argument to `_()` / `__()` | `translation-dynamic` | **Off** |

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
