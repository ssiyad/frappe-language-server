# 016 — Distribution

**Phase** 5 · **Depends on** 014

## Scope

- Release binaries for Linux and macOS, x86_64 and aarch64. Static where
  possible — the point of choosing Rust was a dependency-free drop-in.
- Tagged releases with checksums; `cargo install` path.
- Setup docs: `nvim-lspconfig` registration first, then Helix, Zed, VS Code.
- A README that states what the server does and does not do
  ([00](../specs/00-overview.md)) — the non-goals matter most to a new user
  deciding whether it replaces pyright.
- Troubleshooting: `frappe-lsp/indexStatus` output as the first thing to attach
  to a bug report.

## Done when

A developer with a bench installs the binary, adds a config block, and gets
completion — with no build step and no editor extension.
