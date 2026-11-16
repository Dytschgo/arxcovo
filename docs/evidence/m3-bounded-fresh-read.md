# M3 A0 bounded fresh read — implementation handback

Base: `6dab2e4d8ba19134bfd9cf16d401d84320a0e206`.
Builder: GPT-6.1 Sol, high effort, 2026-11-16. Scope: A0 only.
Status: **blocked on an internally conflicting refusal contract; not ready to publish**.
No A/B, dependency, PKI, issuance, PROGRESS, ADR or workflow changes.
Fresh independent Opus review is required before publication; none was requested
from this implementation child (no child delegation authorized).

## Implemented ownership

One 1500 ms absolute caller deadline spans admission through store wait. Eight
separate read permits use immediate admission, with tracked owners in the existing
JoinSet. The owner awaits the nonwithdrawable OpenRaft request to its actual reply;
apply readiness uses the remaining deadline. The blocking closure owns its store
Arc and permit, and releases guard, Arc, then permit before reporting completion.
Caller cancellation or timeout cannot release a still-running read unit. Expired
results are not delivered. The existing 64 proposal permits, proposal cancellation,
append callback and shutdown engine-before-owner ordering are unchanged.

Shutdown remains a five-second responsiveness bound. It reports Orphaned when
blocking work survives, rather than promising hard termination or silently
claiming cleanup. The tests explicitly release the store gate and wait for weak
store references and permits to clear afterward.

## Blocking source/behavior conflict

Decision 1 requires `wait_timeout(remaining)` for the engine and `Unavailable` on
expiration of the absolute caller deadline. R2 and unchanged T2 require only
`NotLeader | NoQuorum` on an isolated leader. These cannot both be guaranteed with
the pinned OpenRaft API:

- Cached `openraft-0.10.0/src/core/raft_core.rs:408` takes `now` at dequeue;
  lines 446–461 compute `wait_timeout`, then `deadline = now + wait_timeout`.
- `src/core/linearizable_read/pending_read_queue.rs:51–66` expires pending reads
  only once that engine deadline is reached.
- Here remaining is computed before submission. Consequently the engine timeout
  is at or after the caller's deadline (later by any queue delay). An isolated
  leader need not step down, so `NotLeader` need not arrive earlier either.
- `fresh_read` must close the receiver at its caller deadline and reject late
  delivery. Thus it returns `Unavailable` before the engine's `NoQuorum` can be
  observed. R2 and T2 fail. Compilation confirms the specified API exists;
  this is an observable contract conflict, not an API-name mismatch.

No shortened quorum sub-budget, timeout relabeling, retry, timeout increase or
assertion weakening was substituted. Coordinator/design input is needed to settle
which refusal contract changes. The current code and strict tests are retained
for review, not represented as completed acceptance.

## Controlled counterfactuals

Actual OpenRaft nodes, in-memory RPC transport, durable stores; outer test budgets
remain 45 s. Store locking uses a dedicated std::thread and release channel.

- Before changing the read implementation, R1 failed at its old four-second hang
  detector (test time 4.27 s). Leader apply was paused before acquiring the store
  mutex; followers applied the write before the read began. After implementation,
  R1 returned Unavailable in 1.515 s and convergence succeeded after release.
- Temporarily restoring the original `linearizable_state` implementation made R3
  fail at its four-second detector (test time 4.17 s, build+run 28.922 s).
- Temporarily returning the permit before spawning the blocking closure made R3
  fail deterministically: 8 available permits instead of 7 while still gated
  (test time 1.70 s, build+run 26.484 s).
- Both mutants were restored in `finally`; neither is committed. A first mutant
  harness invocation failed compilation because of mixed CRLF handling; it did
  not count as a behavioral red. The corrected runs above compiled and failed
  the intended assertions.

## Verification environment and results

Windows; pinned Rust 1.95.0; offline locked dependencies; unique target and temp
under `D:/arxcovo-a0-02376721/`. No installs, cache deletion, account changes,
grants, owner admin, old Lab Manager, VM work, native M2 rerun, push or PR actions.
Raw local logs remain in that directory.

