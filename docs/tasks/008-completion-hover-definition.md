# 008 — Completion, hover, definition

**Phase** 2 · **Depends on** 007 · **Blocks** 009

The first user-visible milestone. Python surface only.

## Scope

- **Completion** ([20](../specs/20-completion.md)): DocType and field providers,
  trigger characters, string-interior `TextEdit` ranges, ranking, lazy
  `completionItem/resolve`, the 1,000-item cap with `isIncomplete`.
- **Hover** ([22](../specs/22-hover.md)): DocType, field, whitelisted method.
  Markdown with a plain-text fallback. `None` on `Anchor::Unknown`.
- **Definition** ([21](../specs/21-navigation.md)): DocType → controller / JSON /
  form script; field → its JSON object; dotted path → symbol. `LocationLink` with
  `Location[]` fallback.
- `$/cancelRequest` handling.

## Done when

All three work on an ERPNext bench within the budgets in
[31](../specs/31-performance.md), driven end-to-end over stdio in an integration
test.
