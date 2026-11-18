# PR176 Ubuntu founder refusal: independent disposition

OpenAI / Sol 6.1 / high; 2026-11-18; diagnosis/design only; no children.
Assigned HEAD `510a9915f0fa8f70648267776f4b48c596adec76`; clean at admission.
Exact failed PR source `f1bf99bcbb3e18656879d6ce5cbf24e367c6056a`:
HEAD differs only in coordinator PROGRESS documentation. No source changed here.
Read AGENTS/KICKOFF, dytschgo-debug/profile, query brief/evidence/integration
review, founder disposition20870818, F6 result25aad48a, PR177 observation plan
and hosted/guard-correction reviews; relevant ADR0005/0024 constraints retained.
Debug phase1 remains open: no deterministic hosted-symptom loop established.
Phases2–6 are not reached; no ranked causes, fix or stability claim follows.

## Pinned observations and limits

- Supplied run CI44213815251/job134498506249, Ubuntu; completed raw job log
  `D:/Temp/arx-pr176-f1bf-hosted/ubuntu.log`, 101234 bytes, independently hashed
  SHA256 `040328dacf0d441ac1229c380ee091d0b456f9f4ff9d25c1ebddfa7650786f5e`.
  Log verdict: exit100; 33/637 run, 30 PASS, 3 FAIL, 22 skipped,
  604 not run after failure; nextest summary25.918s. No retry was performed here.
- Separately green parent main `05b9c82930cd3812614fc20f5d03d0a5fa05952a`,
  first CI44212212325: cached main-first/main-success/measurement JSON inspected
  under `D:/Temp/arx-pr177-approved-merge/`; 608s wall, 605s charged,
  Windows604s; founder13 PASS per OS. Cached Ubuntu log reports existing
  anchored proof0.178s, F10 drop0.156s and F6 0.112s. This is historical evidence,
  not a live status refresh or new execution. PR176 red does not make main red.
- Five query paths equal reviewed `f9d0a413d02737214af62601aa345c90634a4e26`
  by actual git comparison. Relative to parent, production changes include
  query read/ownership, pure eligibility, module binding and issue pool
  `Arc<Semaphore>` type/initializer around unchanged capacity4, plus new tests
  and inventory. Existing found borrows through Arc deref. Equal reviewed bytes
  do not establish causal irrelevance; no changed issuance budget was found.
- Prior disposition20870818 ran one local sequential F6/F7/F8 control:
  3 PASS/0.405s test time; hosted failure unreproduced. Result25aad48a's one
  instrumented F6 passed with14 events, all six phase exits Ok, final35.415ms,
  remaining4964.585ms; restored source. These are reused reports, not reruns.

| Exact failing unwrap | Whole-test duration | Established call position |
| --- | --- | --- |
| anchor_query_tests.rs:521:14 | 10.058s | initial found; before anchor query, TLS or proof |
| anchored_join_tests.rs:67:14 | 15.348s | existing initial found; before anchored proof |
| found_tests.rs:552:32, f10_drop_bounded | 15.303s | final found after gate release, read release and uninitialized assertions |

All three print `called Result::unwrap() on an Err value: Unavailable`.
F10's earlier eight dropped calls were blocked in preflight, then Busy was
checked; no successful initialization preceded the failing final call.
It is the first initialization opportunity, not evidence initialize was reached.
New durable owned_query_timeout_shutdown_retains_submitted_resources PASS24.814s;
retained local natural26-case phase6.667s included that case6.664s. Different
hosts, source ancestry and concurrency prevent treating these as a controlled
comparison. Broad elapsed-time inflation is observed; CPU/storage cause is not.

## What phases and clocks can actually tell us

PR177's current cfg(test) observer samples preflight, initialize, leaderwait,
proposal, guard1, guard2 and finaldeadline, but CAPTURE is installed ONLY for
F6's first issuance. None of these three calls is scoped. Their phase entries,
exits, remaining budget and pending expression are unobservable in this log.
F6 was not reached before fail-fast; absence of DEBUG-F6-OBS supplies no phase.
Even with capture: missing raft, CSR/CA checks, ticket mapping, signer/clock
gaps have no distinct phase; completed mapped Unavailable does not reveal its
underlying storage/Raft reason. Proposal's inner ? may skip its exit. Pending
means caller expression, never worker completion. Post-predicate timer samples
cannot reconstruct the exact earlier predicate instant. Missing/overflow trace
is invalid, not a reason to rerun. Current production macros use reviewed local
result bindings; historical literal-token-identity claims do not apply.

Nextest durations are runner elapsed measurements of entire tests, not found
phase spans. Log UTC emission order is separate; no cross-process monotonic
epoch or exact found entry times are recorded. At01:13:33.322739Z first FAIL,
then01:13:33.324153Z cancellation announcement (3 still running); later FAILs
at01:13:42.168241Z/42.169164Z and durable PASS at01:13:48.667355Z. These overlap
in the runner and finish about8.84s/15.34s after announcement; output buffering
and process teardown forbid interpreting these gaps as refusal timestamps.