Initial A0 engine run: R1/R3/R4/R5 pass, R2 fails. R3 caller 1.512 s with 150
10 ms heartbeat ticks, permit retained until release, then current reads succeed.
R4 fills all eight units, aborts a caller, checks fast Busy and unchanged proposal
capacity, then drains and reaps. A test-only per-node counter observes actual
blocking-closure entry before R4 cancellation. R5 clean shutdown 1.218 ms;
gated Orphaned shutdown 5.005 s, followed by release and cleanup.

Workspace nextest command:
`cargo nextest run --workspace --locked --offline --no-fail-fast`.
First run: 567 run, 516 passed, 51 failed, 5 skipped; execution 14.942 s,
build+execution 43.277 s. R2 and T2 are the A0 conflict. The other failures are
outside cluster: 17 owner_windows fixtures and 32 inventory fixtures. Baseline
comparison and final checks are recorded below.

M2 remains incomplete: A7 FINAL NO-GO and A12 no-retry remain unchanged. M3
acceptance remains open. Real-host, VM and hardware paths are **unverified on real
hardware**. Store-file advisory 1 (Weak expiry does not prove file closure) remains
open; this slice adds no production restart caller.

Final checks on restored source:

- `cargo fmt --all --check`: PASS.
- `git diff --check`: PASS.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  PASS, 9.909 s including a build-lock wait. The first clippy run identified a
  test-helper guard scope; it was fixed with a lexical block, without suppression.
- `cargo test -p arxd --locked --offline cluster:: -- --nocapture`: 112 passed,
  2 failed (R2/T2), 2 ignored crash-child entrypoints invoked by passing parent
  tests. Test execution 8.00 s, build+execution 30.210 s. T1/T3/T4/U6 and both
  four-process-kill parent tests pass; existing test bodies are unchanged.
- Final R1 caller: 1.502 s. R2: Unavailable at 1.515074 s, all units returned by
  1.515348 s. R3: 1.514 s and 150 heartbeat ticks. R4 passes with closure-entry
  synchronization before abort. R5 clean: 2.845 ms; gated Orphaned: 5.014763 s;
  release/weak-reference cleanup assertions pass.
- Exact-base workspace comparison: 562 run, 513 passed, 49 failed, 5 skipped;
  execution 13.949 s, build+execution 42.329 s. Failure-name set comparison proves
  the 49 non-cluster failures are identical to the A0 workspace run. Baseline T2
  passes. Inventory fixture failures report Windows error 5; no permissions or
  unrelated fixture code were changed. Baseline source was restored afterward.

No corrective policy attempt was made against the refusal conflict: the handoff
threshold of three genuinely different failed fixes was not exhausted. This is a
source-backed contract blocker, retained explicitly rather than patched around.
Hosted CI and fresh independent implementation review have not run.

## Coordinator refusal-contract correction and fresh verification

Opus review55e56a6 (retainedc0bf702) confirms the pinned engine's dequeue-based timeout cannot guarantee a NoQuorum reply before the absolute caller deadline. Sol independently accepts the narrow policy and records it in the brief review. Test-only correction8237db1 accepts deadline Unavailable in R2 only when elapsed >= READ_BUDGET, keeps caller <=2.5s, and bounds permit return by the same original call-start +2.5s deadline. T2 adds Unavailable only to its isolated-read non-success pattern. No production code, timeout duration or error mapping changed; no quorum sub-budget or relabelling was introduced. The earlier112-pass/2-fail result remains historical handback evidence, superseded by this corrected-contract run.

Fresh coordinator Windows checks on8237db1 used unique D:/Temp/arxcovo-a0-correction-root-target, with TEMP/TMP on D:. `cargo test -p arxd --lib --locked --offline cluster:: -- --nocapture` passed114 tests,0 failed,2 ignored owned crash-child entrypoints,416 filtered out,9.33s. R2/T2 and all other cluster tests pass; R5 gated shutdown returned Orphaned in5.0027259s, as required rather than claiming hard termination. `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` passed in34.17s; fmt check and diff check passed. No unrelated workspace failures are counted green; the builder's exact-base49-failure comparison remains unchanged. Independent delta recheck and fresh hosted gates remain pending. Hardware paths remain **unverified on real hardware**.
