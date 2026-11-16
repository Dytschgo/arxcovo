# M3 Windows three-node: Opus diagnosis and re-enable plan

2026-11-16. Anthropic / Claude Opus 5.5 / high, design sub-agent. Base: quarantine PR152 head `7bf2d53` (on main `01a42d5`, which is red). Docs only: no source, config, workflow, PROGRESS or ADR change, no CI dispatch or rerun, no fix. **Status: proposal.** Sol contract review is required before any implementation, and the coordinator dispatches the first task only after main is restored.

## Authority and limits

- Sources: KICKOFF stuck rule (hand off to the other provider after three failed fixes) and stop condition; owner disposition `dc55dca` (retained as `95e4e28`); ADR 0024; PROGRESS clarification of 2026-11-16. That clarification lets the reviewed quarantine merge onto red main after all five gates pass, and requires the next exact-main CI to be green before any feature merge. It supersedes the pending owner exception question.
- Sol's three-attempt stop stays in force. This plan does not authorize a fourth Sol tweak. Opus owns the work below, Sol reviews it, and grok-4.7 high is the reviewer fallback if Sol hits a provider limit.
- The eight tests stay quarantined on Windows until the re-enable criteria in §6 are met. Ubuntu and macOS keep running all eight as required tests. Windows three-node is **unverified on hosted Windows** and **unverified on real hardware**. M3 stays unchecked: re-enabling does not credit the M3 acceptance, which needs a real transport and real hosts. M2 stays open: A7 is NO-GO, and A12 is consumed with no retry.

## 1. Evidence (hosted Windows only; Ubuntu and macOS have no three-node failure)

| Run | Head | Signature | Helper / site |
| --- | --- | --- | --- |
| 43782024400 | PR149 `8eaed30` | U6 `Err(Orphaned)`, resolved 43/64, 20.7 s | `stop()` → `shutdown()` (`three_node.rs:78`) |
| 43788629298 | main `01a42d5` | T2, U6 `leader deadline`; T3, T4 write `Elapsed` | `leader()` 5 s; `write()` 5 s |
| 43794002309 | PR151 `fb64b3e` | T4 `Err(NotLeader)`, 19.7 s | `converge()` single `linearizable_state()` (`:121`) |

Known so far: induced SM-op latency of 200 ms or more is a *sufficient* mechanism for U6 `Orphaned` (U6 diagnostic disposition), and the shared-flush model removed the leader-deadline signature locally. Both results are **compatible with the hosted failures, but neither is proven to be their cause.** Sol's 144 unmodified local U6 runs never went red.

## 2. Distinct root-failure seams (test the seams, not fixes)

The four seams below can share one upstream cause: slow fsync and SM apply on the hosted disk, CPU starvation of the 4-worker runtimes, or both. Each needs its own discriminator.

- **S1 Leadership loss in a quiet cluster** (T2 and U6 `leader deadline`, and possibly the cause of S3). With heartbeats every 100 ms and elections timing out at 500–1000 ms, losing the leader without a partition means heartbeats or vote persistence stalled for over 500 ms. To discriminate: record the term and leader history per node, plus timer lag (CPU) against vote/append store-op duration (disk). Separately, `leader()` busy-spins on `yield_now`. That makes the fixture itself a candidate source of CPU contention; it must be measured, not assumed.
- **S2 Write commit stall** (T3 and T4 `Elapsed`). Determine where the in-flight proposal sat when the 5 s expired: retrying a `NotLeader` loop (that is S1), log append/flush, replication, or SM apply. Count `write()` retries and record the commit and apply indexes when it times out.
- **S3 Convergence read: helper churn vs production consistency** (T4 `NotLeader`). `converge()` reads the leader from metrics once, then calls `linearizable_state()` once, and does not retry. Two different causes give this error:
  - (a) **Helper / leader churn**: on that node the term advanced, or the node stepped down, between the two calls. Production then correctly returns `NotLeader`, and the real question moves to S1.
  - (b) **Production read defect**: the same term, the node still reports itself as Leader in metrics both before and after the call, and `ensure_linearizable` returns `ForwardToLeader` anyway; or the read returns `applied` below an acknowledged index.
  To discriminate: snapshot (term, state, current_leader, millis_since_quorum_ack) on the reading node immediately before and after the call.
