# 012 — Diagnostics

**Phase** 3 · **Depends on** —

[23](../specs/23-diagnostics.md). The riskiest task in the project, and the
first one that speaks unprompted about code somebody else wrote.

Much of what it needs exists: the code registry in
[23](../specs/23-diagnostics.md), `Confidence` on the index, the `Document`
member table that keeps attribute access quiet, and a resolver whose every
uncertain answer is already `Unresolvable`. What is missing is the publishing,
the gating and the checks themselves.

Take the checks that cannot be wrong about correct code first — `malformed-json`,
unknown hook keys, unknown doc events, invalid cron, duplicate assignments — and
the ones that judge user code after. Each of the first group is decidable from a
vendored table or a parse failure; each of the second needs the negative cases in
the fixture bench to hold.

## Scope

- Gating of absence checks on confidence, and on the version match in
  [31](../specs/31-performance.md) for anything decided by a vendored table.
- Publishing at all: the index already records `problems`, and nothing surfaces
  them.
- Check sets: hooks, cross-language, schema, Python, translations, patches.
- Stable codes and `codeDescription` URLs.
- Suppression comments, and per-rule severity including `off`.
- Push and pull diagnostic transports.
- 150 ms per-buffer debounce.
- Quick fixes: close-match replacement (single candidate, edit distance ≤ 2),
  insert `@frappe.whitelist()`. No "fix all".

## Done when

Every check has a positive **and** a negative test case
([32](../specs/32-testing.md)), and the nightly smoke test reports zero
diagnostics across `frappe` and `erpnext`. A false positive on that corpus blocks
the task.
