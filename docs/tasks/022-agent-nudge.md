# 022 — Point agents at the tools

**Phase** 6 · **Depends on** — · **Blocks** nothing

Agents with the pi package installed answered field questions with grep: over
two weeks of sessions, four `frappe_doctype` calls and none of the other tools,
against thousands of shell calls. The skill alone does not change that.

## Scope

- In the pi extension, when the agent greps a bench for a word that is a
  fieldname of an indexed DocType, append a one-line hint to the result naming
  `frappe_references` with that DocType and field. Once per field per session,
  so it does not become noise.
- The same for a grep for a DocType name, naming `frappe_doctype`.
- Measure with the natural and guided arms of `evals/`, before and after.

## Done when

- The natural arm of `evals/` calls a frappe-lsp tool in at least half of the
  cases where the guided arm does, up from the measured baseline.
- No hint fires on a grep outside a bench, or for a word that is not a field.
