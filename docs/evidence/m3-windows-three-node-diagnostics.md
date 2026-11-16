# M3 Windows three-node: D1 diagnostics evidence

2026-11-16. Anthropic / Claude Opus 5.5, implementation sub-agent under coordinator dispatch. Base `6b00d17` (main `6f6e9d2`, CI 43809711957 green). Authority: Opus plan `d3e2a02`, retained as `1bdad09`, and the mandatory Sol contract `docs/reviews/m3-windows-recovery-plan-sol-review.md`. **D1 is test-only instrumentation, not a fix attempt.** It contains no source fix, no re-enable, no workflow or CI change, no PROGRESS/ADR/Cargo change, and no change to the quarantine or its guard. This is not a fourth Sol attempt.

## Commits and scope

| Commit | Content |
| --- | --- |
| `6b70689` test(consensus): record bounded three-node diagnostics | New `consensus/diagnostics.rs` (infrastructure and 8 unit tests); `consensus.rs` adds `#[cfg(test)] mod diagnostics`; `adapter.rs` adds `#[cfg(test)]` latency spans and queue/delay hooks; `three_node.rs` wires the fixture, helper attribution and the drop dump |
| `01a2e0d` test(consensus): add opt-in D1 positive controls | Three `#[ignore]` controls in `diagnostics.rs` (opt-in for every platform) |
| this commit | This evidence file |

Every line added to `consensus.rs` and `adapter.rs` is a `#[cfg(test)]` item or statement. No non-test line changed (checked with `git diff -U0`). In `three_node.rs`, all eight `cfg_attr(target_os = "windows", ignore = ...)` gates keep the same test IDs and reasons. The file still contains the token `ignore` exactly eight times, and `tools/ci/test_windows_cluster_quarantine.py` passes unchanged. Helper timeouts (5 s), retries, panic messages and outcomes are unchanged. The helpers now also count polls and retries, and record an attribution before the panic they already raised.

## What D1 records (bounded; integers and enums only)

- **Metrics rings.** Each node has one owned watcher on OpenRaft `metrics()` (event-driven, no polling). Its ring holds the last 128 samples of `(ms since fixture start, term, state, leader, last_log_index, local_committed, last_applied, age of last quorum ack)`. Run-wide counters cover samples seen, overwritten samples, and term, leader and state changes. **Watch history coalesces updates.** A transition missing from a ring proves nothing.
- **Store-op latency.** Measured at the adapter boundary, from submit to completion, so the figure includes blocking-pool queueing and store-mutex wait. It is bucketed <10 / <50 / <200 / <1000 / ≥1000 ms, plus the maximum, for vote save, append (including its durable flush), apply, snapshot build or read, and other operations (log read, truncate, purge, install). Each node also records how long blocking work waits in the queue before it starts.
- **Timer lag.** One owned 50 ms interval probe per fixture (`MissedTickBehavior::Delay`). It records the worst lag and when it happened, the count of ticks ≥200 ms late, and a 32-entry ring of lag events ≥100 ms.
- **Helper attribution.** Counts calls and polls for `leader()`, calls and `NotLeader` retries for `write()`, `converge()` calls and stops. The first failing helper is recorded with its kind, node, polls or retries, a fresh sample from every node, the `ShutdownReport` for `stop()`, and, for the converge read, before and after samples with an S3 class.
- **S3 class.** `Churn` means one of: the before or after sample is not a same-term Leader with the same leader; the reading node's ring left that term or the Leader state after `before`; or any other node's ring shows a higher term after `before`. `SameTermCandidate` means only that the bounded, coalesced history retained no churn. It is a candidate, not a verdict.
- **Candidate.** Timer lag or queue wait ≥200 ms points to CPU. An adapter op ≥200 ms points to the store. The result is `None`, `Cpu`, `Store` or `Both`, and both can share one upstream cause.
- **Output.** On failure, the dump is the `[DIAG-3n]` lines, hard-capped at 8192 bytes with a `trunc=` marker. Rings are written newest first, sharing the remaining budget. The dump fires when the fixture drops while panicking, or before `finish()` ran (which covers the outer 45 s timeout). Drop aborts the owned tasks without awaiting them, reads locks with `try_lock` only (a held lock prints `busy`; a poisoned lock is read through), and writes stderr while ignoring errors, so it cannot panic. `finish()` aborts and joins every owned task, then prints one `[DIAG-3n] pass` line. Store addresses only key the internal registry and are never rendered. Unit tests pin the output to ASCII with no `/`, `\` or `"`.

