# Development

## Toolchain

Rust stable, edition 2024. `rustfmt` and `clippy` (`-D warnings`) are enforced in
CI. MSRV is the current stable minus two releases.

## Layout

```
crates/
  frappe-lsp/      binary — protocol handlers          [02]
  frappe-index/    discovery, metadata graph, cache    [03] [04]
  frappe-syntax/   grammars, queries, extraction       [06]
  frappe-bridge/   live site client                    [05]
docs/
  specs/           normative design
  tasks/           pending and in-progress work only
tests/fixtures/    fixture bench                       [32]
xtask/             constant generation, benchmarks     [31]
```

## Workflow

Spec first. A change to observable behaviour starts as a diff to `docs/specs/`,
in the same PR as the code. Specs that lag the code are worse than no specs — the
next person trusts them.

1. Pick a task from [tasks](tasks/index.md); mark it in progress.
2. Implement against the referenced specs.
3. Golden tests including negative cases ([32](specs/32-testing.md)).
4. Update the spec if behaviour diverged from it; say why in the PR.
5. Delete the task file when done — the directory holds only open work.

## Dependencies

| Purpose | Crate |
| --- | --- |
| LSP transport | `tower-lsp-server` |
| Async runtime | `tokio` |
| Parsing | `tree-sitter` + language grammars |
| Buffers | `ropey` |
| JSON | `serde_json` |
| Watching | `notify` |
| Parallelism | `rayon` |
| Snapshots | `insta` |
| Benchmarks | `criterion` |
| HTTP (bridge) | `reqwest`, `rustls` |

Adding a dependency needs a line in the PR on what it replaces. Compile time and
binary size are features here — the server is expected to start in milliseconds.

`tower-lsp-server` is the maintained fork of `tower-lsp`, which has not shipped
since August 2023 and is pinned to `lsp-types` 0.94. The API is the same shape;
`ls_types` replaces `lsp_types`, and `Uri` replaces `Url`.

The command line is hand-rolled — `--log-level` and `--stdio` do not justify a
parser dependency in a process whose entire interface is stdio.

## Vendored constants

Fieldtypes, hook names, form and doc events, scheduler frequencies and Jinja
globals are generated from a `frappe` checkout by `cargo xtask codegen`, and
committed with the source commit hash. CI re-runs generation against `develop`
and fails on a diff. Never hand-edit the generated file. Rationale in
[31](specs/31-performance.md).

## Testing

```
cargo test              # unit, golden, integration
cargo insta review      # inspect snapshot diffs
cargo bench             # budgets from [31]
cargo xtask smoke       # real bench, clones frappe + erpnext
```

Levels and what belongs in each: [32](specs/32-testing.md).

## Conventions

- Conventional Commits; scope is the crate (`index`, `syntax`, `bridge`, `lsp`).
- Comments explain why, never what. Spec references over restated prose:
  `// Layer 3 evicts without a rebuild — see specs/04`.
- `unwrap`/`expect` only where a panic is genuinely unreachable, with the reason
  in the message. A panic in an LSP kills the session and the user loses the
  server without knowing why.
- Errors carry the file path that caused them.
- Public items in `frappe-index` and `frappe-syntax` carry doc comments; the
  binary crate does not need them.

## Ignore

Add `.frappe-lsp/` to `.gitignore` — the index cache
([31](specs/31-performance.md)) is written into the bench root.
