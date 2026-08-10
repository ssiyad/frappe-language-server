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
profile = ""           # frappe-cli profile supplying credentials
refresh_interval = 0   # seconds; 0 = manual only

[diagnostics]
enabled = true
# "error" | "warning" | "info" | "hint" | "off"
unknown-doctype = "warning"
unknown-field = "warning"
non-whitelisted-call = "error"
unknown-hook = "warning"
unknown-doc-event = "error"
translation-literal = "error"

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
```

Diagnostic keys mirror the codes in [23](23-diagnostics.md) with the
`frappe-lsp/` prefix dropped. The block above shows the defaults worth stating;
every code in [23](23-diagnostics.md) is settable the same way.

## Credentials

Never in any of these files. Resolution order in [05](05-live-bridge.md):
environment, then a named `frappe-cli` profile. `bridge.profile` names a profile;
it never contains a secret. A config file containing something that looks like a
key MUST be rejected with a logged error, not silently used — committed
credentials are the failure mode worth designing against.

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
