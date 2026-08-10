# 02 — Architecture

## Process model

Single binary, stdio LSP, `tokio` runtime, `tower-lsp-server` transport. No editor
extension and no sidecar process. The server never imports or executes app code.

```
editor ──stdio(JSON-RPC)──> frappe-lsp
                              ├── documents   open buffers (ropey + tree-sitter trees)
                              ├── index       metadata graph (RwLock, snapshot reads)
                              ├── watcher     notify → debounce → incremental re-index
                              └── bridge      optional HTTP client to a live site
```

## Crates

| Crate | Responsibility |
| --- | --- |
| `frappe-lsp` | Binary. Protocol handlers, request routing, config. |
| `frappe-index` | Workspace discovery, metadata graph, layer merge, cache. |
| `frappe-syntax` | tree-sitter grammars + queries; surface pattern extraction. |
| `frappe-bridge` | Live site client. Optional at runtime, always compiled. |

Dependency direction is `frappe-lsp → frappe-index → frappe-syntax`, with
`frappe-bridge` at the bottom. `frappe-syntax` MUST NOT depend on
`frappe-index`: pattern extraction yields unresolved names, resolution happens
in the server layer. This keeps surface extraction unit-testable without a
bench, and that ban is the whole of the rule.

The other direction is allowed, and the index uses it. Controller methods and
the whitelisted-method registry ([04](04-metadata-index.md)) are read out of
Python source, and there must be exactly one Python reader in the workspace: a
second one written to dodge a dependency edge would disagree with the first
about what a decorator is, and the disagreement would surface as a diagnostic
on correct code.

## Concurrency

- Index lives in `Arc<RwLock<Index>>`. Requests take a read guard and clone the
  small slice they need; long operations MUST NOT hold the guard.
- Initial index build runs as a background task. Requests arriving before it
  completes are served against a partial index with `confidence = Partial`
  ([23](23-diagnostics.md)).
- Re-index is incremental per file. A `didSave` or watcher event on a DocType
  JSON re-parses that file only and patches the graph.
- Buffer edits use tree-sitter incremental parsing; the old tree is reused via
  `Tree::edit`.

## Request flow

```
textDocument/completion
  → locate node at position (incremental tree)
  → surface match: which pattern is this string literal in?   [10..13]
  → resolve anchor (DocType) from the match                   [04]
  → produce candidates from index
```

The three stages are separately testable: extraction (no index), resolution
(index only), presentation (LSP types).

## Layered metadata

The index is a merge of ordered layers; higher layers override lower ones on the
same key. See [04](04-metadata-index.md) for the merge algebra and
[05](05-live-bridge.md) for the optional layers.

```
3  live site      Custom Field, Property Setter, DB-only DocTypes   (optional)
2  app fixtures   <app>/fixtures/*.json
1  app schema     <module>/doctype/<x>/<x>.json
```

## Failure policy

| Failure | Behaviour |
| --- | --- |
| Malformed DocType JSON | Drop that DocType, log, continue. Publish a diagnostic on the file. |
| No bench detected | Serve nothing Frappe-specific; do not error the session. |
| Bridge unreachable | Fall back to static layers, downgrade confidence, no user-facing error after the first. |
| Grammar returns no tree | Use the last good tree for that buffer. A tree carrying `ERROR` nodes is **not** this case: a half-typed buffer always has them, and refusing it would stop completion mid-keystroke. |

## Resolved

- `frappe-lsp/indexStatus` is exposed. Cheap, and [32](32-testing.md) depends on
  it for integration assertions.
- Jinja grammar choice is deferred to phase 4; options in
  [13](13-surface-jinja.md).
- Cache format is decided by benchmark during implementation
  ([31](31-performance.md)).
- The server is branch-agnostic; version handling in [31](31-performance.md).
- Licensed GPLv3, as `frappe/erpnext` is. Consequence for upstreaming in
  [019](../tasks/019-upstream.md).
