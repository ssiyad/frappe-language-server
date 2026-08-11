# 014 — Live bridge

**Phase** 4 · **Depends on** —

[05](../specs/05-live-bridge.md). Layer 3 of [04](../specs/04-metadata-index.md).

## Scope

- Read-only `/api/v2` client. `GET` only — assert no mutating call is reachable.
- Credential resolution: environment, then `frappe-cli` profile. Reject anything
  key-shaped found in a config file.
- Sync of Custom Field, Property Setter, custom DocTypes, Client Script, Server
  Script. Paged and bounded.
- Layer 3 merge with eviction that needs no rebuild.
- Confidence transitions, especially `Degraded` suppressing absence checks.
  This is the last item task 012 left open ([23](../specs/23-diagnostics.md)): the
  suppression is written and tested against `Partial`, and `Degraded` has no
  way to occur until a bridge can be configured and fail.
- `frappe-lsp/refreshBridge`; optional interval, default manual.
- In-DB scripts as read-only `frappe-lsp://script/…` virtual documents.

## Done when

A field added through Customize Form completes without a restart, hover reports
it as `Custom Field · <site>`, and stopping the site suppresses unknown-field
diagnostics rather than flooding them.
