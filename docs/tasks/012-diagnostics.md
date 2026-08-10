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

The hooks remainder and the patch set are in: `unresolvable-path`,
`wrong-path-kind`, `missing-asset`, `app-name-mismatch`, `unknown-required-app`,
`unresolvable-patch`, `duplicate-patch`, `patch-without-execute`, plus
`unknown-doctype` on hook map keys. `patches.txt` gained a parser
([07](../specs/07-dotted-paths.md)'s line forms, `execute:` opaque).

Five false-positive sources were removed on the way: literal strings — a
licence, an email, a website route — were being resolved as dotted paths; bare
`*.bundle.js` names resolve to their source under `public/js`; a `doc_events`
value may be a `Class.method` and a `jinja` value a module *or* a function, so
only a path no reading accepts is reported; a symbol behind a re-export is
unknowable, so only a missing *module* is; and a file belonging to an app the
index excluded is not judged at all.

Quick fixes are in for the three replacement cases — `unknown-doctype`,
`unknown-field`, `unknown-hook` — offered only where exactly one candidate
exists, and never as a bulk action. Diagnostic ranges now cover the string
interior rather than the quotes, matching hover and definition; the quotes had
been part of the range, which is both wrong to underline and what a replacement
would have overwritten.

Triaged to **7 findings on the reference bench, 5 of them true**: `app_icon`,
and frappe's own `override_whitelisted_methods`, which maps two paths that
define no `download_file` to the bare string `"download_file"` — dead config
this check exists to find. The other 2 are allowlisted in
[32](../specs/32-testing.md). Three `.css` bundles were a false positive and are
fixed: a `.css` bundle is compiled from `.scss`, so the name in the hook exists
nowhere on disk.

The re-export guard is behaving as written; what confused it is one line above.
`unresolvable-path` resolves `index.effective_method(dotted)` first, so a path
that `override_whitelisted_methods` remaps is judged by its *replacement*: the
guard was asked about `download_file`, a bare name with no module at all,
rather than about `frappe.utils.file_manager.download_file`. The finding is
still a true positive — that bare string is not a path — but the message names
the key while the failure is in the value, and a key that resolves fine would
be reported for its replacement's sake. Judging the two on their own terms is
the fix.

## Scope

- The last three codes in the registry: `not-single-doctype`,
  `parent-field-on-child`, and wiring `translation-dynamic` — it is defined and
  configurable but its extraction arm returns nothing, so turning it on today
  would do nothing at all. The first two need resolution to say what it cannot
  yet: that a `get_single_value` anchor is not a Single, and that a fieldname
  belongs to the parent of the child a name is bound to.
- The `@frappe.whitelist()` quick fix. Unlike the three replacements, its edit
  lands in a *different* file from the diagnostic — the Python definition —
  which is the only reason it is not done.
- Publishing conflicts against files that are not open. `duplicate-doctype` and
  `custom-field-collision` land on two files by definition, and only the open
  one sees them.
- `Degraded` confidence and the suppression of absence checks under it, once
  [014](014-live-bridge.md) gives that state a way to occur.
- `unresolvable-path` judging a hook key by its `override_whitelisted_methods`
  replacement rather than on its own terms — see above.

## Absence checks — on

`unknown-doctype`, `unknown-field` and `unknown-form-event` ship **on**.
`unknown-attribute` is a new code for the same claim reached as `doc.x`, and it
ships **off**: a `Document` is an open Python object, and telling a typo from a
runtime attribute needs type information this server does not have
([23](../specs/23-diagnostics.md)).

Turning them on took reducing the corpus from 2 524 findings to 459, then
splitting the residue by how the fieldname was written. Nine fixes, each one a
rule the checks did not know: `self` binding outside the controller class,
`self.x =` on the class and its bases, grid events registered against the
child, handlers called through `frm.trigger` and `frm.events`, the legacy
three-argument `form.on`, hook map keys that name no DocType, frappe's own
tables, `locals[":Company"]`, and `dist/`. All of them are in the specs and
have a negative case in the fixture bench.

What remains is on the allowlist with a reason, and the honest summary is in
[32](../specs/32-testing.md): 214 entries, of which 160 are the absence checks.
Most of the 33 `unknown-doctype` would disappear on a bench that installs the
apps erpnext links into.

## Corpus check — done

`cargo xtask smoke` runs every check over `frappe` and `erpnext` and fails on
anything not in `tests/smoke-allowlist.toml`. It currently passes: 7 079 files,
54 allowed findings, nothing else. Its first run found 36 false positives, all
fixed — the registry now unions the vendored hook table with every key an app
reads back, and `dist/` is no longer walked. Details and the allowlist contents
in [32](../specs/32-testing.md).

Precision is now measured on every run rather than by hand once, which is what
the rest of this task needed before it could move.

## Done when

Every check has a positive **and** a negative test case
([32](../specs/32-testing.md)), and `cargo xtask smoke` stays clean. A false
positive on that corpus blocks the task.
