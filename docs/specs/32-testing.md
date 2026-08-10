# 32 — Testing

The pipeline in [02](02-architecture.md) splits into three independently testable
stages. Test each at its own level; do not drive everything through the protocol.

## Levels

| Level | Covers | Needs |
| --- | --- | --- |
| Unit — extraction | `frappe-syntax` queries; source → matches | Source snippet only |
| Unit — resolution | Index construction, layer merge, path resolution | Fixture bench |
| Golden — features | Completion, hover, definition, diagnostics | Fixture bench |
| Integration | Protocol conformance, lifecycle, cancellation | Server over stdio |
| Benchmark | [31](31-performance.md) budgets | Synthetic + real bench |

## Fixture bench

`tests/fixtures/bench/` — a real directory tree, not mocks. Small, hand-written,
committed, covering: two apps with an inter-app DocType reference, a child
DocType, a Single, a submittable DocType, a virtual DocType, Link and Dynamic
Link and `fetch_from` fields, a `hooks.py` exercising every value schema from
[12](12-surface-hooks.md), a `patches.txt`, a fixtures file carrying a Custom
Field, and one deliberately malformed JSON.

Deliberate errors belong in the fixture. Half the checks in
[23](23-diagnostics.md) can only be tested against code that is actually wrong.

## Golden tests

Cursor markers in fixture sources, expectations beside them:

```python
frappe.get_all("Sales Invoice", fields=["cust|"])
#                                            ^ completion
```

Snapshot the result with `insta`. Reviewing a snapshot diff is how a behaviour
change gets noticed; that is the point of the format.

Each check in [23](23-diagnostics.md) needs both directions:

- a positive case that MUST fire, and
- a **negative case that MUST NOT** — the dynamic-argument forms from
  [06](06-analysis-pipeline.md), `Anchor::Unknown` positions, and valid code that
  merely resembles the error.

The negative cases are the ones that keep the governing rule in
[23](23-diagnostics.md) honest. Treat a missing negative case as an incomplete
test, and never accept a new check without one.

## Integration

Drive a real server process over stdio: `initialize` → `didOpen` → request →
assert → `shutdown`. Covers capability negotiation, `positionEncoding` handling,
incremental sync correctness, cancellation, and the custom methods in
[30](30-configuration.md). `frappe-lsp/indexStatus` makes these assertions
possible without reaching inside the process.

## Real-bench smoke test

`cargo xtask smoke [bench]` indexes a bench, runs every check over every `.py`,
`.js`, DocType JSON, `hooks.py` and `patches.txt` in it, and asserts:

- the index builds without panic;
- DocTypes are indexed at all;
- **zero diagnostics**, save for a committed allowlist.

Only `frappe` and `erpnext` are checked. A developer's bench holds their own
apps too, and findings there are theirs, not this repository's; `--all` widens
it for a local look.

Frappe's own code is the best available corpus of valid Frappe code. Any
diagnostic it produces is, by default, a false positive — and this job is what
turns [23](23-diagnostics.md)'s precision rule into something enforced rather
than merely stated.

### The allowlist

`tests/smoke-allowlist.toml`. Each entry names a file, a code, the message
verbatim, how many times it may fire there, and the reason:

```toml
[[allow]]
file = "apps/frappe/frappe/hooks.py"
code = "unresolvable-path"
message = "`download_file` does not resolve in this workspace"
count = 2
reason = "the dotted path defines no such attribute; frappe's own \
          override_whitelisted_methods carries two of these"
```

Matching on the message and the count rather than on the code is what keeps the
list honest: `hooks.py` alone holds twelve allowed `unknown-hook` lines, and a
thirteenth is news. An entry without a reason is a false positive wearing a
disguise, and the list must not grow to accommodate a noisy check. Anything not
on it fails the job. `--write` regenerates the file from what the corpus
currently produces; the reasons it writes are per-code and are reviewed by hand
before the file is committed.

Entries that no longer fire are printed but do not fail — the corpus moves —
and are pruned when noticed.

### What is on it

Measured with the shipping defaults over `frappe` `5a94ce8` and `erpnext`
`02118e7`: 863 DocTypes, 14 303 fields, 7 079 files checked, **54 allowed
findings** and nothing else. By code:

| Code | Entries | Why they are allowed |
| --- | --- | --- |
| `translation-literal` | 16 | f-strings, concatenation and `.format()` inside `_()`; the string never reaches the catalogue |
| `unknown-hook` | 12 | erpnext hooks with no reader anywhere in the bench — `app_icon`, `bot_parsers`, `source_link`, `payment_gateway_enabled` and nine more |
| `dangling-depends-on` | 10 | names a field no schema declares: a leftover, or a Customize Form field only the live site can see |
| `unresolvable-path` | 4 | frappe's `override_whitelisted_methods` maps two paths that define no `download_file`, and a bare name that is not a path |
| `unresolvable-fetch-from` | 4 | the source field is absent from the linked DocType, so the fetch silently stays empty |
| `unresolvable-patch` | 3 | `patches.txt` names three modules that are not in the checkout |
| `missing-asset` | 3 | absent from the app, or served from a DocType at runtime |
| `duplicate-patch` | 1 | listed twice |
| `unknown-link-target` | 1 | `Payment Gateway` lives in the `payments` app, which this bench does not install |

Every one of these was checked against the source before it was written down.

### What it has already caught

The corpus is not decoration. Its first run reported 90 findings, of which 36
were the server's fault:

- 33 `unknown-hook` on hooks that work. An app may invent a hook — nothing
  registers `invoice_doctypes` anywhere, and the only evidence it means
  something is that erpnext reads it back. The registry now unions the vendored
  table with every key any app asks for ([12](12-surface-hooks.md)).
- 2 `translation-literal` inside `public/dist/`, which is esbuild output. The
  walker was reporting the minifier's rewrites as the author's mistakes.
- 1 stale-cache failure that turned out to be a bug in the cache rather than in
  the test: a file created while the server was off had no stamp to fail and
  appeared in no artifact list ([31](31-performance.md)).

## Stub completeness

[24](24-type-integration.md) generates a stub that replaces a real module's
types, so an omission becomes a false error in the user's editor. CI generates
against `frappe` `develop` and asserts stock pyright reports **zero new
diagnostics** on `frappe` itself. Generation that cannot meet that fails loudly
rather than emitting a partial stub.

## Invariants

Property tests where the input space is large:

- `scrub` is stable and matches Frappe's implementation over generated names.
- Layer merge is deterministic regardless of file iteration order.
- Incremental re-index yields a graph identical to a full rebuild.
- An incremental `didChange` sequence yields the same tree as a fresh parse.

The last two are the ones that break subtly in production and are nearly
impossible to debug from a bug report.
