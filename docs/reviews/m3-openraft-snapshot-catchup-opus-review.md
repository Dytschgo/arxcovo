# M3 OpenRaft T3 snapshot catch-up: independent Opus review

Reviewer: Opus 5.5 high, 2026-11-15. Target `8d9a8e4893c206701e89011bb79372d5a38117c1`
(`t3/review-openraft-snapshot-test`, refreshed `work/m3-openraft-snapshot-catchup`). Parent: PR #145 head
`80f87e376b5dbdf27ea116c0d9fc87cbe3985617`, independently reviewed PASS (6901884167/status 58979456839); its
hosted gates are still pending. Builder: Sol high `19f89e75eec4619372ca28831a31059b440ef731`, retained as
`146134096c38b909f023cc1fbbfade91c43a7605`. Contract: brief `docs/briefs/m3-openraft-adapter-first-slice.md`
(the T3 line, decision 9 "never relax" preflight, decision 10 test params), ADR 0005, ADR 0023, evidence
`docs/evidence/m3-openraft-snapshot-catchup.md`, and the parent review `docs/reviews/m3-openraft-three-node-opus-review.md`.

## Verdict

- **Standards: PASS.** No blocker. The low notes are listed below.
- **Spec: PASS** for the brief's T3 line, for the exact in-process slice.
- **Security: PASS.** The change is test-only. It makes no production authentication claim.
- **Eligibility:** the T3 source and content may be published **only after** parent PR #145 has passed its
  hosted gates and been merged. T3 then needs its own PR, exact-head review and green required gates (three
  OSes, ci-time-budget, agent-review). None of those gates have run for T3. This review gives no KICKOFF M3
  acceptance and no T3 PROGRESS credit.

## Provenance and scope (verified)

- `19f89e7^{tree}` and `1461340^{tree}` are both `920ec876…`. The retained commit is byte-identical to the builder's.
- `3fb24d2..1461340` changes exactly three files: `three_node.rs` (+57/−1), `mem_net.rs` (+10/−1) and the new
  evidence (+96). `bd0471e` adds two PROGRESS lines only.
- Merge `8d9a8e4` brings in only `docs/PROGRESS.md` and the parent review. That review's blob `5cfb28a…` is
  identical on both sides. All three T3 feature blobs (`e799e25`, `c66b9ff`, `5a5a4b1`) are identical at
  `19f89e7`, `1461340` and `8d9a8e4`. The PROGRESS conflict resolution keeps the T3 entry, the parent-review
  entry and the new coordinator line. The parent side's "Current work" line wins, which is the right result.
- Nothing changes in the production adapters, stores, `consensus.rs`, Cargo manifests, `Cargo.lock`,
  workflows or ADRs (a 0-line diff over every manifest, lock and `.github` path). `mem_net` and `three_node` are
  both `#[cfg(test)]` (`consensus.rs:403-406`).

## Contract conformance

| Brief T3 requirement | Source | Verified |
|---|---|---|
| Small params 8/0/1/16 pass `check()` | `params.config().unwrap()` before the cluster starts | yes |
| Isolate and shut down a follower | `stop(f)` isolates it, removes its router entry, then does an owned shutdown | yes |
| Commit 40 Redeem-unknown entries | `181..221`, each asserted `Rejected(UnknownCode)` | yes |
| Until the leader has purged beyond the follower | `Raft::wait(5 s).metrics(purged > follower_applied)` | yes, purged 39 > 2 |
| Restart (same dir, mandatory preflight) | `c.start(f)` → unchanged `ConsensusNode::start` | yes |
| ≥ 1 `full_snapshot` to it | per-destination `snapshots_to[f+1]`; `None` asserted before restart | yes, 1 |
| Applied converged, blobs equal | `converge()` now also asserts `s.applied() == p` on every node | yes |
| Consecutive applies afterwards | the next write lands at `before + 1`, then everything converges again | yes (one apply; see note 5) |
| No blind sleeps | only `Raft::wait`, `timeout`, `yield_now`; outer deadline 45 s | yes |

The counter sits after the size check and after `remote()` (sender, isolation and route checks), immediately
before the real `install_full_snapshot`. It counts RPCs that reached the target, not successful installs.
Convergence is the oracle that proves the install happened, and counterfactual A below shows the counter alone
cannot pass the test.

The fixture's added shutdown assertions are the store `Weak`s released, 64 permits, empty owners and a
`Stopped` refusal. They run on every stop and at `finish`. `finish` still bounds append ≤ 64, snapshot ≤ 16384
bytes and mismatches == 0. The marker scan reads real persisted files. `unknown()` redeems carry
`JoinCode::proof(code 100)`, so the scan's absence check for proof 100 is a meaningful negative over a
snapshot-bearing store. ADR 0005 requires that only verifiers are persisted.

## Actual checks (mine, this review)

All runs used `--locked --offline` with `CARGO_TARGET_DIR=D:/Temp/arxcovo-openraft-t3-review-target`,
`TEMP=TMP=<target>/tmp`, a fresh target, no installs, no deletions and no cache cleaning. Logs are under
`<target>/logs/`.

