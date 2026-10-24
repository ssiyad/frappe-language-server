# 26 — Command line

The server's answers for a program with a shell rather than an LSP client: a
coding agent, a CI job, a pre-commit hook.

## Why

An agent reads and greps files with its own tools. Its harness pushes this
server's diagnostics only after the agent edits a file, so a review or an audit
never sees them, and its LSP client cannot call the custom methods of
[30](30-configuration.md). Measured with `claude plugin eval` over press, 13
runs: the skill loaded, the agent made no-op edits and then a real temporary
one to make the diagnostics fire, and an audit of `press/api/` cost $5–6 per
run of fanned-out grep either way.

## Commands

```text
frappe-lsp check [--json] [<path>...]
frappe-lsp describe [--json] <DocType>
frappe-lsp references [--json] <DocType> [<fieldname>]
```

| Command | Answers | Exit |
| --- | --- | --- |
| `check` | [23](23-diagnostics.md)'s findings for each file named, and for each file a directory holds that the server would open | 1 on any finding, 0 on none |
| `describe` | The `frappe-lsp/describe` answer of [30](30-configuration.md) | 1 for an unknown name |
| `references` | The `frappe-lsp/references` answer of [30](30-configuration.md) | 1 for an unknown DocType or field |

2 means no answer: no bench, a missing path, bad arguments.

`check` prints one finding per line, `path:line:column: severity: message
[code]`, 1-based, the column in bytes, paths relative to the current
directory. A summary goes to stderr, so stdout is only findings. `references`
prints `path:line: text`, and its count and `covers` to stderr. `describe`
prints a header, then one field per line.
`--json` prints the same as one JSON value.

An unknown name MUST be said as such. An empty reference list for a misspelt
field reads as "nothing uses this", which is the wrong answer stated
confidently.

## Bench and index

The bench is found as the server finds it ([03](03-workspace-model.md)), from
the first path or the current directory, with the same configuration sources
less the two only an editor supplies ([30](30-configuration.md)). The index
comes from the server's cache when it is current; otherwise the command builds
it, scans occurrences and writes the cache, so the next command and the next
editor session start warm.

Measured on a four-app bench (frappe, press, wiki, newsletter): `check
press/api` over 57 files takes 2.6 s from no cache and 0.13 s for two files
from a warm one.

## Non-goals

- **A daemon.** A warm start is 0.1–0.3 s ([31](31-performance.md)); a
  process that outlives the command would have to watch the bench for an
  answer that fast already.
- **Fixes.** Quick fixes stay in the editor, where a person sees the edit. An
  agent applies its own.
- **An MCP server.** A shell is the one tool every agent harness has, and the
  plugins put `bin/frappe-lsp` on its `PATH`.
