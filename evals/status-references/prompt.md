---
max_turns: 30
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, ToolSearch, LSP, Bash]
---

The bench is in the current directory. In apps/press/press/press/doctype/site/site.py, list every line that reads, writes or filters on the `status` field of the Site Update DocType. Leave out the status of Site and of every other DocType.

Put one location per line as `site.py:<line>`.
