---
max_turns: 40
timeout_seconds: 1200
allowed_tools: [Read, Glob, Grep, ToolSearch, LSP, Bash]
---

The bench is in the current directory. Audit the Python files in apps/press/press/api/ for code that names a field the DocType does not have: in `get_value`, `get_all` and `exists` calls, in filters and field lists, and in `get_doc` dicts.

For each one, give the location as `path:line`, the DocType, and the missing field.
