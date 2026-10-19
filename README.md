# frappe-lsp

A language server for [Frappe Framework](https://docs.frappe.io/framework) apps.

Frappe connects its parts with strings. `frappe.get_doc("Sales Invoice")`,
`doc_events`, `frappe.ui.form.on("Task")` and every `hooks.py` path name things
that only exist at runtime, so an editor cannot follow them. frappe-lsp indexes
the bench and turns those strings into links.

## What it does

- **Go to definition** from a DocType name to its controller, from a fieldname
  to its field in the DocType JSON, and from a dotted path in `hooks.py` or
  `frappe.call` to the function.
- **Hover** on a DocType, a field, a hook or a path: what it is and which app
  defines it.
- **Completion** of DocType names, fieldnames, hook keys, doc events and
  scheduler frequencies, also on a line you have not finished typing.
- **References** to a DocType or a field across Python, JavaScript, DocType
  JSON, `hooks.py` and fixtures.
- **Diagnostics** for names that do not exist: an unknown DocType, a misspelled
  field, a hook frappe never reads, a patch that does not resolve. Each check
  is measured against `frappe` and `erpnext`, and the defaults stay quiet on
  correct code.
- **Test lenses** on test classes and methods. A click runs
  `bench run-tests` for that scope in the editor's terminal.
- **Outlines** of DocType JSON, `hooks.py` and `patches.txt`.

It works on Python, JavaScript, DocType JSON and Jinja templates, in every
installed app of the bench, with no configuration. In a template, `include`
and `extends` paths go to the file, and `frappe.*` calls work as in Python.

## What it does not do

It is not a Python or JavaScript language server. Keep pyright, ruff and
typescript-language-server for types, imports, formatting and renames, and run
frappe-lsp beside them. It does not edit files, rename fields across a bench,
or write to a site. The one thing it starts is a test run, and only when you
click a lens. The full list is in [00](docs/specs/00-overview.md).

## Install

Tagged releases carry one binary per platform — Linux and macOS, x86_64 and
aarch64 — with a `SHA256SUMS` file. The Linux builds are static. Download the
archive for your platform from the
[releases page](https://github.com/ssiyad/frappe-language-server/releases),
check it against `SHA256SUMS`, and put `frappe-lsp` on your `PATH`.

To build it instead, you need Rust 1.95 or later. From a checkout:

```sh
cargo install --locked --path crates/frappe-lsp
```

Or straight from the repository:

```sh
cargo install --locked --git https://github.com/ssiyad/frappe-language-server frappe-lsp
```

The result is one binary, `frappe-lsp`, that links nothing beyond the C
library.

## Set up your editor

Point your editor's LSP client at the `frappe-lsp` command for Python,
JavaScript and JSON files. [docs/editors.md](docs/editors.md) has the
configuration for Neovim and Helix, and what any other client needs.

Open a file in any app of a bench. The first index of a bench with `frappe`,
`erpnext` and six more apps takes about two seconds. Later starts read a cache.

## When something is wrong

Ask the server for `frappe-lsp/indexStatus` first, as
[docs/editors.md](docs/editors.md#when-something-is-wrong) shows. Attach its
answer and the server log to a bug report.

## Status

The index, navigation, completion, references, diagnostics and test lenses are
done. A live site can be read too, and the bridge is off by default: its Custom
Fields, Property Setters, DocTypes created through the UI, and Client and
Server Scripts ([05](docs/specs/05-live-bridge.md)). The
[roadmap](docs/roadmap.md) has the rest.

## Development

[docs/development.md](docs/development.md) covers the toolchain, the test
suites, and the checks that run against a real bench.

## License

GPL-3.0. See [LICENSE](LICENSE).
