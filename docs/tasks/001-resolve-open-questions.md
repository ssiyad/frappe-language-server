# 001 — Resolve open design questions

**Phase** 1 · **Blocks** 002

Open items left unsettled by the specs. Each needs a decision recorded as a spec
edit, not a comment thread.

| Question | Spec | Note |
| --- | --- | --- |
| Jinja grammar strategy | [13](../specs/13-surface-jinja.md) | Leaning hand-written delimiter scanner; can defer to phase 4 |
| Expose `frappe-lsp/indexStatus`? | [02](../specs/02-architecture.md), [30](../specs/30-configuration.md) | Leaning yes — [32](../specs/32-testing.md) depends on it |
| Cache format: `bincode` vs `rkyv` | [31](../specs/31-performance.md) | Decide with a benchmark in 009, not in advance |
| Do we pin behaviour to a detected Frappe version? | [31](../specs/31-performance.md) | Affects whether "unknown" can ever become "invalid" |
| License and repo home | — | Blocks 002 |

## Done when

Each row is either resolved in the relevant spec, or explicitly deferred to a
named phase with the reason recorded.
