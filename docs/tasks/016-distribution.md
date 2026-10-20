# 016 — Distribution

**Phase** 5 · **Depends on** 014

## Done already

`README.md` says what the server does and does not do. `docs/editors.md` has
the Neovim and Helix setup, what any other client needs, the `runTest` glue
and the troubleshooting steps. The Neovim block is the one
`tests/editors/neovim.lua` runs against a real bench. `cargo install --path
crates/frappe-lsp` builds one binary that links only the C library.

`.github/workflows/release.yml` builds Linux (static, musl) and macOS,
x86_64 and aarch64, on a `<version>` or `v<version>` tag. It refuses a tag that does not match
the crate version, runs each binary its runner can execute, and publishes the
archives with a `SHA256SUMS` file. 0.1.1 is the first release: all four
archives match their checksums, and the x86_64 Linux binary is static and
passes `tests/editors/neovim.lua` against the reference bench.

## Scope

- Run the aarch64 Linux and both macOS archives on their platforms; CI only
  runs `--version` on two of them.
- The `--git` install path checked against a tag.
- Run the Zed extension in `editors/zed` and the VS Code setup in
  `docs/editors.md` in those editors; both are written and compile or parse,
  and neither has been run.

## Done when

A developer with a bench downloads a release binary, adds the block from
`docs/editors.md`, and gets completion — with no build step and no editor
extension.
