# 009 — Index cache and file watching

**Phase** 2 · **Depends on** 008

Warm start and staying current, per [31](../specs/31-performance.md).

## Scope

- Serialize the index to `<bench>/.frappe-lsp/index.v<N>.bin`. Format decided by
  benchmark here, closing the question in 001.
- Validity key: schema version + `(path, mtime, len)` per indexed file. Partial
  invalidation re-parses only changed files.
- Corrupt or absent cache falls back to a full scan, silently.
- `notify` watcher with 250 ms per-path debounce; prefer client-side watcher
  registration where the client supports it ([30](../specs/30-configuration.md)).
- `frappe-lsp/reindex`.

## Done when

Warm start hits the 200 ms target on an ERPNext bench. A DocType JSON edited
outside the editor is reflected without a restart. A truncated cache file does
not surface an error to the user.
