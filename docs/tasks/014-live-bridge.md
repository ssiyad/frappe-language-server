# 014 — Live bridge

**Phase** 4 · **Depends on** —

[05](../specs/05-live-bridge.md). Layer 3 of [04](../specs/04-metadata-index.md).

## Done already

The transport, the merge and the confidence model. `GET`-only against
`/api/v2` with a token from the environment, paged at 500 with a stable order
and a ceiling; Custom Field and Property Setter merged as Layer 3 after Layer 2
so the live site wins; eviction keyed on provenance having no file, so a
re-sync drops what the last one added; `Static` / `Full` / `Degraded`
transitions, with `Degraded` suppressing the absence checks;
`frappe-lsp/refreshBridge`; `indexStatus` reporting the site.

DocTypes created through the UI, each read again from its meta for its fields,
and merged only where no installed app defines the same name.

Credentials from the environment or from `bridge.token_command`, which is any
command printing `<key>:<secret>`. `bridge.refresh_interval` runs a timer that
re-reads its own configuration, so setting it to `0` stops it.

Tested against a socket rather than a bench site — a database is not a fixture.
`frappe-bridge`'s tests assert the request is a `GET`, carries the token, pages
correctly and treats a refusal as an error rather than an empty sync;
`frappe-index`'s assert the merge, the eviction, and that the two off states
differ; and one integration test drives a server process against a fake site
for the task's "done when", with a control asserting the same line *is*
reported when no bridge is configured.

## Scope

- **Client Script and Server Script**, and the read-only
  `frappe-lsp://script/…` virtual documents they would be reached through.


## Done when

A field added through Customize Form completes without a restart, hover reports
it as `Custom Field · <site>`, and stopping the site suppresses unknown-field
diagnostics rather than flooding them. **Met**, by
`a_customize_form_field_arrives_without_a_restart`.
