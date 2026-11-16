# Opus review: log-store shutdown close-wait regression (P1 follow-up)

Disposition: **APPROVE test-only follow-up (no blockers); one Standards advisory to fix before publishing, two nits/info.**
This is not merge permission. Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-16. Single reviewer, no
subagents. Independent different-provider review of a Sol coordinator (medium) change. Standards and Spec axes are separate.

## Exact pins

| Item | Identity |
| --- | --- |
| Reviewed head (isolated worktree, clean before and after all runs) | `e6e7a88567f1f79e9368adfde4fb0fd9ba43213e` |
| Base (live green main, PR158 merge, merge-base of head) | `e601caa594f591cc585007be8a455c07282cde8b` |
| Commits | `e6e7a88` only |
| Changed paths (3) | `crates/arxd/src/cluster/consensus/tests.rs`, `docs/PROGRESS.md`, new `docs/evidence/m3-log-store-close-wait.md` |
| `consensus.rs` blob, base and head (no production change) | `5b83422bf8d0cc56b7c76ad15609543c40c14e00` |
| `consensus/tests.rs` head blob (matches evidence pin) | `d804f9b752959c0f1021d49817a5a548baf1e987` |
| Spec | `docs/briefs/m3-store-close-completion.md` regressions 2–3, and P1 of `docs/reviews/m3-store-close-completion-opus-review.md` |

`git diff --name-only e601caa e6e7a88` lists no path outside the test file and `docs/`. No Cargo, lockfile, workflow, ignore,
retry, timeout or production source change.

## Checks run by this reviewer

Windows local, `CARGO_TARGET_DIR`/`TEMP`/`TMP` under the unique `D:/arxcovo-logwait-e6e7a88-opus-review`, `--locked --offline`,
`-p arxd --lib`, filter `store_close_completion_shutdown`, `--test-threads=1`. Logs stay in that directory and are not committed.

| # | Tree | Result |
| --- | --- | --- |
| 1 | head (fresh target, build 1m13s) | exit 0, 3 passed, 568 filtered, 6.13 s (`control.log`) |
| 2 | head with only the wait changed to `while !self.sm_released.is_set() {` (mutant SHA-256 `420efe62…b70c`) | **compiled**, exit 101: `store_close_completion_shutdown_waits_for_log` FAILED at `tests.rs:933` (the 500 ms negative window) with `shutdown returned while store destructor held its lock`; both State tests passed; 5.61 s (`mutant.log`) |
| 3 | restored head (`git checkout --` in an EXIT trap; blob re-hashed `5b83422`, status clean) | exit 0, 3 passed, 6.13 s (`restored.log`) |
| 4 | head | `cargo fmt --all -- --check` exit 0; `git diff --check e601caa e6e7a88` exit 0 |

Run 2 also exercised the failure cleanup path: the test panicked while the `DropGate` held the log destructor, and the process
still finished (the unwound `release_tx` unblocks `DropGate::drop`), so a red run does not hang.

**Reused, not rerun:** Sol's Clippy `-D warnings` (10.68 s) and Sol's own control/mutant/restore runs; my runs independently
reproduce the same outcomes. PR158's source controls, its 140-test cluster pass and the main CI `43862021319` result are
cited from PROGRESS and were not re-verified. **Not run:** the state-half-deleted mutant, full cluster, `three_node`,
D1/quarantined tests, integration binaries, hosted CI, native M2, VM or hardware. No full-suite or hosted claim.

## Contract checklist (test)

- **Real log destructor and file lock held.** The Log arm takes `node.log_weak.upgrade()`, the same `Arc<Mutex<VoteLogStore>>`
  allocation openraft owns, and installs the real `DropGate` through `VoteLogStore::hold_drop`. `gate` is declared immediately
  before `_lock` and `release: ReleaseAck` last (`log_store.rs:164-169`), so the gate blocks inside the real destructor while the
  file lock is held. The store-level held-gate reopen==`Io` test from PR158 already pins that placement.
