# 30 — Configuration

## Sources

Merged, later wins:

1. `<bench>/.frappe-lsp.toml` — committed, team-wide.
2. `$XDG_CONFIG_HOME/frappe-lsp/config.toml` — per user.
3. LSP `initializationOptions`.
4. `workspace/didChangeConfiguration`.

TOML for files, the same key structure as JSON for `initializationOptions`.
Anything not set falls back to a working default: a zero-config launch against a
bench MUST do something useful.

## Schema

```toml
[workspace]
root = "auto"          # "auto" | absolute path
apps = []              # [] = all discovered
exclude = []           # glob patterns, added to the built-in exclusions

[index]
cache = true
cache_dir = ".frappe-lsp"

[bridge]
enabled = false
site = ""              # site name under sites/, or a full URL
token_command = ""     # prints "<key>:<secret>"; ignored from .frappe-lsp.toml — see 05
refresh_interval = 0   # seconds; 0 = manual only

[diagnostics]
enabled = true
# "error" | "warning" | "info" | "hint" | "off"
unknown-doctype = "warning"
unknown-field = "warning"      # fieldnames written as names
unknown-attribute = "off"      # `doc.x` — needs types, see 23
unknown-form-event = "warning"
non-whitelisted-call = "error"
unknown-hook = "warning"
unknown-doc-event = "error"
translation-literal = "error"
translation-dynamic = "off"    # 721 hits on frappe itself — see 23
parent-field-on-child = "warning"
not-single-doctype = "warning"

[types]                  # experimental — see spec 24
stub = false             # generate frappe/model/document.pyi
stub_dir = ".frappe-lsp/stubs"
scope = "referenced"     # "referenced" | "all" — "all" is ~60x slower, unsupported
cast_action = true       # cast quick fix; independent of the stub

[completion]
max_items = 1000
detail = true

[hover]
enabled = true

[test]                   # spec 25 — the one feature that executes anything
enabled = true           # discovery only; lenses cost nothing
runner = "client"        # "client" | "server" — server-run has no TTY, so it is opt-in
site = ""                # overrides resolution; required when a bench has several sites
bench = "bench"          # ignored from .frappe-lsp.toml — see 25
failfast = false
profile = false
coverage = false
lightmode = false
```

Diagnostic keys mirror the codes in [23](23-diagnostics.md) with the
`frappe-lsp/` prefix dropped. The block above shows the defaults worth stating;
every code in [23](23-diagnostics.md) is settable the same way.

## Credentials

Never in any of these files. Resolution order in [05](05-live-bridge.md):
environment, then whatever `bridge.token_command` prints. That key holds a
command, never a secret. A config file containing something that looks like a
key MUST be rejected with a logged error, not silently used — committed
credentials are the failure mode worth designing against.

A key naming a *command* is exempt from that scan: `token_command` holds a
shell command, not a secret, and rejecting the file it appears in would make
the one safe way to supply credentials unusable.

## What a committed file may not decide

`.frappe-lsp.toml` is source 1 and arrives with a `git clone`, so it is trusted
with settings and not with actions. Two keys are dropped from it and taken only
from the user's own configuration: `test.bench`, the program the test runner
spawns ([25](25-test-runner.md)), and `bridge.token_command`, which would both
run a program and receive credentials ([05](05-live-bridge.md)). Cloning a repository must not be enough to have the
editor offer to run a program its author chose, under a lens labelled "Run
test".

This is the same reasoning as the credentials rule above, in the other
direction: that one keeps secrets from leaving, this one keeps instructions
from arriving.

## Capabilities

Advertised: `completionProvider` (triggers `"`, `'`, `.`, `[`; `resolveProvider`),
`hoverProvider`, `definitionProvider`, `typeDefinitionProvider`,
`implementationProvider`, `referencesProvider`, `documentSymbolProvider`,
`workspaceSymbolProvider`, `codeActionProvider`, `diagnosticProvider`,
`textDocumentSync: Incremental`.

Not advertised: formatting, rename, signature help, semantic tokens, inlay hints
([00](00-overview.md)).

Client capabilities MUST be honoured, not assumed:

| Capability | Fallback when absent |
| --- | --- |
| `LocationLink` | Plain `Location[]` |
| `MarkupKind::Markdown` | Plain text hover |
| `labelDetails` | Fold into `detail` |
| Pull diagnostics | Push via `publishDiagnostics` |
| `positionEncoding: utf-8` | UTF-16 |
| Dynamic file-watcher registration | Internal `notify` watcher |

## Custom methods

| Method | Purpose |
| --- | --- |
| `frappe-lsp/indexStatus` | App/DocType counts, confidence, build time. Drives testing ([32](32-testing.md)) and bug reports |
| `frappe-lsp/reindex` | Force a full rebuild |
| `frappe-lsp/refreshBridge` | Re-sync Layer 3 ([05](05-live-bridge.md)) |

## Logging

`--log-level` and `FRAPPE_LSP_LOG`. Traces to stderr; `window/logMessage` for
anything the user should see. Never write to stdout — it carries the protocol.
