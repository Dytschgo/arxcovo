# M2 cancelled activity join — Muse independent review

Scope: base `1958f99`, head `b811ae6` (`test/m2-cancel-residue-join`),
single commit, 2-file delta only:

- `crates/arxd/src/mcp/activity_tests.rs` (extended existing SDK test)
- `docs/evidence/m2-normal-cancel-residue-join.md` (new evidence note)

Verified with `git show --stat 1958f99...b811ae6` and
`git diff --name-only 1958f99...b811ae6`: no production source change,
no other files. Landed parent is exactly `1958f99`, so there is no
overlap with watched docs-only PR116. (The evidence header cites a
stale worktree "prepared head" `9047d80`; the landed commit is what was
reviewed. Harmless — the landed delta contains no PR116 docs.)

## Standards

- One concern, conventional test commit, test-only: production code is
  untouched (name-only diff confirms). No new seam, framework, helper,
  fixture process, or network surface; the change reuses `good`,
  `completed`, `OnceReport`, `default_fixture` inside the existing test.
- No assertion weakening: the two baseline cases
  (`Exited`/`Some(0)`/`succeeded`, `Exited`/`Some(7)`/`failed`) are kept
  verbatim in parametrized form (`stop_reason` now asserted per case).
  The new case only adds assertions: null `reason`, null `exit_code`
  (`json!(exit_code)` with `Option`), non-`quarantined` capability,
  `stop_reason: "cancelled"`, `cleanup_confirmed: true`, and two extra
  persisted columns (`stop_reason`, `cleanup_confirmed`) in the
  separate-connection SQLite check.
- Smell baseline: no mysterious names (the `(stop, exit_code, outcome,
  stop_reason)` tuple is explicit); no duplicated logic (one loop, no
  copied test); no speculative generality (no new helpers or params
  beyond the case the spec names); no shotgun/divergent edits (one test
  fn, one evidence doc).
- Standards findings: none. One non-blocking note: the evidence header's
  prepared-head hash does not equal the landed commit (see Scope).

## Spec

Requirement (from the evidence note and the pre-existing SDK test it
extends): cover the missing join — post-spawn **Cancelled + confirmed
cleanup + no exit code** through the SDK `run_command` response and the
reopened exact activity lookup — while retaining the normal exit cases.

- Production path traced, not executed: `OnceReport::run` returns the
  typed `Report::Completed` → `Execution::dispatch` →
  `Execution::complete` → `assess` (`crates/arxd/src/service/executor.rs`)
  → `Activity::finish_typed` → `run_command` response; exact lookup via
  `Activity::list_filtered` (`crates/arxd/src/mcp.rs`). For
  `stop: Cancelled`, `exit_code: None`, `cleanup_confirmed: Some(true)`
  with the `good(Windows, Standard)` identity/point/boundary,
  `assess` yields `reason: None`, `Outcome::Failed`, quarantine false
  (stop-valid requires `exit_code.is_none()` for `Cancelled`; outcome is
  `Succeeded` only for `Exited`/`Some(0)`). Every test assertion matches
  this path: `outcome == "failed"`, null `reason`/`exit_code`,
  `stop_reason == "cancelled"`, `cleanup_confirmed == true`, no
  quarantine, identity/rights/program/argc, committed terminal row seen
  over a separate SQLite connection before lookup, then reopened SDK
  lookup checking every returned metadata/identity field, timestamps,
  and `other-agent` principal isolation (empty page).
- Non-duplication verified: the cited prior coverage exists under the
  named symbols (`residue_tests`: `independent_owned_domains…`,
  `cleanup_source_pins…`; `e6.rs`: `t11_t12_t13_t13b_reports_and_latches`
  — before-spawn cancellation only; `unix_bridge/tests.rs`:
  `t7_dropped_request_cancels_running_fake_and_joins` — fake port,
  residue always false; `executor/tests.rs`:
  `adr0017_dispatch_durable_matrix` — durable matrix but not SDK exact
  lookup after reopen). The gap (SDK exact lookup after reopen for the
  cancelled/confirmed/null-exit case) is real and is exactly what the
  new case fills. No A12 native producer/residue overclaim: the evidence
  note explicitly disclaims native cancellation, Runner residue
  mutation, and the Linux producer/adapter join.
- Normal cases retained: baseline (`1958f99`) looped `(0, succeeded)`,
  `(7, failed)`; the new loop keeps both plus the cancelled case.
- Spec findings: none. Behavior asked for is implemented; nothing in the
  diff was unasked for.

## Security

- Principal isolation is exercised for the new case too (reopened store,
  both `fixture-agent` and `other-agent` sessions; foreign principal gets
  `{"activities":[],"next_before_id":null}`). MCP argument validation,
  bounded reads, and tampered-record handling live in the untouched
  sibling test and untouched `mcp.rs` paths.
- No secrets, owner data, or new externally reachable surface: fixture
  SIDs/pins only; no path, network, account, VM, or admin action.
- The new case asserts the non-quarantined branch
  (`capability()["reason"] != "quarantined"` plus `cleanup_confirmed`
  true); the unconfirmed-cleanup/quarantine branch remains covered by
  `report_boundary_case` in `executor/tests.rs`. No security posture
  weakened.
- Security findings: none.

## Evidence vs rerun (this review)

Call-graph audit first: the test drives only typed fixture reports
through the in-process dispatcher over an in-memory duplex with a
private temp store; no native/process/helper/ignored/LinuxFs/network
execution. Safe to rerun the focused checks, and only those.

- Focused test
  `cargo test --locked --offline -p arxd --lib mcp::activity_tests::completed_command_sdk_exact_lookup_preserves_outcome_and_identity -- --exact`:
  evidence reports 1 passed / 408 filtered. Reran: **1 passed,
  0 failed, 408 filtered, 0.19s** (54.88s compile). Corroborated.
- `cargo fmt --all -- --check`: evidence reports pass. Reran: **pass**.
  Corroborated.
- `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings`:
  evidence reports pass (28.92s). Reran: **pass** (29.31s). Corroborated.
- `git diff --check`: **pass**. Both changed files are LF-only with a
  final newline (byte-checked). Corroborated.
- Neighboring/native tests were not run here — same stated boundary as
  the evidence note.

## Open items (unchanged, not blocking)

A12 native, M2 full, F/G, and PA6 remain open and
**unverified on real hardware**; this review executed no hardware paths.
No reviewer/subagent was started by the builder; this file is the fresh
Muse independent review for coordinator incorporation. No push or PR
from this review; PROGRESS intentionally untouched.

Close: Standards 0 findings (1 stale-header note); Spec 0 findings;
Security 0 findings. Worst item overall is the non-blocking evidence
header staleness. Eligible for coordinator merge once the required gates
(green CI, agent-review posting, coordinator-only merge) pass.
