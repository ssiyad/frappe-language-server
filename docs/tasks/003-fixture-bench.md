# 003 — Fixture bench

**Phase** 2 · **Depends on** 002 · **Blocks** 004, 005, 006

The test corpus everything else asserts against. Built early on purpose — writing
it forces the edge cases in [04](../specs/04-metadata-index.md) into the open
before code depends on a wrong assumption.

## Scope

`tests/fixtures/bench/` per [32](../specs/32-testing.md): two apps, an inter-app
DocType reference, child / Single / submittable / virtual DocTypes, Link +
Dynamic Link + `fetch_from` + `Table` fields, a `hooks.py` covering every value
schema in [12](../specs/12-surface-hooks.md), `patches.txt`, a fixtures file with
a Custom Field, and one malformed JSON.

Include the deliberate errors needed by [23](../specs/23-diagnostics.md), each
annotated with the code it should produce — and the near-miss valid cases that
MUST stay silent.

## Done when

The tree exists, is documented by a README naming what each artifact exercises,
and every DocType JSON is byte-shaped like real Frappe output.
