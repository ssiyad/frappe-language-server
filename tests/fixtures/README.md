# Fixture bench

`bench/` is a real directory tree, not mocks — the corpus every level in
[32](../../docs/specs/32-testing.md) asserts against. It is small enough to read
in full and is meant to be read: when a rule in
[03](../../docs/specs/03-workspace-model.md) or a check in
[23](../../docs/specs/23-diagnostics.md) has no artifact here, it has no test.

## The three apps

| Directory | Package | `app_name` | Installed | Role |
| --- | --- | --- | --- | --- |
| `apps/frappe/` | `frappe/` | `frappe` | yes | Stub. Carries `__version__` so version detection ([31](../../docs/specs/31-performance.md)) has a source. Nothing else. |
| `apps/library/` | `library/` | `library` | yes | Correct by construction. **Any diagnostic reported here is a false positive.** |
| `apps/circulation-app/` | `circulation/` | `circulation_app` | yes | Deliberately wrong. Every error is annotated with the code it must produce. |
| `apps/library-old/` | `library_archive/` | `library_archive` | **no** | On disk, absent from `sites/apps.txt`. Must contribute nothing. |

Directory, package and `app_name` differ from each other in
`apps/circulation-app/` on purpose: all three assumptions fail on real benches
([03](../../docs/specs/03-workspace-model.md)).

`library-old` redefines `Library Member` and `Library Settings` — 2 of its 2
DocTypes collide with `library`. Because it is not installed it MUST NOT be
indexed and MUST NOT raise `duplicate-doctype`. This is the reference bench's
41-of-41 conflict in miniature.

## DocTypes

| DocType | App | Exercises |
| --- | --- | --- |
| Library Member | library | `Link` (self), `Table`, `Dynamic Link`, `fetch_from`, `Select` with a leading blank, `depends_on` with `eval:`, `autoname: field:`, `title_field` |
| Library Loan Item | library | Child (`istable`), implicit `parent` / `parentfield` / `parenttype` |
| Library Membership | library | Submittable (`is_submittable`), `amended_from`, `Currency` whose `options` is a fieldname |
| Library Settings | library | Single (`issingle`), `depends_on` naming a plain fieldname |
| Library Availability | library | Virtual (`is_virtual`) with a controller that implements the persistence methods |
| Loan | circulation | Inter-app reference: `Link` and `Table` into `library` |
| Circulation Broken Field | circulation | Field-level errors, beside their near misses |
| Circulation Broken Meta | circulation | Document-level errors |
| Circulation Malformed | circulation | JSON that does not parse |
| Library Availability | circulation | The same name as `library`'s, in a second **installed** app |
| Library Member, Library Settings | library-old | Duplicate names in an uninstalled app |

`Library Availability` exists twice on purpose, and the two copies are the pair
of cases that must not be confused: installed twice is `duplicate-doctype` on
both files, installed once with a stale copy on disk is silence.

## Discovery cases

| Artifact | Asserts |
| --- | --- |
| `library_member/test_records.json` | Rejected: stem ≠ parent directory |
| `library_member/member_activity_report.json` | Rejected for the same reason, and it is not a `DocType` record either |
| `library/public/node_modules/leftpad/index.js` | `node_modules/` is never traversed — it holds a form script that must never be indexed |
| `sites/assets/library/js/library.bundle.js` | Build output. `sites/` is entered only for `apps.txt` and `*/site_config.json` |
| `sites/apps.txt` | **No trailing newline.** The parser must tolerate it |
| `sites/apps.json` | Per-app `idx`, `version`, `resolution.branch` / `commit_hash`; keyed by `app_name` |
| `library/modules.txt` | No trailing newline; `circulation/modules.txt` has one |
| `sites/library.localhost/site_config.json` | Bridge target. Credentials in it MUST NOT be read ([30](../../docs/specs/30-configuration.md)) |
| `.frappe-lsp.toml` | Committed team-wide configuration, source 1 of [30](../../docs/specs/30-configuration.md) |

## Layers