- **S4 Shutdown queued-apply tail** (U6 `Orphaned`). After `try_shutdown`, the OpenRaft SM worker keeps draining queued applies, and proposal owners wait on them. `ShutdownReport` (#150, on main) already carries the failing phase. A repeat must show `first=Owners` with `owners_left>0` before this seam is credited. Pair the report with the SM-op latency distribution to tell **spikes** from **uniform slowness**. Uniform slowness was modelled as passing.

## 3. Minimal observability, before any hypothesis (test-only, bounded)

All of this is `#[cfg(test)]` instrumentation with no change to production behavior. It records only bounded integers and enums: no paths, SQL, payloads, join codes, addresses or error strings.

1. **Per-node metrics ring.** A watcher on each node's OpenRaft `metrics()` watch channel (event-driven, no polling) keeps the last 128 changes of (ms since test start, term, state, current_leader, last_log_index, committed, last_applied, millis_since_quorum_ack).
2. **Store-op latency buckets.** Atomic counters for vote save, log append, log flush, SM apply and snapshot build, bucketed at <10 / <50 / <200 / <1000 / ≥1000 ms, plus the maximum. They sit at the existing `spawn_blocking` boundaries in the adapters, behind a `cfg(test)` hook.
3. **Timer-lag probe.** One 50 ms interval task per test records the worst lateness and the count of ticks over 200 ms late. A large lag with fast store ops points to CPU; a small lag with slow store ops points to disk.
4. **Helper attribution.** Each failing helper records which helper failed (leader, write, converge or stop), its iteration and retry counts, and the S3 before/after snapshot.
5. **Output.** A failure-only dump, ≤8 KiB, tagged `[DIAG-3n]`, from a drop guard on `Cluster` while panicking. Passing runs print a single summary line (bucket maxima, term changes, worst timer lag), so a green run also yields evidence.

**Positive controls, required before trusting any green run.**
- A test-only SM-delay hook at 200 ms (the retired U6o probe was never merged, so it must be rebuilt behind `cfg(test)`) must turn U6 red and be attributed to S4, with SM apply in the ≥200 ms bucket.
- Isolating the leader in the router between `leader()` and the read must produce S3(a), with a term change recorded.
- A CPU-burn control must raise the timer lag without moving the store-op buckets.
Each control must fail with its own attribution, or the instrumentation is not accepted.

## 4. Red-capable loop

- **Local.** Run on the existing ordinary Windows dev host as the current user: no VM, no admin step, no Hyper-V, and nothing from the old Lab Manager. Use nextest, which isolates each test in its own process, and reuse the existing worktree target. If a cold target is ever needed, use a unique target directory per run and delete it afterwards. Command: `cargo nextest run -p arxd --locked --run-ignored all -j 4 -E 'test(/consensus::/)'`. The quarantine uses `ignore`, so `--run-ignored all` still runs the eight tests locally. Run 20 iterations at most and stop at the first red. This first run doubles as a contention variant: the whole consensus set runs alongside the eight. Do not repeat the 144-run U6 loop.
- **Hosted sampling without dispatch or rerun.** The instrumentation PR may add *one* non-gating Windows step that runs only the eight tests with `--run-ignored only` and prints `[DIAG-3n]`. It must stay within the 15-minute ci-time-budget, needs a separate contract review, and runs naturally on every PR push and main CI. Each run is one hosted sample. The step never turns a job red, so the quarantine still gates.
- If neither loop goes red, the hosted summary lines still bound the latencies. The diagnosis then reports "not reproduced; bounds X", not a cause.

## 5. Ownership and bounded attempt budget (Opus)

- **D1 instrumentation PR.** Test-only, the quarantine stays in place, and it is **not a fix attempt**. It goes through the five gates and Sol review.
- **Fix attempts.** At most **two Opus fix attempts**, counting from the first fix PR. Each must name one seam and cite `[DIAG-3n]` evidence that attributes the failure to that seam. A hosted Windows red on a fix PR's head consumes an attempt. Never rerun the same head.
- **Forbidden as a fix or a mask:**
  - raising any deadline or timeout (helper 5 s, read 1.5 s, test 45 s, shutdown 5 s, election timing);
  - new retries in helpers or production;
  - scheduling groups or serialization;
  - narrowing, widening or extending the quarantine without credit.
  A `converge()` change is acceptable only together with an evidenced S1 root cause, and only if it still fails on a same-term `NotLeader` (S3b).
- **Stop.** After two failed Opus attempts (five in total across both providers), stop and record the failure. The quarantine and the M3 block remain. The next step is the owner's option B (a controlled runner) or a new owner decision. There is no third Opus attempt.

## 6. Re-enable criteria (all eight together, one PR)

The re-enable PR removes all eight Windows gates and updates the guard test to pin zero ignores. It keeps the instrumentation. It merges only when all of the following hold:
- instrumented hosted Windows CI is green on its exact head, and the summary lines show no term churn;
- all five gates are green;
- Sol has reviewed it independently, as the different provider;
- the coordinator merges it.
After the merge, the **next exact-main CI must be green**. If any of the eight goes red there, revert to the reviewed quarantine at once, count the failure as an attempt, and allow no rerun.

## 7. Concrete next actions

1. **Root (now):** keep the PR152 pipeline running. Main must be restored, meaning PR152 merged and the next exact-main CI green, before anything here starts.
2. **Coordinator, after that:** request a Sol contract review of this plan, covering §3 scope, the §4 non-gating step and the §5 budget.
3. **First Opus task on a pass:** "m3-3n-diag-instrumentation". Implement §3 items 1–5 test-only and prove the three positive controls locally. Then run the bounded local loop once and open the D1 PR with the evidence, with no fix included. The coordinator reviews it before Sol's exact-head review.
