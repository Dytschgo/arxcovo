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

## PR150 parent integration

PR150 merged as01a42d5 with reviewed/squash tree equality after five green gates and green exact-main6e9edd5 CI43777637580. Merge34bf2d6 retains its main apply pause hook (blocking-thread gate before the store lock), adapts R1's release channel, captures actual read-held permits before abort alongside proposal-held permits, and caps mixed-owner report counts at72 (64 proposals plus8 reads). R5 now asserts Owners/Joined/read_held1/propose_held0/owners_left1 with held state store and no log holder. Main R6 tests are retained unchanged, including the exact external store count.

Fresh coordinator Windows cluster run at34bf2d6 passed117 tests,0 failed,2 owned crash-child ignored,416 filtered out,9.54s. R5 gated report: Owners/Joined/read_held1/propose_held0/owners_left1/sm_strong2/log_strong0, total5001ms; main R6 store report has exactlyone external state-store reference. Lint and independent integration review are still pending at this checkpoint. The natural hosted U6 cause remains unresolved; this integration prepares finite changed-diagnostic hosted attempt2 and is not a fix claim.

The same fresh main-integration run completed `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` successfully in27.10s. `cargo fmt --all --check` and `git diff --check` passed. Local integration validation is complete; independent Opus review and a new exact-head publication review remain before hosted acceptance.

## Integration review F1/F2

Opusf8e945b confirms production integration correctness but reproduces a local cargo-test race: R1 and R6 overwrite the single APPLY_PAUSE slot. Hosted nextest uses process isolation, but local cargo-test evidence must also be reliable. A test-only Tokio mutex now serializes complete R1/R6 hook lifetimes without holding a standard mutex across await. Every future shared-slot fixture must take the same ownership lock. Production source and adapter.rs remain unchanged.

The current integration is the PR150 main hook inside a blocking worker before the store lock (not the superseded A0 async hook). The report now has actual read_held snapshots, a72-unit mixed-owner ceiling, and R5 read attribution; these supersede the main-only diagnostic baseline's read_held0/64 ceiling for this A0 branch. PR150's original evidence remains a truthful historical main-only record. The117-test integration results above are the coordinator's fresh runs; Opus independently reported117 passing plus the focused race failures. Brief wording is corrected. Post-F1 verification is pending, with no green or hosted claim inferred yet.

Coordinator post-F1 verification at6eb5ad8: locked/offline focused `cluster::consensus::three_node::r -- --test-threads=4 --nocapture` passed8 tests in8.71s, including both shared-slot fixtures. Full cluster passed117 tests,0 failed,2 owned ignored in9.50s. Locked/offline workspace Clippy passed in3.58s, fmt/diff checks passed. No production change or hosted/natural-flake fix credit follows; independent recheck remains active before push.

Independent Opus recheck57f23e4 closes F1/F2 at6eb5ad8: the exact R1/R6 pair passes8/8 after formerly failing8/8; focused eight-test group passes3 default-thread runs and2 four-thread runs; full cluster117 passes in9.48s. Removing R6's ownership lock compiles and fails4/4; source restored clean. Independent arxd Clippy/fmt/diff pass; production correctness reuses the unchanged34bf2d6 review. The reviewer target contains a restored-source but stale mutant binary and must not be reused.

Publication is held despite these local results: latest exact-main01a42d5 CI43788629298 is red on T2/U6 election waits and T3/T4 write waits. A0 does not address those paths. No finite changed-diagnostic attempt2 push has occurred; the other-provider recurring-suite diagnosis is active. No further merge until exact-main CI is green. Local/shared-hook fixes do not resolve the natural hosted U6 failure.


## A0 resumption on sampler main — 2026-11-16

Builder: OpenAI GPT-6.1 Sol, high, implementation sub-agent; no children.
Authorization: `docs/briefs/m3-bounded-read-resumption.md` and coordinator
clarifications. Prerequisite supplied by the coordinator: exact-main
`a18e95c20b7340127d746fb3632f3b0eab32e586` CI43836099670 success, Windows 433 s,
sampler all eight pass in 29 s. This agent did not query or rerun hosted CI.

### History, scope and source identity

Prepared base `9cf98a4b829491b9e91fd3aa0858cb7d8fef3708` already contains sampler
main. Merge `9137ef3a90a80de14e73fb2c675ba422923e1bbf` has that first parent
and reviewed local A0 `8a8c1fd11fe783a44af21fc022303712684e2b92` second.
Both histories and old PR149 head `8eaed305b1a32b7097f7a46685904976f336fc84`
are ancestors. No source file was restored from the old branch: Git auto-merged
consensus.rs and the instrumented fixture; only the Cluster declaration conflict
was resolved to keep `pub(super)` and add the reviewed APPLY_PAUSE_OWNER static.
The other conflict takes prepared main's PROGRESS byte for byte. Root owns progress.