| File | Layer | Effect |
| --- | --- | --- |
| `library/fixtures/custom_field.json` | 2 | Adds `loyalty_points` to Library Member and `renewal_of` to Library Membership |
| `library/fixtures/property_setter.json` | 2 | Overrides `email_id.reqd`, and a DocType-level `search_fields` (empty `field_name`) |
| `circulation/fixtures/custom_field.json` | 2 | Custom Field `member_name` on Library Member — collides with a schema field |

`library/api.py` reads `loyalty_points` through `frappe.db.get_value`. That
call is only clean if Layer 2 was merged, so it is the load-bearing test that
the merge happened at all.

## Hooks an app invents

`library/hooks.py` sets `loan_policies` and `member_greeters`, which no vendored
table knows. `library/policy.py` reads them back — the first through
`frappe.get_hooks("key")`, the second off the `_dict` that a bare `get_hooks()`
returns. Both keys MUST stay silent: an app-defined hook is as real as a
built-in one ([12](../../docs/specs/12-surface-hooks.md)), and the two files are
the negative case for `unknown-hook` that
[32](../../docs/specs/32-testing.md) requires.

## Annotations

Sources that take comments carry the expectation inline:

```python
frappe.get_doc("Libary Member", name)  # expect: unknown-doctype
frappe.get_doc(doctype, name)          # expect: clean, not a literal
```

`expect: <code>` names a code from the registry in
[23](../../docs/specs/23-diagnostics.md); `expect: clean` marks a near miss that
MUST stay silent. `tests/fixture_bench.rs` in `frappe-index` fails if an
annotation names a code the spec does not define — the fixture and the registry
cannot drift apart.

JSON takes no comments, so those expectations live here:

| File | Position | Code |
| --- | --- | --- |
| `circulation_broken_field.json` | `unknown_link` | `unknown-link-target` |
| | `wrong_table` | `table-not-child` |
| | `dangling_dynamic` | `dangling-dynamic-link` |
| | `bad_fetch` | `unresolvable-fetch-from` |
| | `bad_depends` | `dangling-depends-on` |
| | `good_link`, `good_dynamic`, `good_fetch` | clean |
| `circulation_broken_meta.json` | `title` twice | `duplicate-fieldname` |
| | `field_order` names `ghost_field` | `field-order-mismatch` |
| | `orphan`, absent from `field_order` | clean — Frappe regenerates `field_order`, and its own schema ships fields missing from it |
| | `title_field: no_such_field` | `unknown-meta-field` |
| | `sort_field: also_missing` | `unknown-meta-field` |
| `circulation_malformed.json` | file | `malformed-json` |
| `circulation/**/library_availability.json` | the DocType, and `library`'s copy | `duplicate-doctype` |
| `circulation/fixtures/custom_field.json` | `member_name` | `custom-field-collision` |
| `library_member.json` | `sort_field: creation` | clean — `creation` is an implicit field |
| `library-old/**` | both DocTypes | clean — the app is not installed |

## Byte shape

Every DocType JSON is written the way `frappe` writes it, so a fixture and an
exported file are indistinguishable:

```python
# frappe/modules/export_file.py + frappe/__init__.py
json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n"
```

One space of indent, keys sorted, ASCII only, trailing newline.
`tests/fixture_bench.rs` re-serializes each file and compares bytes, so a
hand-edit that drifts from the format fails the build. To put a file back:

```
python3 -c 'import json,sys
p = sys.argv[1]
json.dump(json.load(open(p)), open(p, "w"), indent=1, sort_keys=True)
open(p, "a").write("\n")' <file>
```

## Deliberately absent

- **Vendored `frappe`.** The stub carries a version and nothing else, so no
  fixture names a core DocType (`User`, `Custom Field`, `DocType`) in a checked
  position. Real frappe is covered by the smoke test in
  [32](../../docs/specs/32-testing.md), which is the only honest way to test
  against it.
- **Templates beyond `www/books.html`.** The Jinja surface
  ([13](../../docs/specs/13-surface-jinja.md)) arrives with its own task and
  will bring the corpus it needs.