- **Focused T3** (`--exact --nocapture`): 1 passed in 1.71 s, after a cold build of 1 m 10 s. Follower 2:
  stopped_applied 2, leader purged 39, snapshot RPCs to follower 1, snapshot_max 198 bytes, mismatches 0,
  final applied 43. I repeated it 5 more times; all passed in 1.07–1.67 s with the same positions, and
  append_max varied from 3 to 17.
- **`cargo nextest run -p arxd --locked --offline cluster::`: 99 passed**, 456 skipped, 8.904 s. T3 3.110 s,
  T1 1.226 s, T2 7.988 s, U6 1.891 s. Both four-process-kill probes passed.
- `cargo fmt --check`, `git diff --check 3fb24d2 8d9a8e4`, and
  `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings` (33 s) all pass.

### Counterfactuals (temporary; source restored with `git checkout`, worktree clean afterwards)

- **A: the counter is satisfied but the install is lost.** After the `snapshots_to` increment, I returned
  `Unreachable` for every target instead of calling `install_full_snapshot`. The test failed with exit 101 in
  5.72 s, at `converge`: `applied_index>=42` timed out while the follower's last_log and last_applied stayed at
  2 and the cluster had committed 42. So a counter-only proof cannot pass. This repeats the builder's
  route-disabled red (5.80 s), with the drop moved after counting.
- **C: snapshot params removed (defaults: snapshot every 64, keep 32).** The test failed with exit 101 in
  6.18 s, at the purge wait: "leader purged beyond stopped follower" timed out with snapshot `None` and purged
  `None` at last_log 42. So the purge precondition is not vacuous, and T3 depends on the small params actually
  driving engine snapshot build and purge.

### Additional probe: restart around the install/purge boundary (temporary, not retained, no credit)

Parent finding 5 asked whether a legitimate restart around install or purge trips the preflight. After T3's
final converge, I added two restarts:

1. Stop and restart the follower that had installed the snapshot (its snapshot and purged were both 42).
2. Stop the purged leader (purged 39), write `unknown(222)` through the new term-2 leader, then restart the
   old leader.

Both restarts passed `ConsensusNode::start` preflight and converged: the follower at applied 43, the old leader
at applied 45. RPC bounds held (append_max 17, one 198-byte snapshot, mismatches 0), and the run took 3.72 s.
No preflight refusal was observed, and no relaxation was involved. This is reviewer evidence only. T3 as
retained does not restart after install.

## Reused, not rerun

The parent review covers the unchanged production adapters, preflight, service, Suite partition, dependency
lock and the T1/T2/U6 semantics. I relied on it for those and did not re-review them. I did not run hosted
three-OS checks or measure full CI wall time. Owner fixtures and the broad workspace suite were not run.

## Non-blocking findings

1. **Long-outage `Full` liveness is still unexercised (risk, accurately unclaimed).** The 40-entry outage sits
   far below `MAX_LIVE` 512. The parent's request for `Full` liveness under a long follower outage remains open.
   The evidence does not claim it. It should not be read into "snapshot catch-up".
2. **Restart after install is not retained (low).** My probe shows both post-install and post-purge restarts
   pass preflight. A retained version (about 6 lines in T3) would close parent finding 5 permanently. It is
   optional and belongs in a later test slice.
3. **The retained positive control is matcher-level only (low).** `control = prefix+m+suffix` proves that
   `contains` works. It does not prove that the scan enumerates and reads the store files. The parent's
   temporary probe (the `[13;16]` public value, `seen=true`) covered that. The forged-sender `mismatch` control
   from parent finding 3 is still absent.
4. **The `Weak` assertions in `stop()` re-check `shutdown()`'s own loop condition (informational).** Parent
   finding 1 (release-after-field-drop signal) is unchanged. As requested, there is no churn in this slice.
   "Weak references released" must not be read as proof that the files are closed. The parent's non-blocking
   finalizer/read-deadline advisories stay as they were, and I observed no concrete T3 failure from them.
5. **Small timing and coverage edges (low).** The `after.index == before + 1` check would flake if a leader
   change happened between the linearizable read and the write. That window is very small in-process, and I
   saw 0 of 7 failures. "Consecutive applies" is satisfied by one further apply plus reconvergence. The global
   `snapshots` counter now counts only routed RPCs rather than attempts. It only appears in `eprintln`; the
   parent's `snapshots=0` statements remain true.
6. **The evidence status line is stale after the merge (docs nit).** It still says T1/T2 are "separately
   pending independent review", but they are now reviewed PASS at PR #145. The coordinator should refresh it at
   T3 publication. I made no edit.

## Boundaries

The proof is in-process: graceful restarts, logical time, and actual OpenRaft 0.10.0 over persistent local
private SQLite stores, with a test-only in-memory RPC. The snapshot RPC is a test double, not a production
transport or an authentication claim.

Not claimed:

- full KICKOFF M3
- crashes during consensus or power loss
- key authentication
- separate processes or hosts
- TCP, mTLS, CA or signing
- physical expiry
- `Full` long-outage handling beyond the existing store bounds

All are **unverified on real hardware**. M2 stays open: A7 is FINAL NO-GO, and A12's attempt is consumed with
cleanup uncertain. No retry is authorized.

Retention: this file is the only change and the only commit. No source, PROGRESS or ADR edits. No push, PR,
status, comment, merge, watch, subagent, native M2, admin, VM, Lab Manager, dispatch, variable, listener, CA or
signing action.