Ordered added/removed-line comparison of `git diff --unified=0 01a42d5 8a8c1fd
-- crates/…` against `git diff --unified=0 9cf98a4 9137ef3 -- crates/…` passed:
144 changed lines in consensus.rs and 309 in three_node.rs. Hunk positions and
unchanged diagnostic context are excluded; the old private Cluster remove/add
pair is excluded because main's public visibility is retained. Every A0 behavior
line matches. This proves the reviewed production delta without erasing main D1.
Retained reviews: c0bf702, 08f8832, df0d42e and 62bfd25.

C2 `d1736417219c9abf796536a4ffbd35612ade19b4` adds exhaustive test-only
`read_fail(&ReadError) -> Fail`, preserving the three mappings and adding
Busy => Fail::Busy; the new unit asserts all four variants. No wildcard.
C3 `7420f1dd275f636f5712fb26af73a8363edbf75b` adds one pure sampler fixture:
five non-ignored R1–R5 cases mismatch with reason `ignored` pass inventory;
each individually changed to `matches` must raise Guard("inventory").
No sampler production, selection, workflow, guard, ignore or test-group changes.
Wrapper/inventory integrity failures still gate CI; only test outcomes are
non-gating. Exactly eight Windows ignores remain.

Verified source head: `7420f1dd275f636f5712fb26af73a8363edbf75b`.
Git blob pins (evidence-only descendant leaves these unchanged):

- consensus.rs: `b7d1f350afb64409b82677ed04e2fe1315993c06`
- three_node.rs: `ddd35c4cf52eb40cf0a2c0c57ee989f7d0ef00a2`
- adapter.rs: `181e9dce6765b1db9339c254ec2dcead9b88a0b9`
- diagnostics.rs: `6139626427dbc4449a8923a3b2e3459e727e876f`
- windows_diag_sample.py: `6ba6f257152964cbde7722a3f6a633cdbb3fa056`
- quarantine guard: `bb376fccf99e786bfbe1c84e712cab892dd58d05`
- CI workflow: `0c7af38b33f6c127029d8312a0c9ecd92f6361da`
- PROGRESS: `69594c883c1db6c1786ad2624df468a10420b8f5`

Adapter, diagnostics, sampler production, guard, workflow and PROGRESS are
byte-identical to prepared main. Production has no changes after the merge.
The reviewed helper assertions, deadlines, routing, 64 proposal + 8 read permits,
1500 ms absolute caller budget and 5 s shutdown remain. No fourth cluster fix,
scheduling/timeout/retry change, quarantine extension or filter move.

### Single local verification round

Local Windows x86_64 MSVC, pinned Rust 1.95.0, cargo-nextest 0.9.151
(commit 6b3f994577a98874a2e0ea5bef77fbbab3434044). Unique run root:
`D:/Temp/arxcovo-a0-resume-7420f1d-d466393d`; CARGO_TARGET_DIR is its `target`,
TEMP and TMP its `tmp`; CARGO_NET_OFFLINE=true. All Cargo build/test/lint
commands below used `--locked --offline`. No install or cache cleanup.
Raw output is retained locally under that root's `logs/` (not committed).
No three-node red occurred; no test command was repeated and no fix followed testing.

| Command (additional nextest output flags: --success-output immediate --failure-output immediate) | Result / log |
| --- | --- |
| cargo nextest run -p arxd -E 'test(/cluster::/)' | PASS 119/119, 468 skipped, 8.157 s execution; initial clean build 1m14s; cluster-nextest.log |
| cargo test -p arxd --lib cluster:: -- --nocapture | PASS 119, 0 failed, 13 ignored, 417 filtered, 7.78 s; cluster-inprocess.log |
| cargo nextest run -p arxd --run-ignored only -E 'test(/cluster::consensus::diagnostics::controls::/)' | PASS 3/3, 5.496 s; d1-controls.log |
| cargo nextest run -p arxd --lib --run-ignored only -E 'test(/^cluster::consensus::three_node::/)' | PASS exact eight, 7.840 s, local sanity only; quarantined-eight.log |
| python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py | PASS 1, exactly eight; guard.log |
| python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py | PASS 58, including C3; sampler.log |
| node --test scripts/ci-workflow.test.mjs | PASS 4/4; node-workflow.log |
| cargo clippy --workspace --all-targets -- -D warnings | PASS, 28.20 s; clippy.log |
| cargo fmt --all --check | PASS; fmt.log |
| git diff --check 9cf98a4 HEAD | PASS on verified source head; diff.log |

The ordinary in-process run exercises shared REGISTRY and R1's APPLY_PAUSE_OWNER
in one libtest process; Windows-ignored R6c remains ignored there. R6c runs once
in its own nextest process in the eight-test sanity run, so this round does not
claim a fresh same-process R1/R6c contention test. The retained 62bfd25 review's
pair/mutation evidence covers the unchanged lock, without repeating tests here.

Actual inventory command (with the same offline/locked flags):
`cargo nextest list -p arxd --lib --message-format json --run-ignored only -E
'package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/)'`.
Saved nextest-list.json proves all five non-ignored R1–R5 cases have exactly
`{"status":"mismatch","reason":"ignored"}` on 0.9.151. Only the eight guard IDs
match. Calling unchanged check_inventory on those bytes passes, using the unique
run root whose target directory is the actual CARGO_TARGET_DIR. C2's non-ignored
unit is also excluded. Logs: inventory-confirmation.log and source-identity.txt.
The hosted sampler wrapper itself was not run locally; no hosted admission,
Windows job-budget or wrapper Job Object execution claim is made.

