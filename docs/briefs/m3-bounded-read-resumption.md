# M3 PR149 A0 bounded fresh reads: resumption after sampler

2026-11-16. Anthropic / Claude Opus 5.5 / high, research sub-agent. Docs only. Base: sampler head `c9cfaa0` (PR156 candidate, on live main `87c9940`). A0 source: local `work/m3-certificate-issuance-boundary` `8a8c1fd`; PR149 remote head `8eaed30`. No source, PROGRESS or ADR edit. No build, native run, CI dispatch, rerun, push, PR, status, merge or watch.

**Decision: settled, with one prerequisite.** A0 resumes by merging main forward into the reviewed A0 branch. R1–R5 run as **ordinary required tests on all three OS, Windows included**. No new ignore: the quarantine stays exactly the eight ADR 0024 IDs, and the sampler keeps its exact-8 selection. Resumption does not depend on Windows recovery evidence. It does depend on this: **PR156 merged, and exact-main CI green on its merge SHA.** Until then nothing in this brief starts.

## Why R1–R5 can gate Windows honestly

- **Hiding them is forbidden.** A Windows ignore, `cfg(not(windows))`, a moved file or a nextest filter would each extend the quarantine without credit. The guard (`count('ignore') == 8`, no `#[cfg(target_os`) and ADR 0024 both forbid it. The only alternative to gating is not adding the tests, which leaves A0 behavior without tests on any OS.
- **A red is real and attributable.** R1–R5 use the same `Cluster` fixture as the quarantined eight. After the transplant, that fixture carries the PR153 `[DIAG-3n]` attribution. A red inside `leader`/`write`/`converge`/`stop` records a helper attribution: evidence for the quarantined seam. A red on a direct read or shutdown assertion with no helper attribution remains an unclassified A0-path failure; absence of attribution alone proves no root cause. Neither result gets masked.
- **Hosted precedent: one sample, not proof.** On PR149 run 43782024400, R1–R5 passed on hosted Windows (only U6 failed) while all 13 three-node tests competed for the 4 slots. Now that the eight are ignored, R1–R5 are the only three-node tests in the Windows pool, which means less flush contention. That is the only mechanism the cdl7 disposition supports, and it was never proven on hosted.
- **The owner disposition argument still holds, but it does not apply here.** Its "same fixture exposure" reasoning picked the narrowest scope covering *existing* tests that were observed failing. It does not license hiding new tests ahead of time. Gating them openly accepts a risk of red main. The stop rules below handle that risk; no gate is weakened.
- **No credit flows from a green.** A Windows pass of R1–R5 is one exact-head sample. It gives no Windows three-node stability credit and no re-enable credit for T1–T4/U6/R6 (recovery plan §6 is unchanged). Windows three-node remains **unverified on hosted Windows** and **unverified on real hardware**.

## Source diff against current main (trial `git merge-tree 87c9940 8a8c1fd`; same result on `c9cfaa0`)

Source files: `consensus.rs` (auto-merged) and `three_node.rs` (one conflict), about +429/−28. `adapter.rs`, the sampler, the workflow and the guard are untouched: main's PR150 blocking-worker apply hook and the D1 spans carry over unchanged.

