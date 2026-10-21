---
max_turns: 30
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, ToolSearch, LSP, Bash]
---

The bench is in the current directory. I want to rename the `update_job` field of the Site Update DocType in apps/press.

List every line in apps/press (Python, JavaScript and DocType JSON) that reads, writes, filters on or names this field. Put one location per line as `path:line`. Leave out anything that is not this field, such as functions or variables that have the same name.
