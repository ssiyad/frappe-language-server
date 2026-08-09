# 25 — Test running

Run a Frappe test from the file it lives in, without reconstructing a `bench`
invocation by hand.

## Capability class

This is the one feature that **executes** something. Everything else in these
specs reads files ([00](00-overview.md)). Running a test needs a bench, a site,
and the venv — exactly the runtime coupling the architecture avoids.

Therefore:

- Off unless a site is resolvable; never a hard error.
- Discovery (lenses) MUST NOT depend on execution being possible. A read-only
  checkout still shows what is runnable and why it is not.
- Nothing here touches the index or the surfaces. It is additive and isolated.

## Discovery

A test class is a `unittest.TestCase` subclass, in practice one of
`UnitTestCase`, `IntegrationTestCase`, `MockedRequestTestCase`, or anything
deriving from them ([`frappe/tests/classes/`](https://github.com/frappe/frappe/tree/develop/frappe/tests/classes)).
Resolution is syntactic — a base-class name match, plus one level of local
subclassing within the file. No cross-module class hierarchy walking.

Test methods are `def test_*` on such a class. Files are `test_*.py`; those
beside a DocType (`test_<scrubbed>.py`) additionally carry the DocType anchor.

## Lenses

`textDocument/codeLens`, one per runnable scope:

| Position | Lens | Maps to |
| --- | --- | --- |
| File | `Run module` | `--module <dotted>` |
| Class | `Run class` | `--module <dotted> --case <Class>` |
| `def test_*` | `Run test` | `--module <dotted> --case <Class> --test <method>` |
| DocType JSON, when `test_<x>.py` exists | `Run DocType tests` | `--doctype "<Name>"` |

Module path via [07](07-dotted-paths.md), reversed: file → dotted path.

Command shape, from `bench run-tests`
([`frappe/commands/testing.py`](https://github.com/frappe/frappe/blob/develop/frappe/commands/testing.py)):

```
bench --site <site> run-tests --app <app> --module <dotted> --case <Class> --test <method>
```

`--failfast`, `--profile`, `--coverage` and `--lightmode` are exposed through
configuration, not through separate lenses. One lens per scope; a lens menu is
noise.

## Execution

LSP has no test protocol, so the client decides. Two paths, both supported:

1. **Client-run (preferred).** The lens carries command
   `frappe-lsp.runTest` with the argv array as arguments. The client spawns it in
   a terminal, where the user gets a TTY, colour, and their own scrollback. The
   server never sees the output. Requires a small client-side handler, which
   [16](../tasks/016-distribution.md) documents for Neovim.
2. **Server-run fallback.** `workspace/executeCommand` runs the argv, reports
   through `$/progress`, and streams output via `window/logMessage`. For clients
   with no glue. Strictly worse — no TTY, no interactivity — and MUST be opt-in.

The server MUST NOT invent a third channel. In particular it does not parse test
output into diagnostics: a failing test is not a defect in the line it points at,
and [23](23-diagnostics.md)'s precision rule would be meaningless applied to it.

## Site resolution

`sites/currentsite.txt`, else the single site directory if exactly one exists,
else configuration. Ambiguity with no configured site disables execution and says
so once — it does not guess. Guessing here runs a test suite against the wrong
database.

## Non-goals

Test result trees, inline pass/fail gutters, watch mode, coverage overlays, and
UI (Cypress) tests. Those want a test-explorer protocol the LSP does not have;
attempting them through code lenses produces a worse version of what the terminal
already does well.