- `consensus.rs`: `LeaderId` alias, `READ_BUDGET` = 1500 ms (the same constant as the old `with_wait_timeout(1500)`, so no deadline change), `READ_PERMITS` = 8, and `ReadError::Busy`. `ConsensusNode` gains `id`, `read_permits` and the `cfg(test)` `read_blocking`. `fresh_read`: `try_acquire` (failure gives `Busy`), then an owner tracked in `Owners` under an absolute deadline, then `get_read_linearizer` + `try_await_ready`, then a blocking store read that drops guard, then Arc, then permit before completion (#147). `linearizable_state` wraps it. `shutdown` caps counts at 72 and captures the real `read_held` before abort in both branches.
- `three_node.rs`: the `APPLY_PAUSE_OWNER` Tokio mutex, held by R1 and R6c. The T2 isolated-read pattern adds `Unavailable`, as reviewed at `8237db1`. Adds R1–R5, plus `StoreGate` and `read_units_return`.
- Every pre-existing assertion and deadline is retained. The single reviewed exception is the T2 pattern; T2 is Windows-ignored, and the pattern must not widen further.

## Composition conflicts (instrumentation × A0)

| # | Conflict | Required resolution |
| --- | --- | --- |
| C1 | Textual, `three_node.rs:8`: main has `pub(super) struct Cluster`; A0 adds the static before `struct Cluster` | Keep `pub(super)` and insert A0's comment and static above it. Nothing else changes. |
| C2 | Semantic, compile: `attributed_read` matches `ReadError` exhaustively (NotLeader/NoQuorum/Unavailable), so `Busy` gives E0004 | Add `Busy => Fail::Busy` (the `Fail::Busy` variant already exists), through an exhaustive `read_fail(&ReadError) -> Fail` helper with a unit test over all four variants |
| C3 | Sampler: `--run-ignored only -E <module>` will list non-ignored module tests for the first time. They must be `mismatch`, or `check_inventory` raises `Guard("inventory")` and the required CI job fails (inventory is a gating wrapper-integrity condition, unlike test results) | No selection change. Add a pure fixture: five non-ignored module cases with mismatch reason `ignored` → inventory OK; one non-ignored `matches` → Guard. Confirm the reason string in local nextest 0.9.151 `list --message-format json` output |
| C4 | Evidence contamination: R1/R3/R4/R5 deliberately pause apply or hold the SM mutex inside D1 spans, so their `[DIAG-3n] pass` lines show induced, not natural, store latency | Leave the code as is. Natural latency bounds come only from the sampler's eight tests; never pool R-test lines |
| C5 | Sample epoch: once merged, the eight sampled tests' `converge` uses `fresh_read` (`Unavailable`/`Busy` become possible S3 inputs) | Treat the merge SHA as an epoch boundary; never pool samples across it |
| C6 | `docs/PROGRESS.md` conflict | Take main's file unchanged; the coordinator records progress separately |
| C7 | R5, like main's R6, takes a node without `diag.forget`; its stores stay registered until `finish()`→`close()`. Its expected `Orphaned` correctly bypasses `stop()` attribution | No change. Verify that no "diagnostic observer panicked" occurs |

`APPLY_PAUSE_OWNER` is a reviewed test-correctness lock for a shared static slot. It is not a scheduling or serialization fix for any Windows signature. Under nextest, each process has its own static, and R6c is Windows-ignored, so the lock never contends on Windows.

## Exact transplant sequence (merge-forward; preserves reviews and provider independence)

0. **Prerequisite.** PR156 is merged and exact-main CI on its merge SHA `M` is green. If main later moves and touches `consensus*`, `three_node`, `diagnostics`, `adapter`, the sampler or the guard, re-run `git merge-tree M 8a8c1fd` and re-check C1–C7 before step 1.
1. **Sol high (the A0 builder).** On local `8a8c1fd`, run `git merge M`. Resolve only C1 and C6. Prove that `git diff M HEAD -- crates/` equals `git diff 01a42d5 8a8c1fd -- crates/` apart from the C1 context line. Do not touch helpers, `Params`, deadlines or the adapter.
2. **Sol.** Commit `test(consensus): attribute Busy fresh-read refusals` (C2 + unit test). Commit `test(ci): pin sampler inventory with non-ignored module tests` (C3 fixture only).
3. **Sol local verification**, run once, with no loops or repeats. All of these: `cargo nextest run -p arxd --locked -E 'test(/cluster::/)'`; in-process `cargo test -p arxd --lib --locked cluster::` (the shared statics `APPLY_PAUSE` and `REGISTRY`); the three D1 controls via `--run-ignored only`, whose control tests must pass while confirming their respective induced failure/latency attribution; the quarantined eight once, locally only, as a sanity check under the new read path; the guard and the 57+ sampler tests; Node workflow tests; workspace Clippy, fmt and diff check. Append the results to `docs/evidence/m3-bounded-fresh-read.md`. Any local three-node red stops Sol: hand back with evidence, and make no tweak.
4. **Opus high** reviews only the delta: the merge resolution, the diff-of-diffs proof, steps 2–3 and C1–C7. Retained A0 reviews cover the unchanged hunks: Opus review `c0bf702`, the refusal recheck `08f8832`, integration review `df0d42e`, and recheck `62bfd25` (closing the `6eb5ad8` mutex). Fallback reviewer: grok-4.7 high.
5. **Coordinator.** Fast-forward push to PR149 (`8eaed30` is an ancestor, so no force). Then the five gates and an exact-head `agent-review` by Opus. The merge gate requires all of:
   - R1–R5 *executed and passed* on all three OS (not not-run under fail-fast);
   - the guard passes with exactly 8;
   - the sampler step reports the exact-8 inventory;
   - the Windows job stays within budget. If it does not, raise no cap: return to design.
6. **Coordinator** squash-merges. The next exact-main CI must be green before any other merge.

## Red handling (pre-registered; no rerun of any head)

- **R1–R5 red on the PR head, with a helper attribution** (quarantined seam): no merge, no ignore. Route the `[DIAG-3n]` evidence to the Opus recovery plan. A0 is then **blocked** until recovery plan §6 re-enable evidence exists or the owner decides. This is not a Sol attempt and not an Opus fix attempt.
- **R1–R5 red on the PR head, no helper attribution** (unclassified A0-path failure on Windows): no merge. It goes back to the A0 owner under the stuck rule, with Opus review. Make no deadline, budget, permit or retry change.
- **Any R red on exact main after merge:** merges stop. Record it and ask under delegated steering. This brief grants no revert onto red main, quarantine or rerun.

## Not authorized here

New or moved quarantine; timeout, deadline, retry, scheduling or test-group changes; a fourth Sol cluster fix; any change to fixture helpers; acceptance waivers; T1–U6/R6 re-enable. M2 stays open (A7 FINAL NO-GO, A12 consumed with no retry). M3 stays unchecked: no real transport or hosts. Windows three-node stays **unverified on hosted Windows** and **unverified on real hardware**.

## Coordinator Sol contract review

2026-11-16: inspected actual `check_inventory` and `attributed_read`, confirmed existing `Fail::Busy` and the old A0 two-file source delta. Corrected C3: inventory errors gate the required job. Positive controls pass by recognizing induced conditions; they are not expected to fail as test cases. Absence of helper attribution does not prove an A0 production fault. These clarifications preserve the no-rerun/stop rules. The diff-of-diffs comparison must account for main's diagnostic module/context while proving A0 behavior hunks unchanged; do not restore obsolete files from the old branch. PR156 has merged as `a18e95c`; its exact-main CI43836099670 remains pending at this review. Implementation is still held on that concrete prerequisite.
