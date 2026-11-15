# M2 completed command to persisted activity

Provider: OpenAI / gpt-6.1-sol / high.
Base: `fafb8d4f4bd460eff51328d781966d2e57f0fa22`.
Initial checkout was clean. This is a test-only coverage join, with no reproduced
production defect and no production changes.

PR84's `activity_id_command_correlation_visibility_and_pagination` already
roundtrips IDs, visibility, arguments, pagination, tampered rows and audit failure
withholding through the SDK. Its `FakeForTest` report always fails and its exact
command lookup checks ID/tool only. The local command bridge test also uses that
fake report and list pagination. Neither covers both terminal outcomes and the
complete command metadata/identity join through exact lookup.

Added one test in `crates/arxd/src/mcp/activity_tests.rs`:
`completed_command_sdk_exact_lookup_preserves_outcome_and_identity`.
Two cases use existing `default_fixture`, `OnceReport`, `good` and `completed`
fixtures, injecting typed reports with exit codes 0 and 7 into the existing
dedicated dispatcher. This exercises the real outcome assessment, audit
completion, MCP handler, official rmcp SDK serialization and real temporary
SQLite store. No native executor or command process is invoked.

For each case the test checks:

- SDK `run_command` returns a positive ID, expected succeeded/failed outcome,
  exit code, synthetic pinned identity, rights, program, argument count and
  terminal facts.
- A separate SQLite connection sees the terminal row and correct principal
  immediately after the response, before the lookup.
- After closing the command session and reopening the store, exact SDK lookup
  returns that same ID and outcome, all metadata fields and every observed
  identity field, plus completion timestamps and a null pagination cursor.
- A separate SDK session with another server-bound principal receives the
  empty page for that returned ID, for both outcomes.

Call graph checked before running tests: Inventory calls the configured
dispatcher; `default_fixture` replaces its backend with `OnceReport`; its
preflight returns directly and `run` consumes the typed report. Private temporary
store setup sets only its own directory permissions. Reopened lookup has no
executor attached. No native/service/helper/owner/VM execution, network,
publication or CI dispatch is involved.

Development responses are outside this dedicated producer join: their existing
`development_current_user` mode and `development_only_not_dedicated_account`
claim are unchanged; no dedicated identity, outcome or identifier is fabricated
for them. Null observed identities and dedicated unavailability remain unchanged.
Authentication, audit-before-payload, argument bounds, read fences and output
limits are unchanged. Existing PR84 audit-withholding coverage is retained.

## Checks

- `cargo test --locked --offline -p arxd --lib mcp::activity_tests -- --nocapture`:
  passed, 2 tests, 0 failed, 404 filtered out (0.37 seconds after compilation).
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings`: passed.
- `git diff --check`: passed.

Independent review and hosted checks remain with the coordinator. PROGRESS and
ADRs are outside this slice's allowed files; this evidence records its task
outcome for coordinator incorporation. This establishes no M2 or dedicated-account
acceptance. Hardware paths remain **unverified on real hardware**.