### Deviations from the plan (OpenRaft 0.10 API and adapter shape)

- The plan's metrics fields `committed` and `millis_since_quorum_ack` are deprecated in OpenRaft `=0.10.0` and fail `-D warnings`. D1 uses `local_committed`, and computes ack age from `last_quorum_acked` at sampling time.
- The adapter appends and flushes in one blocking closure, so "log flush" cannot be measured on its own. It is included in `Append`.
- The plan had the shutdown control rebuild a fixed 200 ms SM-delay hook. As the Sol contract required, the S4 control instead sizes the backlog so that the queued work exceeds the unchanged 5 s budget, and asserts that before shutting down.
- Controls are `#[ignore]` on every platform rather than default tests, which keeps CI time and the Windows quarantine unchanged. They live in `diagnostics.rs` because the guard counts the `ignore` token in `three_node.rs`.

## Checks run (local Windows 11 Pro dev host, current user, no VM or admin)

Target and temp were a unique, task-owned `D:\arx-d1-opus-6b00d17-9842\{target,tmp}`. All commands used `--locked --offline`.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | pass (also on `6b70689` alone, for `-p arxd`) |
| `cargo nextest run -p arxd -E 'test(/^cluster::consensus::/)'` | 68 passed, including the 8 new diagnostics unit tests; the 8 quarantined and 3 control tests skipped as ignored |
| `python tools/ci/test_windows_cluster_quarantine.py` | OK (unchanged guard) |
| Controls `--run-ignored only -E 'test(/^cluster::consensus::diagnostics::controls::/)'` | 3/3 passed on `01a2e0d` (the final run is shown below) |

Not run: hosted CI of any kind (no dispatch or rerun), and Ubuntu or macOS locally. Windows three-node behavior is **unverified on hosted Windows** and **unverified on real hardware**.

### Positive controls (measured)

| Control | Induced seam | Measured result |
| --- | --- | --- |
| `d1_control_s4_queued_apply_exceeds_shutdown_budget` | 40 proposals; 250 ms controlled delay per apply in the leader's blocking worker; election 3–6 s | Backlog at shutdown 40 (10 s of queued work against the unchanged 5 s budget). `ShutdownReport { first: Owners, engine: Joined, owners_waited: 40, owners_left: 21, propose_held: 21, total_ms: 5010 }` and 5.011 s elapsed. Apply ops ≥200 ms: 20, max 285 ms. Window lag 0, so the candidate is `Store`. The attribution is `Stop/Orphaned` and the dump contains `first: Owners`. An earlier run gave owners_left 21 and total 5003 ms |
| `d1_control_s3_isolated_leader_read_attributes_churn` | Isolate the leader until a replacement is Leader at a higher term, heal, read immediately on the deposed node | `Err(NotLeader)`. The before **and** after samples were both a term-1 Leader (`ack_ms` ≈1556), and the class was `Churn`, taken from the replacement's term-2 history |
| `d1_control_timer_lag_is_distinct_from_store_latency` | All 4 runtime workers busy-spin 400 ms at once (bounded rendezvous) | Lag 392 ms (379 ms in an earlier run). Store ops ≥200 ms in the window: 0. Candidate `Cpu` |

The first S3 control run failed in its own cleanup, not in its assertions. Its trailing `converge()` called `leader()`, which returned the deposed node because that node still published term-1 Leader metrics while the replacement was already Leader at term 2. The single read then failed `NotLeader`. That run's dump shows node 1 last sampled as `L` at term 1 at t=1851 ms and as `F` at term 2 at t=1859 ms, and node 3 as `L` at term 2 from t=1844 ms. The correction changed control code only: the control now waits for the deposed node to observe the new term before cleanup. No helper changed.

## Bounded natural loop

- Head `01a2e0d`. Command: `cargo nextest run -p arxd --locked --offline --run-ignored all -j 4 --success-output final --failure-output final -E 'test(/^cluster::consensus::/) - test(/::controls::/)'`. The filter stays anchored to `cluster::consensus` and excludes the induced controls, so the loop measures only natural behavior.
- Cap: 20 iterations or 300 s after the build, whichever came first, stopping at the first red. **Result: 20/20 green.** Each iteration ran 76 tests (68 default plus the 8 quarantined), taking 11.5–12.0 s; 249 s after the build. The time cap was not reached. The 144-run U6 loop was not repeated.
- No red occurred, so no attribution could be taken. **Not reproduced.** Bounds from the 180 `[DIAG-3n] pass` lines (20 per test; u6 builds two fixtures per run, so 40):

