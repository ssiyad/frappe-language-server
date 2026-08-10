# 012 — Diagnostics

**Phase** 3 · **Depends on** —

[23](../specs/23-diagnostics.md). The riskiest task in the project; sequenced
last in phase 3 so it runs on resolution that has already been validated.

## Scope

- Confidence model (`Full` / `Static` / `Degraded` / `Partial`) and gating of
  absence checks.
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
