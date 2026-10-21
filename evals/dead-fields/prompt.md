---
max_turns: 30
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, ToolSearch, LSP, Bash]
---

The bench is in the current directory. In apps/press/press/api/site.py, some `frappe.db.get_value` or `frappe.get_value` calls ask for a field that the DocType does not have. Find them.

For each one, give the location as `path:line`, the DocType, and the missing field.
