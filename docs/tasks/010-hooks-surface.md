# 010 — Hooks surface

**Phase** 3 · **Depends on** — · **Blocks** nothing

[12](../specs/12-surface-hooks.md). Extraction, the vendored registry and the
cross-hook effects are done; what remains is wiring them into the index and
presenting them.

## Done already

Module-level assignment extraction with opaque non-literals, the vendored
registry (125 hook names with schemas, doc events, submit-only events,
scheduler frequencies, cron validation), the cross-hook effects as data, asset
path resolution, duplicate and unknown-key detection. `frappe` validates clean:
66 assignments, zero unknown keys.

The Python side is done: the effects are applied to the graph, and `hooks.py`
navigates and hovers. What remains needs the JavaScript surface or completion.

## Scope

- `doctype_js` into the DocType's script list, so [21](../specs/21-navigation.md)
  can offer a form script bound from elsewhere in the app.
- Ordering: `hooks.py` indexed before any JS or template in the same app.
- Completion for hook keys, doc events and frequencies
  ([20](../specs/20-completion.md)).

## Done when

Completion inside a `doc_events` inner key offers the lifecycle events, and a
form script bound by `doctype_js` is a go-to-definition target on its DocType.
