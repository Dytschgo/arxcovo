# M2 activity ID lookup

Provider: OpenAI / gpt-6.1-sol / high. Base: `3b806a130dfeba8044bc519d3ef15ee5f2c0bd4c`.
Isolated `t3/m2-activity-id-lookup` checkout was clean before edits.

`list_activity` now accepts optional `activity_id` (positive signed 64-bit integer).
It is mutually exclusive with `before_id`, in both the advertised schema and
runtime validation. Null, malformed values and unknown fields retain the fixed
`invalid activity arguments` error. Example: `{"activity_id":123}`.

The response remains `activities` plus `next_before_id`. Exact lookup returns
zero or one row and a null cursor. The existing query and bounded row decoder
still enforce authenticated principal, agent actor, ID below the caller's own
committed read, read filtering, typed metadata and field bounds. Read records
require `include_reads:true`. Missing, pruned, future and invisible IDs return
an empty page. Existing pagination, default limit and maximum limit remain.
The existing store `list` API delegates to the shared filtered query so callers
outside this scope require no changes. No schema migration is needed.

## Local verification

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings`: passed.
- `cargo test --locked --offline -p arxd --lib mcp::activity_tests -- --nocapture`:
  passed (one SDK duplex E2E test, private temporary store and existing fake
  dispatcher). Exercises returned command IDs, exact action/read lookup,
  absent/future/own-read/other-agent/lifecycle invisibility, cursor pagination,
  defaults, strict malformed arguments and conflict rejection, committed caller
  validation, tampered oversized records and completion-failure withholding.
- Eight existing pure temporary-store tests passed, each selected with
  `cargo test --locked --offline -p arxd --test inventory activity_v2::<name> -- --exact`:
  `keyset_isolation_own_row_and_exact_boundaries`,
  `read_flood_preserves_actions_and_lifecycle`,
  `exactly_once_completion_rolls_back_duplicate_missing_and_repeated`,
  `v1_migration_is_lossless_and_failure_is_atomic`,
  `corrupt_values_are_bounded_rejected_and_not_echoed`,
  `extra_or_missing_columns_refuse_without_exposing_values`,
  `schema_checks_and_typed_metadata_reject_unsafe_values`,
  `v3_development_constraints_and_v2_migration_preserve_identity_sequence`.

The first E2E run failed at the fixture's second SQLite connection write because
it used a different journal mode. Setting that connection to the service
fixture's existing TRUNCATE mode fixed the test; production journal code is
unchanged.

Independent Grok review and normal required gates remain pending with the
coordinator. Nothing was published. No network, native process, service,
owner-account, VM, installation, private-state or CI dispatch was exercised.
Hardware paths remain **unverified on real hardware**. M2/PA6, Linux F/G OPEN
and optional diagnostic tracks remain parked; this only adds command/feed
correlation. D7a documents, ADRs and PROGRESS were outside the authorized scope.
