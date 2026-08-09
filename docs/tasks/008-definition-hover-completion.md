# 008 — Definition, hover, completion

**Phase** 2 · **Depends on** 007 · **Blocks** 009

First user-visible milestone. Python surface only.

Ordered deliberately: **definition first**. The motivating problem is that
Frappe's dynamism forces grepping, so a concrete link is the payoff. Completion
is nice; navigation is the point.

## Scope

- **Definition** ([21](../specs/21-navigation.md)): DocType → controller / JSON /
  form script; field → its object inside the JSON; dotted path → symbol.
  `LocationLink` with `Location[]` fallback. `originSelectionRange` covers the
  string interior, not the quotes.
- **Hover** ([22](../specs/22-hover.md)): DocType, field, whitelisted method.
  Markdown with plain-text fallback. `None` on `Anchor::Unknown`.
- **Completion** ([20](../specs/20-completion.md)): DocType and field providers,
  trigger characters, string-interior `TextEdit` ranges, ranking, lazy
  `completionItem/resolve`, 1,000-item cap with `isIncomplete`.
- `$/cancelRequest` handling.

## Done when

Go-to-definition on a DocType literal in `frappe` and in a custom app on the
reference bench lands on the right controller, and all three stay within the
budgets in [31](../specs/31-performance.md), asserted end-to-end over stdio.
