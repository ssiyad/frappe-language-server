# Editor setup

frappe-lsp speaks LSP over stdio. Any editor with an LSP client can run it, and
no extension is needed. Run it beside your Python and JavaScript servers, not
instead of them ([00](specs/00-overview.md)).

The server finds the bench by itself. It walks up from the root the editor
sends until it finds a bench, so an app's own checkout under `apps/` works as a
root ([03](specs/03-workspace-model.md)).

## Neovim

Neovim 0.11 or later. This repository is the plugin. With lazy.nvim:

```lua
{
  'ssiyad/frappe-language-server',
  config = function()
    require('frappe_lsp').setup()
  end,
}
```

Or with `vim.pack` in Neovim 0.12:

```lua
vim.pack.add({ 'https://github.com/ssiyad/frappe-language-server' })
require('frappe_lsp').setup()
```

Then run `:FrappeLspInstall` once. It downloads the release for your platform,
checks it against `SHA256SUMS`, and puts it under Neovim's data directory; run
it again to update. With an authenticated `gh` it downloads through `gh`, which
also reaches a private repository. To use a binary of your own instead, pass
`setup({ cmd = '/path/to/frappe-lsp' })`, or leave `frappe-lsp` on your `PATH`.

`setup()` starts the server for any file in a bench, shows test lenses and runs
them in a terminal, and adds two more commands: `:FrappeLspStatus` prints what
the server indexed, and `:FrappeLspReindex` rebuilds the index from scratch.
`setup({ codelens = false })` leaves the lenses off.

Without `setup()`, `vim.lsp.enable('frappe_lsp')` still starts the server, from
the `lsp/frappe_lsp.lua` the plugin ships, but test lenses do nothing when
clicked. `tests/editors/neovim.lua` loads the plugin the same way against a
real bench, so this is the tested configuration.

A bench with more than one site asks which site to run tests against on the
first click. Set `test.site` to skip the question ([30](specs/30-configuration.md)).

## pi

This repository is also a [pi](https://pi.dev) package. Install it from a
checkout, or straight from the repository:

```sh
pi install /path/to/frappe-language-server
pi install git:github.com/ssiyad/frappe-language-server
```

In a session inside a bench it starts frappe-lsp and gives the agent four
tools: `frappe_doctype` (a DocType's fields, controller and JSON),
`frappe_references` (every place that names a DocType or a field),
`frappe_find` (DocTypes, whitelisted methods, reports and pages by name) and
`frappe_check` (frappe-lsp's checks on a file). Every file the agent edits or
writes in the bench is checked too, and any problem is appended to that edit's
result, so a misspelt fieldname is caught on the turn it is written. A skill
tells the agent when to reach for the tools.

The binary is found through `FRAPPE_LSP`, then `PATH`, then the copy
`:FrappeLspInstall` puts under Neovim's data directory.

## Helix

In `~/.config/helix/languages.toml`. Keep the servers you already use, and add
`frappe-lsp` after them. The names below are examples:

```toml
[language-server.frappe-lsp]
command = "frappe-lsp"

[[language]]
name = "python"
language-servers = ["pyright", "ruff", "frappe-lsp"]

[[language]]
name = "javascript"
language-servers = ["typescript-language-server", "frappe-lsp"]

[[language]]
name = "html"
language-servers = ["vscode-html-language-server", "frappe-lsp"]
```

Helix has no code lens support, so tests do not run from the editor.

## Zed

Zed starts a language server only through an extension, so this repository
carries one in `editors/zed`. With Rust installed through rustup, run
`zed: install dev extension` from the command palette and pick that directory;
Zed builds it. It runs `frappe-lsp` from your `PATH` for Python, JavaScript,
HTML and JSON, beside the servers Zed already runs for them. If you list a
language's `language_servers` yourself, keep `"..."` in the list or add
`"frappe-lsp"` to it.

Zed has no client-side command for test lenses, so they show and do nothing.
This extension compiles; it is not exercised by the test suite the way the
Neovim and pi routes are.

## VS Code

VS Code also needs an extension to start a language server. Any generic client
works; with [Simple LSP Client](https://marketplace.visualstudio.com/items?itemName=wdomitrz.simple-lsp-client),
in `settings.json`:

```json
{
  "simpleLspClient.servers": {
    "frappe-lsp": {
      "cmd": ["frappe-lsp"],
      "filetypes": ["python", "javascript", "json", "html", "jinja", "jinja-html"]
    }
  }
}
```

It runs beside the Python and JavaScript servers you already have. Open the
bench, or an app inside it, as the workspace folder: the server starts there and
walks up to the bench. Test lenses do nothing, because a generic client has no
handler for `frappe-lsp.runTest`. This setup is not exercised by the test suite.

## Other editors

Register a stdio server with the command `frappe-lsp` for Python, JavaScript,
JSON and HTML files. HTML files are read as Jinja templates. Then check three things:

1. The client sends `rootUri` or `workspaceFolders`. The server cannot find the
   bench without one of them.
2. The client answers `workspace/executeCommand` for `frappe-lsp.runTest` with
   the reply's `argv` and `cwd`. Otherwise test lenses do nothing.
3. The client keeps its own Python server. frappe-lsp gives no Python or
   JavaScript language features of its own.

## Configuration

A zero-config start against a bench works. To change anything, put a
`.frappe-lsp.toml` in the bench, or a `config.toml` in
`$XDG_CONFIG_HOME/frappe-lsp/`. [30](specs/30-configuration.md) has every key.
For example, to stop a check on one bench:

```toml
[diagnostics]
unknown-form-event = "off"
```

One line can opt out with a comment on it or on the line above:
`frappe-lsp: ignore` for every check, or `frappe-lsp: ignore <code>` for one
([23](specs/23-diagnostics.md)).

## Typed documents in pyright

Pyright types `frappe.get_doc("Sales Invoice", name)` as `Document`, so every
field access on it is an error. Set `types.stub = true` in `.frappe-lsp.toml`:

```toml
[types]
stub = true
```

The server then writes `.frappe-lsp/stubs/frappe/model/document.pyi` in the
bench and keeps it current. Point pyright at it in `pyrightconfig.json`, or the
`[tool.pyright]` table of `pyproject.toml`:

```json
{ "stubPath": "/path/to/bench/.frappe-lsp/stubs" }
```

`get_doc`, `new_doc` and the rest then return the controller class, so a
misspelt field is a pyright error. It costs pyright time: a controller file
checks in about 5 s instead of 3 on an eight-app bench
([24](specs/24-type-integration.md)). Without the stub, the "Cast to …" code
action on a `get_doc` call types that one call.

## When something is wrong

Send `frappe-lsp/indexStatus` and read the answer first. It reports how many
apps, DocTypes, fields and modules the server found, the frappe version, and
whether the index is complete (`confidence`). Zero apps means it found no bench
above the root the editor sent. In Neovim:

```vim
:FrappeLspStatus
```

For more detail, add `--log-level debug` to the command, for example
`setup({ cmd = { 'frappe-lsp', '--log-level', 'debug' } })`. The server logs to stderr,
and Neovim writes that to `:LspLog`. Attach both to a bug report.
