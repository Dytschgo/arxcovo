# M2 command/activity join independent review (test-only)

Reviewer: Muse Spark (muse-spark-1.3, high), independent of the Sol builder.
Single-agent read-only review. No M2/dedicated-account acceptance; hardware paths
**unverified on real hardware**.

Scope: `fafb8d4f4bd460eff51328d781966d2e57f0fa22...b4fa47654b6add335a94c8ef02b7d0e928931e02`
(3 files, +188/-0):

- `crates/arxd/src/mcp/activity_tests.rs` (+123): one new test
  `completed_command_sdk_exact_lookup_preserves_outcome_and_identity`.
- `docs/evidence/m2-command-activity-roundtrip.md` (+61, new): task evidence.
- `docs/PROGRESS.md` (+4): two `docs(progress)` lines (commits `e11ce95`/`b4fa476`).

Retained implementation under review: `1d89e24`
(`test(mcp): join completed commands to persisted exact activity`).
Sol source `45110a8c9ee071fc3d3cdceaa0b98e3f219f91e7` retained as `1d89e24`;
other range commits are PROGRESS-only.

## Standards

No findings. One-concern test commit plus PROGRESS/evidence lines, matching
the repo's small conventional-commit rule. `git diff --check` clean,
`cargo fmt --all -- --check` clean, offline Clippy
(`-p arxd --all-targets -- -D warnings`) clean. No production files touched
(`--numstat` shows only the three in-scope files), no secrets or owner data
(`SID_A` is the existing synthetic test SID, no `PRIVATE-MARKER` material),
no Lab Manager/VM/admin touch. No new public API, export, or alias.

## Spec

No findings. Against [ADR 0011](docs/adr/0011-scoped-activity-history.md:19)
(completion updates exactly one started row; result withheld on completion
failure) and the PR84
`activity_id_command_correlation_visibility_and_pagination` baseline (IDs,
visibility, pagination, tampered-row/audit-failure withholding through the
SDK; `FakeForTest` report always fails; exact command lookup checks ID/tool
only), the new test is meaningful and non-duplicative:

- Real `Inventory::call_tool("run_command")` over official rmcp SDK duplex,
  through the existing dedicated dispatcher (`crates/arxd/src/mcp.rs:103`
  `start_dedicated`, `:114` dedicated branch, `:145` `executor.dispatch`),
  with `default_fixture` swapping only the backend for `OnceReport`
  (`crates/arxd/src/service/executor/tests.rs:307`), whose `preflight`
  returns `Ok` and whose `run` consumes one typed `completed(good(...))`
  report. No native executor or command process runs.
- Both terminal outcomes in one loop: exit 0 -> `succeeded`, exit 7 ->
  `failed`, each asserting positive `activity_id`, `outcome`, `exit_code`,
  `dedicated_account` identity mode, pinned `SID_A`/`windows_sid`,
  `identity_matches_pin`, `exited`, `cleanup_confirmed`, `standard` rights,
  `program`/`argc`.
- Immediately after the response, a separate SQLite connection asserts the
  terminal row (`outcome,exit_code,principal`) with `finished_at_ms NOT NULL`
  — the "already committed" join.
- Session torn down (`cancel`, `drain`), store dropped, then
  `open_service_fixture` reopens the same directory with no executor attached
  (persisted history only). Two fresh SDK sessions per outcome assert exact
  `activity_id` lookup: same-agent returns `[id]`, null cursor, `tool`,
  `outcome`, `exit_code`, `finished >= started`, every `metadata` key, all
  six observed-identity mappings, `execution_mode == dedicated_account`,
  null `claim`; `other-agent` returns the empty page.
- Development/`development_current_user`, null-identity, dedicated
  unavailability, auth-before-payload, argument bounds, read fences, and
  output limits are untouched (zero production diff); PR84 audit-withholding
  coverage is retained, not replaced.

## Security

No findings. Fake-only by construction (typed `Report::Completed` injected;
`Native`/service/helper/owner/network paths not reached), production
unchanged, server-bound principals only, other-principal isolation asserted
for both outcomes, bounded columns/validation enforced by the unchanged store
layer. No network, publication, dispatch, push, or subagent action.

## Checks run (this review)

- `cargo test --locked --offline -p arxd --lib mcp::activity_tests -- --nocapture`:
  2 passed, 0 failed, 404 filtered out.
- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings`: passed.
- `git diff --check` on the review range: passed.

Counts: Standards 0; Spec 0; Security 0. Worst issue: none.
Disposition: PASS for the test-only join slice. No PROGRESS or source edits
made by this review.
