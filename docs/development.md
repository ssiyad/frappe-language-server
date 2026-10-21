# Development

## Toolchain

Rust stable, edition 2024. `rustfmt` and `clippy` (`-D warnings`) are enforced in
CI. MSRV is the current stable minus two releases.

## Layout

```text
crates/
  frappe-lsp/      binary — protocol handlers, benches  [02] [31]
  frappe-index/    discovery, metadata graph, cache     [03] [04]
  frappe-syntax/   grammars, queries, extraction        [06]
  frappe-bridge/   live site client                     [05]
bin/frappe-lsp     launcher: downloads the pinned release, then runs it
lua/ lsp/          Neovim plugin
pi/                pi package: extension and skill
claude/            Claude Code plugin skill
.claude-plugin/    Claude Code plugin and marketplace manifests
docs/
  specs/           normative design
  tasks/           open work only
tests/
  fixtures/        fixture bench                        [32]
  editors/         Neovim end-to-end check
xtask/             drift check, smoke, stress, stub     [31] [32]
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
| Panic boundary | `tower`, `futures` — already in the tree through `tower-lsp-server` |
| Async runtime | `tokio` |
| Parsing | `tree-sitter` + language grammars |
| Buffers | `ropey` |
| JSON | `serde_json` |
| Index cache | `bincode` |
| Watching | `notify` |
| Parallelism | `rayon` |
| Snapshots | `insta` |
| Benchmarks | `criterion` |
| HTTP (bridge) | `reqwest` + `rustls`, no default features |

Adding a dependency needs a line in the PR on what it replaces. Compile time and
binary size are features here — the server is expected to start in milliseconds.

`reqwest` is the largest thing in the tree and it costs **3.0 MiB** of release
binary — 8.8 to 11.8 — for a feature that is off by default. There is no
keyring dependency: credentials come from the environment or from a command the
user names ([05](specs/05-live-bridge.md)), which is one fewer platform
integration and one fewer soft dependency on somebody else's tool. It is the price of
the bridge ([05](specs/05-live-bridge.md)) being a supported configuration
rather than a separate build, and `rustls` over the system OpenSSL is what
keeps the result one artifact. Default features are off: the default pulls a
cookie store, gzip and a blocking client the bridge never uses.

`tower-lsp-server` is the maintained fork of `tower-lsp`, which has not shipped
since August 2023 and is pinned to `lsp-types` 0.94. The API is the same shape;
`ls_types` replaces `lsp_types`, and `Uri` replaces `Url`.

The command line is hand-rolled — `--log-level`, `--stdio` and three
subcommands with one flag between them ([26](specs/26-command-line.md)) do not
justify a parser dependency.

`bincode` is pinned to 2.0.1. Version 3.0.0 exists on crates.io and its entire
source is `compile_error!`; it is a squat, not an upgrade.

## Vendored constants

Fieldtypes, `Document` members, hook names, form and doc events, scheduler
frequencies and Jinja globals are tables written by hand. They cannot be
generated whole: a hook's name can be scraped, but what its value names is a
judgement. Rationale in [31](specs/31-performance.md).

Drift is detected instead. `cargo xtask codegen --check [FRAPPE]` scrapes what
a `frappe` checkout reads: hook names from five sources, doc events from
`run_method`, form events from `trigger`, and `Document` members from the
classes. It fails on any name a table lacks. Add the name with its schema, and
run the check again. `.github/workflows/drift.yml` runs it weekly against
frappe's `develop`.

Its first run found 23 hooks, three doc events and one form event missing.
Each one was a false positive, or a dead path waiting for an app that used it
([23](specs/23-diagnostics.md)).

## Testing

```sh
cargo test              # unit, golden, integration
cargo insta review      # inspect snapshot diffs
cargo bench -p frappe-lsp   # one benchmark per budget of [31]
cargo xtask smoke       # real bench, against tests/smoke-allowlist.toml
cargo xtask stress      # real bench, driven like an editor
nvim --clean --headless -l tests/editors/neovim.lua [BENCH]
```

Tests that need a real bench skip when there is none, and read one from
`FRAPPE_LSP_REFERENCE_BENCH`, defaulting to `~/sources/frappe-dev`. CI has no
bench, so those assertions are only as good as the developer who last ran them.

`cargo xtask smoke` takes the same bench and checks `frappe` and `erpnext` only
— your own apps are yours, and `--all` widens it for a local look. It fails on
any finding not in `tests/smoke-allowlist.toml`; `--write` regenerates that file
and the reasons in it are reviewed by hand before committing. What is on the
list and why: [32](specs/32-testing.md).

`tests/editors/neovim.lua` drives the release binary from stock Neovim over
the reference bench, or over `tests/fixtures/bench` when you name it.
`FRAPPE_LSP` points it at another binary.

Levels and what belongs in each: [32](specs/32-testing.md).

## Conventions

- Conventional Commits with no scope, and a summary that names the change:
  `fix: a wide character before a Jinja global`.
- Comments explain why, never what. Spec references over restated prose:
  `// Layer 3 evicts without a rebuild — see specs/04`.
- `unwrap`/`expect` only where a panic is genuinely unreachable, with the reason
  in the message. A panic in a request is caught at the service boundary and
  answered with an error ([02](specs/02-architecture.md)), but the user still
  loses that answer, and a panic on a background task loses the work.
- `cargo xtask stress` drives the release binary like an editor over a real
  bench: every file opened, asked about and typed into. It fails on a crash, a
  hang, an error response or a panic in the log. Run it after touching request
  handling.
- Errors carry the file path that caused them.
- Public items in `frappe-index` and `frappe-syntax` carry doc comments; the
  binary crate does not need them.

## Releasing

1. Run `cargo xtask stress` against a real bench. CI only drives the fixture
   bench.
2. Set the new version in four places: `version` in the workspace
   `Cargo.toml`, the three `version` pins under `[workspace.dependencies]`,
   `package.json` and `.claude-plugin/plugin.json`.
3. Build, so `Cargo.lock` follows, and commit as `chore: release <version>`.
4. Tag the commit `<version>` and push the commit and the tag.

`.github/check-versions.sh` fails CI when the manifests disagree. The
launcher downloads the release `package.json` names, so a plugin on `master`
runs the last published release. Bump the version only in the release commit:
the plugins on that commit cannot find their server until the release job
publishes it, and they run the previous one until then.

`.github/workflows/release.yml` refuses a tag that does not match the version,
then builds and publishes Linux (static) and macOS binaries for x86_64 and
aarch64 with their checksums. Each archive is unpacked and driven through
`tests/editors/neovim.lua` on its own platform first, through the launcher.
After publishing, the launcher downloads the new release on Linux and macOS. To check the build
without publishing, run the workflow by hand:

```sh
gh workflow run release.yml --ref master
```

A macOS arm64 job is sometimes cancelled because GitHub has no runner for it.
Run `gh run rerun <id> --failed` to try that job again.
