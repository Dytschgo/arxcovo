# M2 normal completion and cancellation reporting join

Provider: OpenAI / gpt-6.1-sol / high. Builder checkout head: `9047d80`;
source baseline: `1958f99`. The initial checkout was clean. Coordinator retained
only this slice as `b811ae6` directly atop `1958f99`; PR116 parent documentation
is excluded. The builder checkout ID is historical provenance, not landed parent.

## Audit and bounded gap

Read AGENTS, KICKOFF, brief 0020 (including A12), and the production call graph
before editing tests. No production defect was reproduced; this is test-only.

- Native producer: `Runner::run_phased` -> `run_core` -> `unix::run` ->
  `ChildGuard::kill_tree` -> `Group::kill_and_confirm`. The result reports cleanup
  plus reap confirmation. Group Drop latches residue on cleanup uncertainty.
  This native path was read, never executed.
- Inactive Unix adapter: `UnixBridge::body` checks residue before/after RunnerPort,
  latches readiness, then maps `RunResult` through `evidence` to `Report::Completed`.
  Production construction remains inactive.
- Reporting/storage: MCP `run_command` -> `Execution::dispatch` -> backend `run`
  -> `Execution::complete` -> `assess` -> `Activity::finish_typed` -> response.
  Exact lookup uses `Activity::list_filtered` and the real stored row.

Existing coverage already proves substantial parts; no duplicate suite added:

- [Residue flag ownership and source forwarding](../../crates/arxd/src/exec/residue_tests.rs):
  `independent_owned_domains_keep_distinct_monotone_runner_flags` and
  `cleanup_source_pins_release_sites_and_legacy_errno_order`.
- [Bridge reports and residue latches](../../crates/arxd/src/service/unix_bridge/tests/e6.rs):
  `t11_t12_t13_t13b_reports_and_latches` includes repeated **before-spawn**
  cancellation with a clear fake residue flag. It does not exercise native cleanup.
- [Bridge post-spawn caller cancellation](../../crates/arxd/src/service/unix_bridge/tests.rs):
  `t7_dropped_request_cancels_running_fake_and_joins` checks the real cancellation
  token and stored stop reason with a fake port whose residue is always false.
- [Durable typed report matrix](../../crates/arxd/src/service/executor/tests.rs):
  `adr0017_dispatch_durable_matrix` covers Cancelled/confirmed-cleanup assessment,
  direct SQL/list output, and quarantine state, but not SDK exact lookup after reopen.
- [Existing SDK roundtrip](../../crates/arxd/src/mcp/activity_tests.rs):
  `completed_command_sdk_exact_lookup_preserves_outcome_and_identity` already
  covers normal exits 0/7 through response, durable commit and reopened exact lookup.

The missing join is post-spawn **Cancelled + confirmed cleanup + null exit code**
through the SDK command response and reopened exact activity lookup. Extended the
existing SDK test by one case, reusing `good`, `completed`, `OnceReport` and
`default_fixture`. It checks failed outcome, null reason/exit code, cancelled stop,
confirmed cleanup, no quarantine, identity and rights; a separate SQLite connection
sees terminal stop/cleanup before response consumption proceeds to lookup. Reopened
SDK lookup checks every returned metadata/identity field, timestamps and principal
isolation. Existing normal exit cases remain in the same test.

This is a typed-report consumer join. It does **not** generate a native cancellation,
observe or mutate a real Runner residue flag, prove the Linux producer/adapter
join, or establish future admission from cached capability. No new seam/framework,
production source change, fixture process, ignored fixture, helper, network,
workflow dispatch, owner account/VM/admin action or external request was used.

## Verification

- `cargo test --locked --offline -p arxd --lib mcp::activity_tests::completed_command_sdk_exact_lookup_preserves_outcome_and_identity -- --exact`:
  passed, 1 test (three report cases), 0 failed, 408 filtered out; 0.15s test
  runtime after 1m00s compilation. Existing neighboring/native tests were not run.
- `cargo fmt --all -- --check`: passed after correcting the new tuple formatting
  reported by the first fmt check.
- `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings`:
  passed (28.92s).
- `git diff --check`: passed. Both changed files decoded as UTF-8 and contained
  LF newlines only with final newlines.

The tests pass on the existing production implementation. No failing production
reproduction, mutation test or production fix is claimed.

Fresh Muse independent review remains required with the coordinator; no reviewer
or other subagent was started. No push or PR. PROGRESS is intentionally outside
this slice; provider/effort/outcome are recorded here for coordinator incorporation.
A12 native, M2, full F/G and PA6 remain open and **unverified on real hardware**.
