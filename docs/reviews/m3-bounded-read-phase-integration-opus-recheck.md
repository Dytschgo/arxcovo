# M3 bounded-read / shutdown-phase integration: Opus F1/F2 correction recheck

2026-11-16. Opus 5.5 high. This is a narrow recheck of the F1/F2 correction only, done as a review sub-agent.

- Target: `6eb5ad8ee4a0ddb948b6cb5ddda22e375d57c726` on `work/m3-certificate-issuance-boundary`.
  The prior review is `docs/reviews/m3-bounded-read-phase-integration-opus-review.md`. It was written as
  `f8e945b` and is kept on this branch as `df0d42e`; the review text is the same.
- Scope: the `6eb5ad8` diff (`three_node.rs` +5, the A0 brief, the A0 evidence) checked against F1/F2. The
  production integration at `34bf2d6` was already reviewed. Here I only confirmed it is unchanged.
- No push, PR status change, merge, watch, child agent, VM, admin or owner-data operation. This file is
  the only commit.

## Verdict

| Axis | Verdict |
| --- | --- |
| Standards (local test determinism, lint) | PASS. F1 resolved. |
| Spec (A0 brief, evidence, Opus F1/F2 requirements) | PASS. F2 resolved. |
| Security | PASS |

**Overall: PASS. F1 and F2 are closed and there are no new findings.** A separate review of the exact
published head after the push is still required.

## Source unchanged where it must be

- `git diff 01a42d5 6eb5ad8 -- crates/arxd/src/cluster/consensus/adapter.rs` is empty. The adapter is
  byte-identical to main PR150.
- `git diff 34bf2d6 6eb5ad8 -- crates/arxd/src/cluster/consensus.rs` is empty. Production is the same as
  at the reviewed integration.
- `6eb5ad8` touches only `three_node.rs`, the brief and the evidence. No Cargo, lockfile or workflow
  change and no new dependency. Tokio 1.48 with the `sync` feature already provides `Mutex::const_new`.
- In the fixture source, only the lock lines were added. No deadline, permit, timeout, assertion or
  ownership check changed: 45 s, 2 s, 4 s, 2.5 s, 5.5 s and 63 permits are all as they were.

## F1: resolved

- `static APPLY_PAUSE_OWNER: tokio::sync::Mutex<()> = Mutex::const_new(())` is test-only (`three_node.rs`
  is a `#[cfg(test)]` module). It is acquired as the first statement inside the 45 s `timeout` future of
  both `r1_apply_wait_deadline` and `r6_shutdown_report_cancelled_proposal`. The guard lives until the
  end of that async block. That covers hook installation, the paused-apply window, release,
  `converge`/`finish` (R1) and `report_cleanup` (R6), so the whole hook lifetime is covered.
- These are the only two users of `adapter::APPLY_PAUSE` (grep: adapter definition plus
  `three_node.rs:771` and `:1093`).
- **No deadlock.** It is a single async lock with no nesting. Tokio's mutex is FIFO-fair. No `std` guard
  is held across `.await`: the `APPLY_PAUSE` std lock is taken and released within one statement, and
  `pause_apply` only takes it briefly on a blocking thread. The holder never waits for the other test.
  The hook's `release.recv()` runs on a `spawn_blocking` worker, not on a runtime worker. If a holder
  panics, unwinding drops the `release` sender, which unblocks the paused worker and drops the guard.
  Tokio mutexes do not poison. A stale `Some` slot left behind by a panicked fixture is harmless.
  The next owner overwrites it. If the store pointer is ever reused, `recv` sees the disconnected
  sender and returns at once. Waiting for the lock counts against each test's 45 s budget, but the
  worst-case wait is the other fixture's runtime of about 7 s or less.
- The brief now requires that every future shared-slot fixture (the issuance guard test at brief line 179)
  take the same owner lock.

## F2: resolved

- Brief lines 127-128 now describe the hook as main PR150's `#[cfg(test)]` `APPLY_PAUSE`, keyed by the
  `SmAdapter` Arc pointer and called inside the blocking worker before the store mutex. That matches
  `adapter.rs:289-292`. The brief adds the owner-lock rule.
- The A0 evidence addendum records the following:
  - The current integration has the actual `read_held` snapshot (`consensus.rs:556`/`:562`), the
    `64 + READ_PERMITS` = 72 cap (`consensus.rs:493`) and R5 read attribution.
  - These supersede the main-only `read_held` 0 / 64 baseline for this branch.
  - PR150's evidence is still an accurate historical main-only record.
  - The 117-test results came from the coordinator's runs at `34bf2d6`.
  - The addendum makes no green or hosted claim after F1.
  - The addendum does not rewrite PR150 history.

## Security

No production code, input surface, paths, payloads or secrets are involved. The change is a test-only
static with no `unsafe`.

## Checks actually run by this reviewer at `6eb5ad8`

Unique target `D:/arxcovo-opus-recheck-6eb5ad8-7193`, TEMP/TMP `D:/Temp`, `--locked --offline`, local Windows.

| Check | Result |
| --- | --- |
| `cargo test -p arxd --lib --no-run` | built, 53.77 s |
| Exact pair `--exact …r1_apply_wait_deadline …r6_shutdown_report_cancelled_proposal`, default threads, ×8 (failed **8/8** at `34bf2d6`) | **8/8 pass**, 2/0 each, about 6.87 s each. That is the two runtimes added together, so the fixtures run one after the other. |
| `three_node::r` filter, default threads, ×3 (failed **3/3** at `34bf2d6`) | **3/3 pass**, 8/0 each, about 7.03 s |
| `three_node::r` filter, `--test-threads=4`, ×2 | 2/2 pass, 8/0 each, 8.69 s |
| `cluster::` full filter, default threads | **117 passed, 0 failed, 2 ignored**, 416 filtered out, 9.48 s |
| Mutation: removed only R6's `APPLY_PAUSE_OWNER` line, rebuilt, exact pair ×4 | **4/4 FAIL**: R1 panics at `three_node.rs:781` (the `entered` `RecvError` unwrap). Both owners need the lock. |
| Mutation restored with `git checkout -- three_node.rs` | `git status --porcelain` empty and `git diff --quiet HEAD` true. The test binary left in this reviewer's target dir is the mutant build, so do not reuse it. |
| `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings` | exit 0, 25.07 s |
| `cargo fmt --all -- --check` | exit 0 |
| `git diff --check df0d42e 6eb5ad8` | exit 0 |

Not run by this reviewer: workspace-wide Clippy, workspace tests, nextest, and hosted CI. The root's own
post-fix runs (focused `--test-threads=4`, full 117 cluster, workspace Clippy, fmt) were still running
when this recheck started. I have not observed their results and claim nothing from them. Reused: the
prior `34bf2d6` integration review for production correctness. Its Source and Security verdicts still
hold because production is unchanged.

## Publication prerequisites (unchanged)

These are still required:
- A new exact-head publication review after the push.
- Fresh hosted gates on the reviewed head (finite attempt 2, not yet pushed, no manual reruns).
- Latest exact-main CI green before merge.

Keep 5 s, 1500 ms, 8 permits and all deadline assertions, and do not relax U6. Neither these local
passes nor a green attempt 2 show that the natural hosted U6 flake is fixed. M2 stays open (A7 NO-GO,
A12 consumed-uncertain, no retry). Hardware paths are **unverified on real hardware**.
