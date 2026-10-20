# 016 — Distribution

**Phase** 5 · **Depends on** 014

## Done already

`README.md` says what the server does and does not do. `docs/editors.md` has
the Neovim plugin, the pi package, the Claude Code plugin, the VS Code setup,
what any other client needs, and the troubleshooting steps. The Neovim plugin
in this repository is what `tests/editors/neovim.lua` loads against a real
bench or the fixture bench. The VS Code block was checked by hand on VS Code
1.142 with Simple LSP Client 0.1.22. `cargo install --git` against the `0.1.1`
tag builds one binary that links only the C library and passes the Neovim
check.

`.github/workflows/release.yml` builds Linux (static, musl) and macOS,
x86_64 and aarch64, on a `<version>` or `v<version>` tag. It refuses a tag
that does not match the crate version and builds every target on a runner of
its own platform: `ubuntu-24.04-arm` for aarch64 Linux, `macos-15-intel` for
x86_64 macOS. Each archive is unpacked, checked against its checksum and
driven through `tests/editors/neovim.lua` over the fixture bench before the
archives are published with a `SHA256SUMS` file. A manual run builds and
checks every archive and publishes nothing. 0.1.1 is the first release.

## Scope

- A manual run of the release workflow passes on all four runners. Until it
  does, the aarch64 Linux and both macOS archives have not run on their
  platforms.

## Done when

A developer with a bench downloads a release binary, adds the block from
`docs/editors.md`, and gets completion — with no build step and no editor
extension.
