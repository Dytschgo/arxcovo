# M3 cluster suite: third failed fix, owner disposition

2026-11-16. Anthropic / Claude Opus 5.5 / high. Independent design disposition on `473d0e5` (branch `work/fix-three-node-nextest-contention`, contains PR151 head `fb64b3e`). Docs only: nothing in source, config, Cargo or workflows changes, nothing is rerun, and no fourth fix is proposed. **Status: blocked on an explicit owner decision.** The Sol coordinator's owner brief must match this document before the owner is asked.

## Evidence (read from repo docs, source, and one CI job snapshot)

| Run (hosted Windows) | Head | Three-node failure | Site |
| --- | --- | --- | --- |
| 43782024400 | PR149 `8eaed30` | U6 `Err(Orphaned)`, 20.7 s | `three_node.rs:78` shutdown |
| 43788629298 | main `01a42d5` | T2, U6 `leader deadline`; T3, T4 write `Elapsed` | `:100`, `:117` |
| 43794002309 | PR151 `fb64b3e` (group max 1) | T4 `linearizable_state` → `Err(NotLeader)`, 19.687 s | `:121:62` in `converge()` |

- PR151 run: Ubuntu and macOS passed. Windows ran 484 of 565 tests: 483 passed, 1 failed, 5 skipped, 81 not run. The job took 342 s and the nextest step 197 s, so it did not hit the job time limit.
- Suite fix attempts that failed or are unproven: PR147 append callback (merged; its signature has not recurred), `6eb5ad8` shared-hook serialization (unmerged), and the PR151 scheduling group. The third failed on hosted Windows. Renaming the signature or switching provider does not reset the count. KICKOFF's stop condition applies: after three different fixes by both providers, document the failure and move on.
- What passed: PR151's own config pins exactly the eight three-node tests (max one at a time). Locally, 68 of 68 consensus tests passed at `-j4`, and Opus's controlled shared-flush model removed the leader-deadline signature. Neither result shows the suite is stable on hosted Windows. The hosted run disproved that the scheduling group alone is sufficient.
- What the NotLeader can and cannot tell us: `converge()` reads the leader from metrics, then calls `linearizable_state()` exactly once. That call has a 1.5 s wait, and its `ForwardToLeader` result becomes `NotLeader`. `write()` retries `NotLeader`; `converge()` does not. Ordinary Raft leadership churn between those two steps can produce this error. **Its hosted cause is unknown.** Slow flushes delaying heartbeats, election timing, or a real product defect under slow storage cannot be told apart without hosted instrumentation. No fix hypothesis is offered.
- Coverage dependency (current main): all eight tests build a real three-node in-process cluster from the same `Cluster` fixture. They use real OpenRaft with SQLite stores (`synchronous=FULL`) and an in-memory router: no TLS, no transport, no real hosts. All of them use `leader()`, `write()` and `converge()`. T1 and the three R6 tests have no recorded failure, but they share the same exposure.

## Exact tests and platforms

Prefix `arxd cluster::consensus::three_node::`. **Windows-hosted failures:** `t2_leader_offline_reads_and_unknown_retry`, `t3_snapshot_catchup_then_consecutive_append`, `t4_registry_revocation_snapshot_and_prune`, `u6_cancelled_callers_keep_engine_permits_until_resolution`. **Same fixture, no recorded failure:** `t1_follower_offline_enrollment_and_reopen`, `r6_shutdown_report_cancelled_proposal`, `r6_shutdown_report_store_only`, `r6_shutdown_report_normal_and_reaped_panic`. Ubuntu and macOS have no recorded three-node failure. The one Ubuntu consensus unit failure (43767515928) is the PR147 signature and has not recurred. That record is an observation, not proof of stability.

## Assessment of the quarantine scopes

1. **Quarantine only T4 on Windows: not justified.** T2, T3 and U6 failed on the same platform with the same fixture and helpers. Narrowing the scope to the newest signature would leave main expected to go red again, and it would launder the stop rule.
2. **Quarantine the whole `three_node` module on Windows (all eight tests): justified as the narrowest honest scope.** It covers every observed failure plus the tests that share the fixture. Ubuntu and macOS keep running all eight.
3. **Quarantine all consensus tests, or call the entire consensus suite unsupported: over-broad.** The other 60 `cluster::consensus` tests (68 in the PR151 mapping, minus these eight), the enrollment pure-core tests (expired, consumed, bad-proof codes), and the durable state, vote-log and registry tests are portable and green, and they do not use the fixture. They stay required on every OS.

