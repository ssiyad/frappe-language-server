---
max_turns: 15
timeout_seconds: 900
allowed_tools: [Read, Glob, Grep, ToolSearch, LSP, Bash, Write, Edit]
---

The bench is in the current directory. Create apps/press/press/utils/site_update_timing.py with a function `get_site_update_timing(name: str) -> dict`.

In one `frappe.db.get_value` call on Site Update, it reads when the update started, when it ended, how long it took, whether backups were skipped, and the backup type. It returns them as a dict.