R1 caller 1.509327 s; R2 Unavailable at 1.5091323 s, units returned by
1.5095165 s; R3 caller 1.5135221 s, 150 heartbeat ticks. R4 and C2's all-four-error
unit pass in both ordinary suites. R5 clean shutdown passes; gated shutdown is
expected Orphaned at 5.0087458 s in nextest (5.0038663 s in-process), with
Owners/Joined/read_held=1/propose_held=0/owners_left=1/log_strong=0/sm_strong=2.
No observer panic occurs in R5, the controls or the eight. The ordinary suites do
print "diagnostic observer panicked" solely from the passing, intentional
`completed_observer_panic_is_joined_and_reported` should_panic unit; that is not
an R5 fixture failure or observer leak.

All three positive controls PASS while confirming induced attribution:
S3 Err(NotLeader), Helper::Converge and Churn; S4 queued=40/backlog=40,
apply_high=20, Candidate::Store and expected Owners/Orphaned at 5.0093002 s;
timer burn=400 ms gives lag=399 ms, store_high_delta=0, Candidate::Cpu.
These induced control results and all R1/R3/R4/R5 store/apply-gate diagnostics
carry **no natural-latency credit**. Do not pool their pass lines with sampler
evidence. The local eight are sanity only and confer no hosted recovery,
stability or re-enable credit. Their R6 fixtures also deliberately hold resources;
no aggregate natural-store bound is claimed from this round.

### Remaining gates and risks

Fresh Opus delta review of the merge/C2/C3/evidence, a separate exact-head review,
and all five hosted gates remain required before coordinator-only publication
and merge. R1–R5 must execute and pass on all three OS; exact-eight guard and
sampler inventory must pass; Windows must meet the unchanged budget. Any red is
handled by the resumption brief, without rerun, new ignore or deadline change.
The future A0 merge is a sampler epoch boundary because convergence now uses
fresh_read and can return Busy/Unavailable; never pool samples across it.
No claim that the natural Windows seam is fixed. Advisory 1 (Weak expiry does not
prove file closure) remains open. M2 stays open, A7 FINAL NO-GO and A12 no retry;
M3 remains unchecked. Windows three-node remains **unverified on hosted Windows**
for this head and **unverified on real hardware**; real transport/hosts and all
hardware paths are **unverified on real hardware**. No native M2, VM, admin,
external dispatch/rerun, status, push, PR, external merge or watch action occurred.

## Resumption review and publication checkpoint

Opus `f57d8cf` retained `360b9a8` passes the integration. Reviewer independently ran119cluster nextest,58sampler and exact8 guard; actual list confirms five R fixtures plus C2 unit mismatch as ignored under sample selection. Other builder checks remain attributed to the builder. The review recomputed the changed-line proof without excluding a Cluster pair: no exclusion was required, so the earlier evidence describes a weaker comparison than the actual proof. No source correction follows the review.

Coordinator re-read exact live main `a18e95c20b7340127d746fb3632f3b0eab32e586` and CI43836099670 success before publication. PR149 is updated by fast-forward, preserving old8eaed30 ancestry and all reviewed histories. New hosted gates and separate exact-head review remain pending. Required merge evidence includes R1-R5 execution/pass on each OS, exactly8 quarantine/sampler IDs and Windows job within budget. This is a new source head, not a rerun of the historical failed head; the original U6 failure remains unresolved and quarantined.

## Hosted resumption and merge

PR149 exact head `4ca2b961893ffd09bf3a8dee6399a6fb885eca26` passed all five required gates on run43839536472. Opus exact-head PASS comment6912554481 and success status59010377793 target that full head. Coordinator independently read the three job logs and confirmed R1-R5 each have PASS entries on Ubuntu (job133204473353), Windows (133204473316) and macOS (133204473025). Local parser initially missed ANSI-decorated names; the corrected extraction uses PASS lines containing the three_node module and exact R names. No test was rerun.

Windows job took543s (09:25:09–09:34:12Z). Sample reported all_pass/nextest0, wall29281ms, list906ms, run28344ms, cleanup0ms, orphans0, E520s. Exactly T1/T2/T3/T4/U6/R6c/R6s/R6p passed; the five required R tests were not sampled. Logs retained locally at `D:/Temp/arx-pr149-{ubuntu,windows,macos}.log`. Neither sampled green nor required-R green proves Windows stability or lifts the quarantine.

Immediately before merge, main `a18e95c` was unchanged and exact-main CI43836099670 green. PR149 merged as `f1c399d253c10bde94f3d2223db898e79d13c551` at2026-11-16T09:37:02Z. Reviewed and squash trees equal `9c72d1d70c850abc81ee6ee7602466a97152c6dc`. New exact-main CI43841341548 is pending; no further feature merge before it is green. Certificate implementation also waits for its readiness contract review. M2 open/M3 unchecked and hardware paths remain unverified.