| Test | worst lag ms | term changes per node (max) | max Vote / Append / Apply / Other ms | `leader()` polls, one call (max) | `write()` NotLeader retries, one call (max) |
| --- | --- | --- | --- | --- | --- |
| t1 | 20 | 3 | 9 / 14 / 27 / 3 | 34,933 | 38 |
| t2 | 15 | 5 | 34 / 11 / 22 / 3 | 2,632,967 | 785 (862 per run) |
| t3 | 16 | 3 | 6 / 20 / 37 / 19 | 19,105 | 37 |
| t4 | 15 | 6 | 14 / 35 / 24 / 10 | 1,165,850 | 59 |
| u6 | 15 | 1 | 10 / 10 / 38 / 7 | 32,389 | 69 |
| r6 cancelled proposal | 15 | 2 | 7 / 10 / 5023* / 0 | 19,762 | 37 |
| r6 store only | 16 | 2 | 8 / 9 / 10 / 3 | 19,098 | 37 |
| r6 normal + reaped panic | 15 | 1 | 5 / 12 / 13 / 0 | 19,224 | 37 |

\*The test's own `APPLY_PAUSE` gate holds that apply for about 5 s by design. It is not a natural latency. In every run, lag never reached 200 ms, queue wait was 0 ms, the snapshot maximum was 0 ms, and the candidate was `None`, except the gated r6 run, which was `Store`.

## Diagnosis facts (local; not inferences)

1. On this host, 160 natural runs of the eight quarantined tests were green. The worst timer lag was 20 ms and every natural store op stayed below 50 ms. The local host does not exhibit the hosted signatures.
2. A single `leader()` call spun on `yield_now` up to 2,632,967 times (t2) and 1,165,850 times (t4) while the fixture waited for re-election. In every run, the largest single `leader()` call spun at least 11,227 times.
3. A single `write()` call retried `NotLeader` up to 785 times (t2). In the 60 runs where the founding write was the only write, it retried 19–37 times.
4. Within one induced leadership change, two nodes published Leader metrics for different terms at the same time, for at least 7 ms of sampled history. `leader()` returned the deposed node, and the single `converge()` read failed `NotLeader`, while both the before and after samples showed a same-term Leader. Per-node before/after snapshots would have suggested S3(b); bounded cross-node history showed churn.
5. A queued-apply backlog that exceeds the 5 s budget produces exactly the U6 `Orphaned` shape (`first=Owners`, owners left, engine `Joined`) with apply-side latency, while timer lag stays low.

## Natural reproduction limit and next candidate (not implemented)

The local loop cannot reach the hosted conditions, and D1 has no hosted samples because workflow sampling is deferred under the Sol contract. The next candidate is evidence, not a fix: a separately reviewed, measured contract for a non-gating hosted Windows sample that prints `[DIAG-3n]` from the eight tests, kept inside the 12-minute job cap. The seams to discriminate first, when hosted evidence exists, are S1 against facts 2–3 (does lag rise with fixture spin during elections on hosted Windows?) and S3(a) against fact 4. No fix was attempted. The two-attempt Opus fix budget is untouched and needs coordinator evidence and contract review before any dispatch.

## Attempts and process

- Development iterations before the commits (none was a fix attempt): two compile and test corrections to D1's own code (the OpenRaft deprecation, and `/` separators that the redaction test rejected), plus the S3 control cleanup described above. No corrective attempt was consumed after the commits.
- No installs, no cache deletion, no cleanup of other agents' scratch, no VM, admin or Hyper-V step, nothing from the old Lab Manager, and no native M2 fixtures (the filter is anchored). The task-owned D: target is left in place.
- A0 PR149's unmerged source is not on this base and is untouched.
- Status: M2 remains open (A7 NO-GO; A12 consumed, no retry). M3 remains unchecked. Windows three-node coverage stays quarantined and **unverified on hosted Windows** and **unverified on real hardware**. Sol's review of this local commit is next.
