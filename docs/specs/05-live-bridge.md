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

1. `FRAPPE_LSP_TOKEN`, or `FRAPPE_API_KEY` + `FRAPPE_API_SECRET`. The site may
   come from `bridge.site`, or from `FRAPPE_SITE` when that is unset.
2. `bridge.token_command` — any command that prints `<key>:<secret>` on stdout.
3. Nothing → bridge stays disabled, logged once at `info`.

Rationale: an LSP config file is commonly committed. Credentials in it leak.

### Why a command and not a credential store

Reusing a credential store should not mean depending on a particular one. This
spec previously named [`frappe-cli`](https://pypi.org/project/frappe-cli/) and
read its keyring; that made a Python tool a soft dependency of the server, cost
an OS-keyring dependency on every platform, and still served only the people
who used that one tool. A command satisfies every store at once — `pass show
frappe/local`, a password manager's CLI, a Frappe client, a three-line script —
and the server never learns which.

It runs through a shell, so pipelines work, and off the async worker, because
it is somebody's script and may prompt.

`bridge.site` always wins over whatever a source names: a bridge pointed at one
site by configuration and another by a stray variable is the shape of an
accident. A token command answers "what is the secret", never "which database
should this read".

A command that fails is `Degraded`, not disabled — a configured bridge that
cannot produce credentials is not an unasked one, and the difference decides
whether the absence checks fire.

Like every other executable a configuration can name, `token_command` is
**refused from `.frappe-lsp.toml`** ([30](30-configuration.md)): that file
arrives with a `git clone`, and a repository that could name it would run a
program *and* be handed what it prints.

## Sync

On enable, and on explicit `frappe-lsp/refreshBridge`:

| Query | Into |
| --- | --- |
| `Custom Field` — `dt`, `fieldname`, `fieldtype`, `options`, `label`, `insert_after`, … | Layer 3 fields |
| `Property Setter` — `doc_type`, `field_name`, `property`, `value`, `property_type` | Layer 3 overrides |
| `DocType` where `custom = 1` | Layer 3 DocTypes (fields via `DocField`) |
| `Client Script` — `dt`, `view`, `script`, `enabled` | Virtual documents |
| `Server Script` — `script_type`, `reference_doctype`, `script` | Virtual documents |

Paging is `start` and `limit=500` with `order_by=name asc`, read until the
site's `has_next_page` is false. Without a stable order the page boundaries move
between calls and a sync silently drops rows. The parameter names matter: v2
ignores v1's `limit_start` and `limit_page_length` and answers 20 rows, so a
client using them read 20 Custom Fields of a site's thousands and reported the
rest as absent. A
per-DocType ceiling stops a pathological site from making the server allocate
without bound.

A partial read is **not** merged. Half a picture merged as if it were whole
looks like absence, which is the one thing `Degraded` exists to prevent.

**Custom Field, Property Setter and UI-created DocTypes are implemented.** The
list row of a UI-created DocType has no fields, and a DocType with none is worse
than no DocType, because every field on it would resolve to nothing. So each one
is read a second time, from `/api/v2/doctype/<name>/meta`, which is the DocType
JSON shape with every field and goes through the same reader. It joins the
graph only when no installed app defines the same name: a file in the bench is
what the code runs against. It has no file, so definition goes nowhere and
hover names the site. Client and Server Scripts are not read at all — the
virtual document surface below is untouched.

Paged, and bounded per DocType. Results are cached with the index but always
tagged Layer 3 so they can be evicted without a rebuild.

Refresh is manual or on an interval (default: never). The server MUST NOT poll
on keystroke: `bridge.refresh_interval` is a timer and nothing else, it is read
again on each tick so setting it to `0` stops it, and each refresh asks the
client to re-pull diagnostics because Layer 3 changes what every open buffer
resolves to.

## In-DB scripts

Client and Server Scripts have no file on disk. Each sync writes them to
read-only files under the bench's cache directory,
`<cache_dir>/scripts/<site>/client/<name>.js` and `…/server/<name>.py`, and
removes the previous sync's. Definition on a DocType lists the scripts that
run for it after its controller and form scripts, and hover counts them. A
Client Script opened from there gets completion and hover like any form script,
because its `frappe.ui.form.on` binds `frm` the usual way. Editing them through
the LSP is out of scope — that would mean writing to the site — and the files
are read-only so an edit fails where it is made rather than vanishing at the
next sync.

A refusal to read either script DocType leaves the scripts out, not the site:
many API keys cannot read `Server Script`, and the fields are still the whole
picture.

### Rejected: virtual documents

The first design served them as `frappe-lsp://script/<doctype>/<name>`
documents. No stock client opens a custom URI scheme: Neovim needs a
`BufReadCmd` handler, VS Code a content provider in an extension, and
[00](00-overview.md) rules extensions out. A file under the bench opens
everywhere.

## Merge and eviction

Layer 3 goes into the graph through Layer 2's reader: `Custom Field` and
`Property Setter` are ordinary DocTypes, and a fixtures file is a dump of their
rows, so the two shapes are identical. Reusing the reader is what keeps Layer 3
from drifting from Layer 2 in how it reads a `depends_on`.

It merges *after* Layer 2, so the live site wins — a customization present in
both is current in the database and a snapshot in the file. A Custom Field the
site reports that a fixture already supplied is the same field, not a
collision.

Eviction needs no rebuild and no file list: Layer 3 entities are exactly the
ones whose provenance has no `source` path. A re-sync evicts the previous one
first, or a field deleted through Customize Form would stay in the graph
forever.

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
