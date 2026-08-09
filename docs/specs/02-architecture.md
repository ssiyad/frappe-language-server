# 02 — Architecture

## Process model

Single binary, stdio LSP, `tokio` runtime, `tower-lsp` transport. No editor
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

Dependency direction is strictly `frappe-lsp → {frappe-index, frappe-syntax} → frappe-bridge`.
`frappe-syntax` MUST NOT depend on `frappe-index`: pattern extraction yields
unresolved names, resolution happens in the server layer. This keeps surface
extraction unit-testable without a bench.

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
| Grammar parse error | Use the last good tree for that buffer. |

## Open questions

- Whether to expose the index over a custom LSP extension method for debugging.
  Leaning yes: `frappe-lsp/indexStatus` is cheap and makes the tasks in
  [32](32-testing.md) far easier.
- Grammar for Jinja — no mature tree-sitter grammar; see [13](13-surface-jinja.md).
