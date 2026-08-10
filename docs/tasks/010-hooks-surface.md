# 010 — Hooks surface

**Phase** 3 · **Depends on** — · **Blocks** 011, 012

[12](../specs/12-surface-hooks.md). Extraction, the vendored registry and the
cross-hook effects are done; what remains is wiring them into the index and
presenting them.

## Done already

Module-level assignment extraction with opaque non-literals, the vendored
registry (125 hook names with schemas, doc events, submit-only events,
scheduler frequencies, cron validation), the cross-hook effects as data, asset
path resolution, duplicate and unknown-key detection. `frappe` validates clean:
66 assignments, zero unknown keys.

## Scope

- Apply the cross-hook effects to the graph, rather than only reading them:
  `override_doctype_class` and `extend_doctype_class` into controller
  resolution ([04](../specs/04-metadata-index.md)), `override_whitelisted_methods`
  into the whitelist registry, `doctype_js` into the DocType's script list so
  [21](../specs/21-navigation.md) can offer it.
- Ordering: `hooks.py` indexed before any JS or template in the same app.
- Definition and hover on hook values — the dotted paths and assets already
  resolve; nothing surfaces them yet.
- Completion and hover for hook keys, doc events and frequencies
  ([20](../specs/20-completion.md), [22](../specs/22-hover.md)).

## Done when

Go-to-definition on a `doc_events` handler path lands on the function, hover on
a hook key names its schema, and a DocType whose controller is replaced by
`override_doctype_class` navigates to both classes.
