# 018 — Test running

**Phase** 3 · **Depends on** — · Independent of the surfaces

[25](../specs/25-test-runner.md). Needs workspace discovery, which is done, and
nothing else. Blocks nothing.

## Scope

- Test discovery: `UnitTestCase` / `IntegrationTestCase` / `MockedRequestTestCase`
  subclasses, plus one level of local subclassing. Syntactic only.
- `test_*` methods; `test_*.py` files; DocType anchor for `test_<scrubbed>.py`.
- File → dotted module path (reverse of [07](../specs/07-dotted-paths.md)).
- Code lenses for module / class / method / DocType scopes.
- Client-run path: lens command `frappe-lsp.runTest` carrying argv.
- Server-run fallback via `workspace/executeCommand`, `$/progress` +
  `window/logMessage`. Opt-in.
- Site resolution: `currentsite.txt`, else sole site directory, else config.
  Ambiguity disables execution with one message — never guess.
- Config passthrough for `--failfast`, `--profile`, `--coverage`, `--lightmode`.

## Done when

Lenses appear on `frappe/tests/test_*.py` in the reference bench and produce a
`bench run-tests` invocation that runs the intended scope. Lenses still render on
a checkout with no site, annotated as not runnable.

## Explicitly not

Result trees, gutter pass/fail marks, watch mode, coverage overlays, Cypress.
Test output is never converted into diagnostics
([25](../specs/25-test-runner.md)).
