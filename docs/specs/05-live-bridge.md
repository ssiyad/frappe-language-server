# 05 — Live bridge

Optional Layer 3 for [04](04-metadata-index.md). Off by default. Everything the
server does MUST remain correct without it.

## Why

Three classes of truth exist only in the site database:

1. **Custom Field** / **Property Setter** — UI customizations. The type exporter
   explicitly declines these, so nothing else can supply them.
2. **DocTypes created through the UI** — `custom: 1`, never written to disk.
3. **Client Script** / **Server Script** — code stored as records.

Without the bridge, a field a user added via Customize Form looks unknown. This
is the single largest source of false positives in [23](23-diagnostics.md), and
the reason confidence gating exists.

## Transport

HTTP against `/api/v2` on the configured site. Read-only: the bridge MUST issue
only `GET`, and MUST NOT call any method endpoint that mutates. Requests carry
`token <key>:<secret>`.

## Credentials

Never from editor config in plaintext, never from `site_config.json`. In order:

1. `FRAPPE_LSP_TOKEN`, or `FRAPPE_API_KEY` + `FRAPPE_API_SECRET`.
2. A named [`frappe-cli`](https://pypi.org/project/frappe-cli/) profile — reuse
   its credential store rather than inventing a second one.
3. Nothing → bridge stays disabled, logged once at `info`.

Rationale: an LSP config file is commonly committed. Credentials in it leak.

## Sync

On enable, and on explicit `frappe-lsp/refreshBridge`:

| Query | Into |
| --- | --- |
| `Custom Field` — `dt`, `fieldname`, `fieldtype`, `options`, `label`, `insert_after`, … | Layer 3 fields |
| `Property Setter` — `doc_type`, `field_name`, `property`, `value`, `property_type` | Layer 3 overrides |
| `DocType` where `custom = 1` | Layer 3 DocTypes (fields via `DocField`) |
| `Client Script` — `dt`, `view`, `script`, `enabled` | Virtual documents |
| `Server Script` — `script_type`, `reference_doctype`, `script` | Virtual documents |

Paged, `limit_page_length` bounded. Results are cached with the index but always
tagged Layer 3 so they can be evicted without a rebuild.

Refresh is manual or on an interval (default: never). The server MUST NOT poll on
keystroke.

## In-DB scripts

Client and Server Scripts have no file on disk. Phase 2+ exposes them as
read-only virtual documents under a `frappe-lsp://script/<doctype>/<name>` URI so
that navigation from a `doc_events`-equivalent lands somewhere. Editing them
through the LSP is out of scope — that would mean writing to the site.

## Degradation

| State | Effect |
| --- | --- |
| Disabled | Layers 1–2 only. Confidence `Static`. |
| Enabled, reachable | Layers 1–3. Confidence `Full`. |
| Enabled, unreachable | Layers 1–2. Confidence `Degraded` — unknown-field diagnostics suppressed ([23](23-diagnostics.md)). |

The distinction between `Static` and `Degraded` matters: with no bridge
configured the user has accepted that customizations are invisible, so warnings
are still useful. With a configured-but-down bridge, the server knows its picture
is incomplete and MUST NOT accuse valid code.