Separately, the **M3 acceptance itself was never credited and stays unsupported** under any option: it needs one node offline without lost writes on a real three-node cluster, plus join-code rejection over the real transport. A quarantine unblocks main; it grants nothing.

## Credit withdrawn or kept, under option 2

- **Withdrawn:** any reading of earlier green Windows runs (for example 43777637580) as evidence that the three-node suite is stable. Windows three-node behavior (failover, snapshot catch-up, revocation/prune, cancellation permits, shutdown reports) is **unverified on hosted Windows** for as long as the quarantine holds, and that includes the U6 seam where PR149 failed. ADR 0024 and the PR151 group are recorded as failed attempt 3, not as an accepted fix. The Windows cluster runtime is **unverified on real hardware**. This matters because the owner's dev host and M4 are Windows.
- **Kept:** all eight tests on Ubuntu and macOS, every portable consensus, enrollment and storage test on all three OS, and Windows compilation and Clippy of the quarantined tests.

## Alternatives for the owner

| Option | Main | Cost / credit | Re-enable |
| --- | --- | --- | --- |
| A. Stop (status quo) | Stays red; nothing can merge, PR149 and PR151 included | No coverage lost, but all work deadlocks: a repair merge needs green main, and any repair would be a fourth attempt | Only through a new owner decision |
| B. Owner-provided controlled CI environment (dedicated Windows runner on separate hardware, or a paid larger runner) for the eight tests | Stays red until the environment exists | Needs owner admin or payment plus a reviewed workflow change. A failure there would be strong evidence of a product defect | Once the environment exists, after a reviewed run |
| C. Narrow reviewed quarantine (option 2), with one merge exception | Can turn green | Windows three-node credit lost as listed above | See below |

**Recommendation: C now, with B as the optional re-enable venue.** Re-enabling takes one separate PR that removes the Windows gate for all eight tests. That PR must carry: hosted-Windows instrumented diagnosis evidence (not a tweak), an attempt budget the owner has approved, review by a different provider, its own green hosted Windows CI, and then a green exact-main run. Hard limits: M3 stays unchecked while the gate is active, and the owner revisits the quarantine before any M3 acceptance decision or Windows cluster use in M4, whichever comes first.

## Draft one-time authorization (NOT granted; valid only once the owner records it verbatim, dated, in PROGRESS)

> I authorize exactly one exception to the green-main merge prerequisite while main `01a42d5` (CI 43788629298) is red. It covers one new main-based PR whose only behavior change is a Windows-only quarantine of these eight tests: `arxd cluster::consensus::three_node::{t1_follower_offline_enrollment_and_reopen, t2_leader_offline_reads_and_unknown_retry, t3_snapshot_catchup_then_consecutive_append, t4_registry_revocation_snapshot_and_prune, u6_cancelled_callers_keep_engine_permits_until_resolution, r6_shutdown_report_cancelled_proposal, r6_shutdown_report_store_only, r6_shutdown_report_normal_and_reaped_panic}`. Conditions:
> (1) The PR changes only that gate, a guard test that pins exactly these eight IDs and the Windows-only condition, and docs. It changes no production Rust, assertion, deadline, timeout, retry, scheduling, Cargo or other workflow setting.
> (2) Each quarantined test shows a skip reason in test output that names this disposition. Ubuntu and macOS still run all eight.
> (3) All five required gates are green on the exact head, with independent review by a provider other than the builder. The coordinator merges it; it does not auto-merge.
> (4) The next main CI on the exact merge SHA must be green before any other merge. If it is red for any reason, merges stop and I am asked again: no rerun, no wider scope.
> (5) M3 stays unchecked, the Windows three-node cluster is recorded as unverified on hosted Windows and on real hardware, and the quarantine ends only through the reviewed re-enable PR described above.
> This does not cover PR149, PR151, the PR151 guard fix, a fourth consensus fix, or any other merge onto red main. It lapses if main moves from `01a42d5` before use, or if it is unused 7 days after I record it.

## Limits

No fourth fix, rerun or merge until the owner decides. M2 stays open (A7 FINAL NO-GO; A12 consumed, no retry); hardware paths are **unverified on real hardware**. This document does not grant itself.