- **No discarded ownership or hidden reference.** Unsized coercion to `Arc<dyn Any + Send + Sync>` keeps the same allocation and
  strong count; `weak` is downgraded after erasure, from that allocation. The `lock()` guards are statement temporaries. `released`
  is the separate observe-only `Arc<AtomicBool>`, not a store reference. The bounded `strong_count == 1` wait, then `drop(held)` on
  a `std::thread`, makes the test thread run the last-owner destructor, and `weak.upgrade().is_none()` proves Weak expiry before
  the ack, as the brief requires.
- **Assertions unchanged.** The same sequence runs for both stores: `!released.is_set()` while held, `start.elapsed() < 4s`, the
  500 ms window must elapse, release, join, `released.is_set()`, shutdown `Ok(())` within the 2 s fixture bound, then an immediate
  `ConsensusNode::start` on the same dir and a clean shutdown with no retry. The deadline branch (5 s production deadline,
  `report.first == StoreRelease`, `Joined`, both strong counts 0, `total_ms >= 5000`, `< 6s`) is byte-identical and still selected
  only by the existing State deadline test.
- **Placement.** The new test is an ordinary all-OS `--lib` unit test on `mem_net` single-node, outside `three_node` and the
  quarantine. It adds about 0.5 s.

## Standards

No documented-standard violation in the test code. One conventional commit, one concern, a runtime-proven behavior, fmt clean.

- **A1 (advisory, fix before publishing; PROGRESS accuracy).** The PROGRESS diff rewrites two *historical* sections, not only the
  current one: `## Historical parent checkpoint` (base line 693) and `## Historical state before Stage C integration after CI recovery`
  (base line 700) now say "Last merged: PR #158 … e601caa …". Their surrounding bullets describe the #54/#57 Stage C era, so the
  new text is false history. The base text there (PR #137) was already anachronistic from an earlier replace-all; this commit
  repeats the pattern. AGENTS requires PROGRESS to stay accurate. Recommend reverting those two hunks (or restoring the
  period-correct text) and editing only `## Current state`. Docs only; it does not affect the test verdict.
- **S1 (nit, possible Duplicated Code).** The two `match` arms are identical except for how `held` is obtained, because the stores
  share no trait. A tiny generic `fn gate<T>(…)` would need a trait; the current duplication is short and the erasure comment explains
  it. Judgement only.

## Spec

P1 is closed as requested: a log-gated shutdown control now exists, and the exact surviving mutant from the PR158 review
(run 5 there) is killed at runtime by the intended assertion, with no compile-failure credit. Brief regressions 2 and 3 remain
implemented unchanged for the State store. No behavior outside P1 was added.

- **I1 (info, not required).** There is no Log-gated *deadline* variant. P1 asked only for a log-gated shutdown control, and the
  waits variant already kills the log-half deletion. A log deadline variant would add about 5 s and prove `Orphaned` truth when only
  the log destructor is late; optional, before or with the first restart caller.

## Blockers, limits, reversibility

- **Blockers:** none for the test. A1 should be fixed before the PR is published.
- **Remaining merge gates, not satisfied here:** all five required gates on the exact PR head, fresh green exact main, and an
  independent review bound to the final head if A1 changes bytes (a docs-only PROGRESS change would not alter this test verdict).
- **Not claimed:** no production restart caller, no SQLite/OS close-success guarantee from the acknowledgement, no historical
  flake fix, no hosted or Windows stability credit for the new test. TLS PR159's held Ubuntu S8 and Windows engine failures are
  separate and neither fixed nor waived by this follow-up. M2 stays open, M3 unchecked, the eight Windows three-node quarantines
  unchanged, and all hardware paths **unverified on real hardware**.
- **Reversibility: two-way door.** Test and docs only; reverting `e6e7a88` restores the prior tests.

No source, PROGRESS, evidence or brief edit. No push, PR, status, comment, merge, watch, CI rerun, dispatch, install, native M2,
VM or admin action.
