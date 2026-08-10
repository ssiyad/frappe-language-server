# 012 — Diagnostics

**Phase** 3 · **Depends on** —

[23](../specs/23-diagnostics.md). The riskiest task in the project, and the
first that speaks unprompted about code somebody else wrote.

## Done already

The transport and the half of the check set that cannot be wrong about correct
code: push on `didOpen` / `didChange` / `didSave` with the 150 ms per-buffer
debounce, the pull transport for clients that advertise it, cleared on
`didClose`, stable codes with `codeDescription`, per-rule severity including
`off`, and `# frappe-lsp: ignore [code]` suppression on the line or the one
above.

Checks: `malformed-json`, `unknown-hook`, `unknown-doc-event`,
`non-submittable-event`, `unknown-frequency`, `invalid-cron`, `duplicate-hook`.
Everything decided by a vendored table is gated on the detected Frappe version
matching the vendored snapshot ([31](../specs/31-performance.md)).

Measured: `frappe` 17.0.0-dev and a custom app produce **zero** diagnostics; a
second custom app produces one, and it is a true positive (`app_icon`, dead
config since v13).

Second pass added the code-judging checks: `unknown-doctype`, `unknown-field`,
`unknown-form-event`, `non-whitelisted-call`, `translation-literal`, and the
`duplicate-doctype` / `custom-field-collision` conflicts the index records.

The two absence checks ship **off**, on measurement: 135 hits across 300 files
of `frappe`, falling to 67 once test files are excluded from absence checks, and
the remainder are runtime-assigned attributes and raw table names. With the
shipping defaults that sample produces **one** diagnostic, and it is a real
f-string inside `_()`.

Confidence gating is in: the twelve checks that assert something is *absent*
are suppressed at `Partial` and `Degraded`, so the server no longer reports a
name it merely has not read yet during the first moments of a session.

The schema set is in: `unknown-link-target`, `table-not-child`,
`dangling-dynamic-link`, `unresolvable-fetch-from`, `dangling-depends-on`,
`field-order-mismatch`, `duplicate-fieldname`, `unknown-meta-field`. Measured
across 372 DocType JSONs on the reference bench: 8 findings, 2 of them on
`frappe` and both allowlisted ([32](../specs/32-testing.md)). Three
false-positive sources were removed on the way — `doc.__islocal` and its family
are client-side flags rather than columns, an implicit field is a valid
`fetch_from` target and a `Dynamic Link` target is unknowable, and a field
absent from `field_order` is ordinary because Frappe regenerates that list.

## Scope

- Make the absence checks defensible enough to default on: learn a controller's
  runtime attributes from its own `self.x =` assignments, and recognise the raw
  table names Frappe queries directly (`Singles`). Both are what stands between
  67 and zero.
- The remaining check sets: the hooks remainder (`unresolvable-path`,
  `wrong-path-kind`, `missing-asset`, `app-name-mismatch`,
  `unknown-required-app`) and patches, which needs `patches.txt` parsing that
  nothing does yet.
- Quick fixes, and publishing conflicts against files that are not open.
- `Degraded` confidence, once the bridge exists, and the suppression of absence
  checks under it.
- Quick fixes: close-match replacement (single candidate, edit distance ≤ 2),
  insert `@frappe.whitelist()`. No "fix all".
- Workspace-wide diagnostics for the conflicts the index already records —
  `duplicate-doctype` and `custom-field-collision` land on files that may not be
  open.

## Done when

Every check has a positive **and** a negative test case
([32](../specs/32-testing.md)), and the nightly smoke test reports zero
diagnostics across `frappe` and `erpnext`. A false positive on that corpus blocks
the task.
