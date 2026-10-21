# frappe-lsp

[![CI](https://github.com/ssiyad/frappe-language-server/actions/workflows/ci.yml/badge.svg)](https://github.com/ssiyad/frappe-language-server/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/ssiyad/frappe-language-server)](https://github.com/ssiyad/frappe-language-server/releases/latest)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENSE)

**Go to definition, completion and checks for [Frappe](https://frappe.io/framework) apps, in any editor.**

Frappe connects its parts with strings: `frappe.get_doc("Sales Invoice")`,
`doc_events`, `frappe.ui.form.on("Task")`, the paths in `hooks.py`. Your
editor cannot follow a string, so you use grep. frappe-lsp reads your bench
and makes those strings work like code.

```python
def close(name):
    invoice = frappe.get_doc("Sales Invoce", name)
    #                         ^^^^^^^^^^^^ no DocType named `Sales Invoce`
    customer = frappe.db.get_value("Sales Invoice", name, "custmer")
    #                                                     ^^^^^^^^^ `Sales Invoice` has no field `custmer`
```

## Features

- **Go to definition.** Jump from a DocType name to its controller, from a
  fieldname to the DocType JSON, and from a `hooks.py` path to the function.
- **Completion.** DocType names, fieldnames, hook keys, doc events,
  whitelisted methods and Jinja globals. It also works on a line you have not
  finished.
- **Hover.** See what a DocType, field, hook or path is, and which app
  defines it.
- **Find references.** Find every use of a DocType or a field in Python,
  JavaScript, DocType JSON, `hooks.py` and fixtures.
- **Checks.** Find unknown DocTypes, misspelled fields, hooks that never run and
  patches that do not resolve. Each check is tested on all of `frappe` and
  `erpnext`, so it stays quiet on correct code.
- **Test lenses.** Click "Run test" or "Run class" above a test to run
  `bench run-tests` for it in the editor's terminal.
- **Outlines.** See the structure of DocType JSON, `hooks.py` and
  `patches.txt`.
- **Commands for agents and scripts.** `frappe-lsp check`, `describe` and
  `references` give the same answers in a shell, for a coding agent, CI or a
  pre-commit hook. See [From a shell](#from-a-shell).

It works in Python, JavaScript, DocType JSON and Jinja templates, in every app
of the bench. It needs no configuration.

## Quick start

Pick your editor below, then open any file inside a bench. The Neovim, Claude
Code and pi plugins download the server on first use, so there is nothing else
to install.

### Neovim

You need Neovim 0.11 or later. Add the plugin with lazy.nvim:

```lua
{
  'ssiyad/frappe-language-server',
  config = function()
    require('frappe_lsp').setup()
  end,
}
```

The first file you open in a bench downloads the server. Updating the plugin
updates the server.

### Claude Code

```sh
claude plugin install frappe-lsp --marketplace ssiyad/frappe-language-server
```

Claude can then navigate the bench, and it sees the checks after each edit.
The plugin also puts `frappe-lsp` on Claude's `PATH`, and its skill has
Claude run `frappe-lsp check` to review code and `frappe-lsp describe` before
it writes fieldnames.

### pi

```sh
pi install git:github.com/ssiyad/frappe-language-server
```

The agent gets tools for DocType schemas and references. Each file it edits
is checked on that turn.

### VS Code

1. Install the [Simple LSP Client](https://marketplace.visualstudio.com/items?itemName=wdomitrz.simple-lsp-client)
   extension.
2. [Install the binary](#install-the-binary).
3. Add this to your `settings.json`:

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

### Other editors

Register a stdio language server with the command `frappe-lsp` for Python,
JavaScript, JSON and HTML files. [docs/editors.md](docs/editors.md) has the
details for every editor.

### From a shell

The same binary answers one question and exits. Run it anywhere inside a
bench:

```sh
frappe-lsp check apps/press/press/api        # files or directories
frappe-lsp describe "Sales Invoice"          # a DocType's schema
frappe-lsp references "Sales Invoice" customer
```

`check` prints `path:line:column: severity: message [code]` and exits 1 when it
finds something, so it works in CI and in a pre-commit hook. Add `--json` to
any command for machine-readable output. The commands share the editor's index
cache, so a warm run takes a fraction of a second.
[docs/specs/26-command-line.md](docs/specs/26-command-line.md) has the details.

## Install the binary

The plugins download the binary for you. For VS Code and other editors, get it
in one of these ways.

**Download a release.** Each [release](https://github.com/ssiyad/frappe-language-server/releases/latest)
has an archive for Linux and macOS, x86_64 and aarch64. The Linux binaries are
static, so they run on any distribution.

1. Download the archive for your platform and `SHA256SUMS`.
2. Check the archive. On macOS, use `shasum -a 256` instead of `sha256sum`.

   ```sh
   sha256sum -c --ignore-missing SHA256SUMS
   ```

3. Unpack it, and move `frappe-lsp` to a directory on your `PATH`.

   ```sh
   tar xzf frappe-lsp-*.tar.gz
   mv frappe-lsp-*/frappe-lsp ~/.local/bin/
   ```

**Build from source.** You need Rust 1.95 or later.

```sh
cargo install --locked --git https://github.com/ssiyad/frappe-language-server frappe-lsp
```

## Configuration

The defaults work on any bench. To change something, add a `.frappe-lsp.toml`
to the bench root. For example, to turn off one check:

```toml
[diagnostics]
unknown-form-event = "off"
```

To silence one line, add a `frappe-lsp: ignore` comment to it.

frappe-lsp writes an index cache to `.frappe-lsp/` in the bench root. Add that
directory to your `.gitignore`.

[docs/editors.md](docs/editors.md) also shows how to get
[typed documents in pyright](docs/editors.md#typed-documents-in-pyright) and
how to [read a live site](docs/editors.md#reading-a-live-site).
[30](docs/specs/30-configuration.md) lists every key.

## FAQ

**Does it replace pyright, ruff or the TypeScript server?**
No. frappe-lsp only knows about Frappe. Keep your Python and JavaScript
servers for types, imports, formatting and renames, and run frappe-lsp next to
them.

**Does it change my files or my site?**
It reads your bench and never writes to a site. It changes your code only
when you apply one of its code actions, and it runs something only when you
click a test lens.

**How fast is it?**
On a bench with `frappe`, `erpnext` and six more apps, the first index takes
about 2 seconds. Later starts read the cache in under half a second. Most
requests answer in less than a millisecond.

**Which Frappe versions does it support?**
It reads the version from the bench. It does not assume one. It is tested on
the `develop` branch of `frappe` and `erpnext`.

**Something is wrong. What do I do?**
In Neovim, run `:FrappeLspStatus`. It shows what the server found in your
bench. [docs/editors.md](docs/editors.md#when-something-is-wrong) shows how to
get the log. Attach both to a
[bug report](https://github.com/ssiyad/frappe-language-server/issues).

## Contributing

Bug reports and pull requests are welcome.
[docs/development.md](docs/development.md) explains how to build, test and
release. The design is in [docs/specs](docs/specs/index.md), and the
[roadmap](docs/roadmap.md) shows what comes next.

## License

[GPL-3.0](LICENSE)
