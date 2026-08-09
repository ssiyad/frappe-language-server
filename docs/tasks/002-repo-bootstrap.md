# 002 — Repository bootstrap

**Phase** 2 · **Depends on** 001 · **Blocks** everything

Cargo workspace, CI, and a server that starts and does nothing.

## Scope

- Workspace with the four crates from [development](../development.md).
- `tower-lsp` server: `initialize` advertising capabilities per
  [30](../specs/30-configuration.md), `shutdown`, incremental `textDocumentSync`.
  No features yet.
- Config loading and merge from all four sources ([30](../specs/30-configuration.md)).
- Logging to stderr; assert nothing but protocol reaches stdout.
- CI: `fmt`, `clippy -D warnings`, `test`, on Linux and macOS.
- `xtask` skeleton.

## Done when

The binary starts under Neovim's built-in LSP client, negotiates capabilities,
and stays alive through an edit session without errors in the log.
