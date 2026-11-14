# Grok review: M2 activity ID lookup

2026-11-14; Grok (grok-4.7); single-agent source review. This reviewer did not write the feature. Standards and Spec stay separate. No checkout, test, build, or GitHub review of an activity PR was performed. The builder's fmt, Clippy, SDK duplex, and store-test results are credited, not rerun.

**Disposition: the four-file feature delta matches the requested lookup. It is not a merge candidate from its current parent, and it grants no activation.**

## Pins

- Feature `026b08986d0d57cc72ed7e190880e4147e17c9bb` is retained as `32c236c610f7928b8842610c546dc76352845c2c`. Both have parent `3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c` and tree `8f0fdf1cd5e7a0bf89042c6f5ef72ce2bd779223`. `git diff 026b08986d0d57cc72ed7e190880e4147e17c9bb 32c236c610f7928b8842610c546dc76352845c2c` is empty.
- Delta against that parent, +314/−6: `crates/arxd/src/mcp.rs` blob `f83acbc8b5da33577296a797c2dbfcefd18f396f`, `crates/arxd/src/store.rs` blob `f223e99c6bdb57569557e384bec828cf5c40a3ab`, new `crates/arxd/src/mcp/activity_tests.rs` blob `f5fc8a7dac99c21653a732e46a8605c10a747213`, new `docs/evidence/m2-activity-id-lookup.md` blob `0a5b3b88bf3d23f05266a2a9f51cc114547449be`.
- At the parent, `crates/arxd/src/mcp.rs` and `crates/arxd/src/store.rs` match main `82162956839740753a0b97a790c04b76784e7c9a`. The parent itself also changes `docs/PROGRESS.md`, ADR 0019, `docs/evidence/linux-stagef-a1a2-source-alignment.md`, and two Stage F review documents. Those docs are outside this feature delta and remain the ineligible D7a tree.
- `id` stays `INTEGER PRIMARY KEY AUTOINCREMENT`. Public `Activity::list` delegates to `list_filtered(..., None)`.

## Standards

Pass. The new argument is removed before `ListActivity` deserializes, and `ListActivity` keeps `deny_unknown_fields`. `list_filtered` is the one query both the old pagination path and the exact lookup use. Names are the existing static column list. No baseline smell is worth a change. Formatting and `clippy -D warnings` are the builder's report.

### Security

The lookup value is a bound `?5`, not interpolated SQL. Principal, cursor, include-reads flag, and limit stay bound. Column identifiers still come from the fixed name list. Decode failure, store rejection, and completion failure all map through `unavailable()`, which is the fixed internal error `activity unavailable` with no payload. Invalid arguments return `invalid activity arguments` with no error data. The tests assert a `PRIVATE-MARKER` in a bad argument or an oversized tampered `tool` does not appear in the client error. The schema keeps `additionalProperties: false`. No approval flow, owner action, native launcher, or service activation was added.

0 findings.

## Spec

The requested behavior is present. Optional `activity_id` is a positive `i64` (`minimum` 1, `maximum` `i64::MAX`) and is mutually exclusive with `before_id` in the advertised schema (`not` / `required`), the handler filter, and `list_filtered`. The transport principal is the only principal. The query keeps `actor_kind='agent'`, `principal=?1`, `id < min(before_id, read_id)`, and read exclusion unless `include_reads` is true. `read_id` must be that principal's own `list_activity` row still in `outcome='started'`. Daemon lifecycle rows use actor kind `daemon` and principal `arxd`, so the tested lifecycle id is invisible. A missing, deleted, future, other-agent, or own-read id yields an empty page. A primary key makes the match at most one row, so `next_before_id` stays null, including `limit: 1`. Existing `list`, limit bounds, keyset `LIMIT limit+1`, and the tampered-value CASE decoder are unchanged. `finish(id, "succeeded")` runs before the page is serialized; a completion failure returns `activity unavailable` and does not build the success response.

The SDK test drives a real in-memory duplex and the existing fake dispatcher. It checks the schema, command-id correlation, exact action and read hits, the invisible ids above, cursor pagination, defaults, malformed arguments, conflict rejection, the store's own-read and future-id fences, oversized tampered decoding, and completion-failure withholding. The eight named `activity_v2` tests are still present and were not modified. The builder reports those tests, `cargo fmt --all -- --check`, and locked offline Clippy passed. This review did not rerun them.

### Security

Missing and invisible rows are empty successes. Bad types, nulls, non-integers, `i64` overflow, zero, negatives, unknown fields, and `before_id` conflicts are rejected before the query. The caller's own open read id cannot be selected. Another principal's read id cannot be used as the cursor. Lifecycle rows created by `start_lifecycle` are not agent rows. No raw SQL error, trigger text, or stored field is returned. The page is released only after the success completion commits.

0 findings.

## Advisories

- ADR 0011 still says `list_activity` accepts only `limit`, `before_id`, and `include_reads`. This slice's evidence records the new argument and states ADR edits were out of scope. The behavior matches the requested lookup. The canonical contract sentence is stale until a later ADR amendment.
- `32c236c610f7928b8842610c546dc76352845c2c` must not be merged as a branch. Its parent is the ineligible docs tree `3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c`. Carry only the four-file delta onto a main that still has the reviewed execution bytes.
- M2, Stage F, Stage G, and real-Runner PA6 stay open. Hardware stays **unverified on real hardware**. Optional diagnostic tracks stay parked. This lookup does not activate execution or accept M2.

## Verdict

- **Behavior:** the retained tree implements one scoped exact lookup with the existing fences.
- **Merge:** not from this parent. No activity PR was reviewed or published.
- **Tests:** builder-reported; not rerun here.
- **Standards:** pass. 0 findings.
- **Spec:** pass. 0 findings.

Standards 0. Spec 0. Worst issue: none.
