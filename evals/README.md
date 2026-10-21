# Evals

`claude plugin eval` cases for the Claude Code plugin, each run with and
without it. They ask an agent real questions about press, at pinned commits,
and grade the answer against ground truth checked by hand.

| Case | Asks |
| --- | --- |
| `doctype-schema` | The Link fields and title field of a DocType |
| `write-field-code` | A function that reads five fields of Site Update |
| `dead-fields` | Calls in one file that name fields a DocType lacks |
| `dead-fields-api` | The same across all 37 files of `press/api/` |
| `field-references` | Every use of `update_job` on Site Update, and nothing else |
| `status-references` | Site Update's `status` among 222 lines that say `status` |
| `review-planted-bug` | A review of a diff that names a field Site Update lacks |
| `hooks-paths` | One misspelt dotted path among 276 in `hooks.py` |

Each case is tagged `natural`. `lib/guided.sh` writes a `guided-<case>` copy
that tells the agent to load the skill: the natural run measures whether the
plugin gets used, the guided one what it is worth when it does.

## Setup

1. Write the path of a bench holding frappe and press to `evals/.bench`. The
   scaffold clones both from it at the commits pinned in `lib/bench.sh`, then
   warms the index with `lib/warm.py`.
2. To test a local build instead of the release `package.json` pins, write
   its path to `evals/.server`.
3. Install `bubblewrap` and `socat`. The cases may run `frappe-lsp` through
   Bash, and Claude Code refuses a Bash grant without its sandbox.

## Running

The eval refuses a plugin directory with hard links, and Cargo's `target/` is
full of them, so run from a copy. Each workspace is about 800 MB; keep it off
a tmpfs `/tmp`.

```sh
bash evals/lib/guided.sh
rsync -a --delete --exclude target --exclude .git --exclude evals/results ./ /tmp/frappe-lsp-plugin/
cd /tmp/frappe-lsp-plugin
TMPDIR=~/.cache/frappe-lsp-evals claude plugin eval . --scaffold --trust-plugin \
  --allow-tools LSP ToolSearch Write Edit "Bash(frappe-lsp *)" \
  --judge-model sonnet --disallow-tools Agent --max-cost-usd 20 --tag natural
```

`--disallow-tools Agent` keeps `dead-fields-api` from fanning out into
subagents at $5–6 a run. The default Haiku judge failed a correct review, so
use Sonnet. `aggregate-result.json` carries each run's turns, cost and
duration, which the docs do not list.
