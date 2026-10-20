# frappe-lsp documentation

New here? Start with the [README](../README.md).

## Using frappe-lsp

| Page | What it covers |
| --- | --- |
| [Editor setup](editors.md) | Neovim, VS Code, Claude Code, pi and other clients, configuration, pyright stubs, troubleshooting |
| [Configuration](specs/30-configuration.md) | Every key of `.frappe-lsp.toml` |
| [Diagnostics](specs/23-diagnostics.md) | Every check, its code and its severity |

## Working on frappe-lsp

| Page | What it covers |
| --- | --- |
| [Development](development.md) | Toolchain, layout, tests, releases |
| [Guidelines](guidelines.md) | How these docs are written |
| [Specifications](specs/index.md) | The design, one concern per file |
| [Roadmap](roadmap.md) | Phases and what each one shipped |
| [Tasks](tasks/index.md) | Open work |

## Decisions

| Decision | Choice |
| --- | --- |
| Language | Rust: `tower-lsp-server` and `tree-sitter` |
| Index source | Static bench files, with an optional live site bridge |
| Editor target | Any stdio LSP client. Neovim, pi and Claude Code have plugins in this repository |
| Scope | Frappe semantics only. Python and JavaScript stay with pyright, ruff and the TypeScript server |
| Priority | Navigation first: grep is the problem to solve |
| Frappe version | Read from the bench, never assumed |
| License | GPLv3, as `erpnext`. [019](tasks/019-upstream.md) has the upstream consequence |

## Reference bench

The specs cite `~/sources/frappe-dev`: `frappe` and `erpnext` on `develop`
(17.0.0-dev) and six more apps. `cargo xtask smoke` and `cargo xtask stress`
run against it ([32](specs/32-testing.md)).