Both engine proof bodies have35s work/40s cleanup deadlines; F10 has20s wrapper.
Their printed failures are inner found unwraps, not outer Elapsed unwraps;
runner durations10.058/15.348/15.303s are below those body budgets. found itself
has5s absolute ISSUE_BUDGET, async timeout plus final elapsed check. Cooperative
polling/synchronous work can overrun5s: a whole-test duration cannot localize it.
The job12min cap did not fire. Workflow invokes ordinary nextest without custom
termination flags; no tracked default nextest config was found. First failure
precedes announced cancellation, so that announcement cannot explain it. Later
calls could have been pending across it, but ordinary Rust panic/error output
and one later PASS show they were not reported as killed/timeouts. No inspected
evidence shows cancellation injecting FoundError; exact runner signal timing
and scheduler effects remain unmeasured. Do not infer cancellation caused—or
could have no indirect effect on—the later refusals from this log alone.

## ONE proposed bounded experiment; NOT authorized or executed

Purpose: establish a red-capable actual-found refusal/phase oracle, rather than
another green-only natural run. Two cells total, same pinned PR176 source,
same isolated cached toolchain1.95.0/nextest0.9.151, fresh private storage each,
one exact F10 invocation per cell sequentially, retries0, no other cases.
Future execution must be separately reviewed/granted on dedicated Ubuntu;
this plan grants no local Windows engine run, hosted rerun or network action.
It induces a specific preflight refusal; it cannot reproduce unknown hosted
phase attribution or establish a reproduction rate for the natural incident.

Minimal diff seam to prepare for review BEFORE editing source: only
`crates/arxd/src/cluster/consensus/found_tests.rs`, inside f10_drop_bounded,
after existing released(&n)/uninitialized(&n), around its final issue expression.
Reuse existing Gate::new(&n) (actual store mutex on its joined private thread)
and parent observation::capture inline; child module already has access.
No production hook, observer visibility change, global switch or storage edit.
Preserve all earlier F10 work, assertions,20s wrapper, four runtime workers,
5s found deadline, guards, clocks, permit admission and nextest settings.
Two reviewed patch variants differ ONLY in gate release position, no env knob:
- C: acquire gate, immediately drop/join it, then capture the final issue.
- T: acquire gate, capture the same final issue while gate remains held; drop
  and join gate immediately after found returns. No injected error or fake clock.
  This is real mutex backpressure in the non-abortable preflight store closure.
Save Result before its original success unwrap. In BOTH cells release/join gate,
await existing released(&n), require issue4/read8, and complete existing shutdown
before propagating saved Result with unwrap. Save shutdown result too, propagate
it after the issuance oracle so cleanup failure cannot mask the primary signal.
This test-only cleanup reorder is part of the exact diff requiring review.

Preregistered oracle: C exit0, final Ok,14 events/no overflow. T exit100 with
the SAME saved-result unwrap `Err ... Unavailable` (line necessarily moves),
preflight enter without exit, finaldeadline exit Unavailable/pending=preflight,
elapsed>=5000000us and remaining0 at final observation; no later phase enter.
Gate release/drain/capacity/shutdown must complete before that deliberate red.
Timing seams: capture existing issuance-relative samples; add only bounded
scalar gate-acquired/released, drain/shutdown-completed markers outside found.
Record whole-command and test elapsed separately. Never emit paths/data/keys.
Expected T red validates the refusal/observer loop under this treatment only;
no natural host causality, phase generalization or source fix follows.

Admission/stop: pin exact patch/source/config hashes; offline private checked
non-reparse cache/target/evidence; no competing build/engine. One compilation
per variant, one invocation: exact F10 filter, threads1, retries0, fail-fast,
success/failure output final. C must satisfy its oracle before T is admitted.
Any compile/admission failure, unexpected red, missing/overflow trace, outer
timeout, cleanup failure or assertion mismatch stops immediately. No mechanical
repair, rerun, tuning, extra cell or follow-on injection; T expected red ends it.
Preserve exit codes, sanitized traces and hashes. Reverse only reviewed patch
after verifying patched hashes; verify all crate bytes against pinned source.
Drift/survivors stop for coordinator disposition; no cache deletion or unrelated
process killing. No post-restore run. Exact patch review is still outstanding.

## Coordinator disposition proposal

PROGRESS unchanged: root may record this static disposition and pending loop
design only. Sol shared3/3 exhausted, historical Opus0/2 unchanged; no fix credit.
PR176 held/red; separately observed main green; all milestones still pending,
M2 open/M3 unchecked; hardware **unverified on real hardware**. No Rust/Cargo,
engine/tests/probes, source/settings changes, network/public writes, VM/admin,
merge/protection or cache deletion occurred. Only this report was written.
