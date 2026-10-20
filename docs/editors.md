# Editor setup

frappe-lsp speaks LSP over stdio, so any editor with an LSP client can run it.
Neovim, pi and Claude Code load it through plugins in this repository. VS Code
needs a generic client extension. Run it beside your Python and JavaScript
servers, not instead of them ([00](specs/00-overview.md)).

The server finds the bench by itself. It walks up from the root the editor
sends until it finds a bench, so an app's own checkout under `apps/` works as a
root ([03](specs/03-workspace-model.md)).

## How the plugins get the server

The Neovim, pi and Claude Code plugins need no `frappe-lsp` on `PATH`. Each one
starts the server through `bin/frappe-lsp` in this repository. On first use,
that launcher:

1. Reads the version from `package.json`, so a plugin always runs the server
   it was released with.
2. Downloads that release for your platform from GitHub and checks it against
   `SHA256SUMS`.
3. Keeps it in `~/.local/share/frappe-lsp/<version>/`, or under
   `$XDG_DATA_HOME`. All the plugins share this copy.

The launcher needs `curl` or `wget`, `tar`, and `sha256sum` or `shasum`. When
the download fails, it runs the newest copy it already has, then a
`frappe-lsp` on `PATH`. To run a local build instead, set `FRAPPE_LSP` to its
path.

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

The first file you open in a bench downloads the server
([above](#how-the-plugins-get-the-server)), with a notification. Updating the
plugin updates the server. `:FrappeLspInstall` downloads it now, for a machine
about to go offline. To use a binary of your own, pass
`setup({ cmd = '/path/to/frappe-lsp' })`.

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

This repository is also a [pi](https://pi.dev) package:

```sh
pi install git:github.com/ssiyad/frappe-language-server
```

In a session inside a bench it starts frappe-lsp and gives the agent four
tools: `frappe_doctype` (a DocType's fields, controller and JSON),
`frappe_references` (every place that names a DocType or a field),
`frappe_find` (DocTypes, whitelisted methods, reports and pages by name) and
`frappe_check` (frappe-lsp's checks on a file). Every file the agent edits or
writes in the bench is checked too, and any problem is appended to that edit's
result, so a misspelled fieldname is caught on the turn it is written. A skill
tells the agent when to reach for the tools.

The first session in a bench downloads the server
([above](#how-the-plugins-get-the-server)). To install from a checkout instead,
run `pi install /path/to/frappe-language-server`.

## Claude Code

This repository is also a [Claude Code](https://claude.com/claude-code) plugin
and its marketplace. One command adds both:

```sh
claude plugin install frappe-lsp --marketplace ssiyad/frappe-language-server
```

The plugin declares frappe-lsp as the language server for Python, JavaScript,
JSON and HTML files. Claude Code starts it when a handled file is first edited,
and its LSP tool answers definition, hover, references, document symbols and
workspace symbols against the bench. Diagnostics are pushed into the session
after each edit, so a misspelled fieldname is caught on the turn it is written.
A skill shipped with the plugin tells the agent when to use the LSP tool and
the `frappe-lsp/describe` and `frappe-lsp/references` methods.

The first start downloads the server
([above](#how-the-plugins-get-the-server)). Claude Code waits up to two
minutes for it. Files outside an app directory get no checks: put code under
`apps/`.

## VS Code

VS Code needs an extension to start a language server, and the binary on
`PATH` ([README](../README.md#install-the-binary)). Any generic client works.
With [Simple LSP Client](https://marketplace.visualstudio.com/items?itemName=wdomitrz.simple-lsp-client),
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
It was checked by hand on VS Code 1.142 with Simple LSP Client 0.1.22: hover,
definition, references, completion in a form script and diagnostics all answer.
With more than one target, definition opens a peek view.

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

The index cache is written to `.frappe-lsp/` in the bench root
([31](specs/31-performance.md)). Add it to `.gitignore`.

## Reading a live site

Custom Fields, Property Setters, DocTypes made in the UI, and Client and
Server Scripts live in a site's database, not in the bench. The bridge reads
them, and it is off by default ([05](specs/05-live-bridge.md)). To turn it on:

1. Add the site to `.frappe-lsp.toml`:

   ```toml
   [bridge]
   enabled = true
   site = "mysite.localhost"
   ```

2. Give the server an API key in its environment: `FRAPPE_API_KEY` and
   `FRAPPE_API_SECRET`, or `FRAPPE_LSP_TOKEN` as `<key>:<secret>`.

Instead of the environment, `bridge.token_command` can name a command that
prints `<key>:<secret>`. Set it in `$XDG_CONFIG_HOME/frappe-lsp/config.toml`.
The server ignores it in a `.frappe-lsp.toml`, because that file is often
committed.

The server reads the site once, after the index is built. Send
`frappe-lsp/refreshBridge` to read it again, or set `bridge.refresh_interval`
in seconds. The bridge only reads: it sends `GET` requests and nothing else.

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
misspelled field is a pyright error. It costs pyright time: a controller file
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

For more detail, add `--log-level debug` to the command. In Neovim:

```lua
require('frappe_lsp').setup({
  cmd = { require('frappe_lsp.install').launcher(), '--log-level', 'debug' },
})
```

The server logs to stderr, and Neovim writes that to `:LspLog`. Attach both to
a bug report.
