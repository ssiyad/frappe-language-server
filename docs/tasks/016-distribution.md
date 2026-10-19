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
archives with a `SHA256SUMS` file. It has not run yet: that takes a tag.

## Scope

- Push the first tag, and check every archive on its platform: the static
  Linux builds in particular, which are built by `cross` and not locally.
- The `--git` install path checked against a tag.
- Setup for Zed and VS Code. VS Code needs an extension or a generic client,
  which [00](../specs/00-overview.md) keeps out of scope for now.

## Done when

A developer with a bench downloads a release binary, adds the block from
`docs/editors.md`, and gets completion — with no build step and no editor
extension.
