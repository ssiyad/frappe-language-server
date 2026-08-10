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

Nightly CI clones `frappe` and `erpnext` at `develop` and asserts:

- index builds without panic;
- DocType count is within an expected range;
- **zero diagnostics** across both apps, save for a committed allowlist.

Frappe's own code is the best available corpus of valid Frappe code. Any
diagnostic it produces is, by default, a false positive — and this job is what
turns [23](23-diagnostics.md)'s precision rule into something enforced rather
than merely stated.

The allowlist is for the other case: a diagnostic that is right about code that
is wrong. It holds 8 `translation-literal` hits at frappe `5a94ce8` — f-strings
and `.format()` inside `_()`, which break string extraction. Each entry names a
file, a code and the upstream issue; an entry without one is a false positive
wearing a disguise, and the list must not grow to accommodate a noisy check.
Anything not on it fails the job.

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
