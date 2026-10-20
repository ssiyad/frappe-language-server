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

Resolution walks the workspace's class graph by **bare class name**: a base
match, one level of local subclassing within the file, and any chain the index
already knows about. Matching by bare name can only say yes too often, and the
cost of that is a lens on a class that is not a test — visible and harmless.

This was originally specified as syntactic only, with no cross-module walking,
on the grounds that building a class graph to answer a code lens was too much.
The graph now exists regardless: [04](04-metadata-index.md) builds it to find
the attributes a controller inherits. With the cost already paid the trade-off
inverts, and the measurement is decisive — erpnext defines `ERPNextTestSuite`
in `erpnext/tests/utils.py`, and **484 of its test classes derive from it**. A
purely local rule shows no lens on any of them.

Measured over `frappe` + `erpnext`: 953 test files, 929 with lenses, 9 532
lenses. The 24 without contain no classes at all — Frappe generates an empty
`test_*.py` beside every new DocType, and nothing runnable is the right answer
for those.

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
   `frappe-lsp.runTest` with a **scope descriptor** — `{app, module, doctype,
   case, test}` — as its argument. The client spawns the resulting command in a
   terminal, where the user gets a TTY, colour, and their own scrollback. The
   server never sees the output. Requires a small client-side handler, which
   the Neovim plugin ships ([editors](../editors.md#neovim)).

   The lens carries a scope rather than an argv on purpose. A lens travels
   through the client, so what comes back is a request and not evidence; the
   server rebuilds the command from the scope against the index it holds, and
   refuses one naming an app this workspace does not have.

   That makes the client path a two-step handshake, which is the whole
   client-side handler:

   ```
   lens clicked → workspace/executeCommand frappe-lsp.runTest {scope}
                → {"argv": [...], "cwd": "<bench>"}   ← the server answers
                → client spawns argv in a terminal
   ```
2. **Server-run fallback.** `workspace/executeCommand` runs the argv, reports
   through `$/progress`, and streams output via `window/logMessage`. For clients
   with no glue. Strictly worse — no TTY, no interactivity — and MUST be opt-in.

The server MUST NOT invent a third channel. In particular it does not parse test
output into diagnostics: a failing test is not a defect in the line it points at,
and [23](23-diagnostics.md)'s precision rule would be meaningless applied to it.

## What may choose the program

`test.bench` is **ignored from `.frappe-lsp.toml`**. That file is committed
team-wide configuration ([30](30-configuration.md)) and arrives with a `git
clone`, which makes it the wrong place to name an executable: cloning a
repository would otherwise be enough to have the editor offer to run a program
of its author's choosing, under a lens that says "Run test". It is honoured
from the user's own config, from `initializationOptions` and from
`workspace/configuration` — all of which are the user's, not the repository's.

Every other key in a committed block still applies. Only the executable is
refused.

## Site resolution

`test.site` if set, else the site chosen this session, else
`sites/currentsite.txt`, else the single site directory if exactly one exists.

With several sites and none of those answers, the server **asks**. The lens
carries `frappe-lsp.selectSite`, titled "— choose a site"; answering it through
`window/showMessageRequest` fixes the choice for the session and refreshes the
lenses, which then run. Asking is not guessing: guessing runs a suite against
the wrong database, and this is the user saying which one they meant. A
dismissed prompt chooses nothing.

The choice is **not** written back to any file. Frappe already has a place for
this — `bench use <site>` writes `sites/currentsite.txt`, which resolution
reads first among the non-configured answers — and a server that edits config
files on a click is a server nobody trusts with the one feature that executes
anything. The log line names the alternatives when the choice is made.

Three ways to make it stick, in increasing blast radius:

| Where | Effect |
| --- | --- |
| The session prompt | This editor session only. Nothing on disk changes |
| `test.site` in the user's config | This server only. `bench` commands are unaffected |
| `bench use <site>` | The whole bench: every `bench` command in that directory, this server included |

The middle one exists precisely because the third is often too much: someone
running tests against one site while `bench console` talks to another is a
normal thing to want.

With no site at all there is nothing to ask, so the lens is inert and titled
"— no site".

## Non-goals

Test result trees, inline pass/fail gutters, watch mode, coverage overlays, and
UI (Cypress) tests. Those want a test-explorer protocol the LSP does not have;
attempting them through code lenses produces a worse version of what the terminal
already does well.
