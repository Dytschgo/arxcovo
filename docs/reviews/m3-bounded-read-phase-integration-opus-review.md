# M3 bounded-read / shutdown-phase integration: independent Opus source review

2026-11-16. Opus 5.5 high, narrow source-integration review sub-agent.

- Target: merge `34bf2d62239cce4d0d61b66296c8dd4781e8b88c` on `work/m3-certificate-issuance-boundary`
  (PR149 A0 bounded fresh reads), parents `e2373a4` (A0 branch) and `01a42d5` (main, PR150).
- Scope: the merge resolution only. A0 read path, A0 R1-R5/T2 assertions and the PR150 shutdown report
  were reviewed earlier (A0 full review and recheck, PR150 phase-report review and recheck). They are
  re-checked here only where the merge touches them.
- No push, PR status change, merge, watch, child agent, VM, admin or owner-data operation. This file is
  the only commit.

## Verdict

| Axis | Verdict |
| --- | --- |
| Source (production `consensus.rs`) | PASS |
| Spec (A0 brief, Sol dependency split, Opus diagnostic disposition) | PASS. Brief and evidence text is stale (F2). |
| Security | PASS |
| Standards (tests and local reproducibility) | **CHANGES REQUESTED**: F1, a test-only problem the merge introduced |

**Overall: CHANGES REQUESTED, test-only and narrow.** The production integration is correct and
truthful. Before the exact-head publication review, fix F1, then correct the brief and evidence text (F2).
F1 does not affect the hosted `cargo nextest` gate (see below), so it does not use up or endanger
hosted attempt 2. It does make the documented local `cargo test` evidence commands nondeterministic.

## Merge structure verified

- Both parents are kept. Main `6e9edd5` and PR149's old head `8eaed30` are ancestors of `34bf2d6`.
  PR150 reviewed `3bb81e9` and the squash `01a42d5` have the same tree `41af163`.
- The diff against main covers exactly 9 paths: `consensus.rs`, `consensus/three_node.rs`, PROGRESS, the A0
  brief, the A0 evidence, the U6 diagnosis note, the A0 full review and recheck, and the Sol brief review. No
  Cargo, lockfile or workflow change. No new dependency.
- `consensus/adapter.rs` is byte-identical to main. Main's synchronous `APPLY_PAUSE` runs inside the
  apply blocking closure before the store lock. A0's async duplicate hook is removed.
- PROGRESS keeps both histories: main's PR150 diagnostic entries and A0's entries, plus the PR150
  merge checkpoint. Its last-merged line (PR150, `01a42d5`, trees equal, exact main `6e9edd5`
  CI43777637580) matches git. Current work says the natural hosted U6 flake is "unresolved, not fixed".
  Cosmetic only: the merge added double blank lines between the A0 history entries.

## Source findings (production): none

- `read_held = count(READ_PERMITS - available)` is captured in the owner-timeout branch before
  `abort_all` (next to `owners_left` and `propose_held`), and in the normal branch after the drain, as
  main does for `propose_held`. `shutdown(&mut self)` is exclusive, so no concurrent `fresh_read` can
  hold a permit between `try_acquire_owned` and the `closed` check. The value counts only owner-held
  units.
- The count cap went from 64 to `64 + READ_PERMITS` = 72 for every count field. 72 still fits in `u8`, the
  report is still `Copy`, and no other field or meaning changed. On the A0 base, 64 under-reported
  possible owners (64 proposals + 8 reads).
- Ownership and release did not change: deadline 1500 ms, eight permits, the permit moves into the
  non-abortable blocking closure and drops after the store Arc and before completion, the shutdown
  deadline is 5 s, abort and join run, weak polling runs, and `Orphaned` is returned on failure. The
  comment "Captured before abort, alongside proposal units" is accurate.
- Security: the report has only enums and bounded integers, with no paths, payloads or secrets. No new
  input surface, and no unsafe code.

## F1 (Medium, test-only, introduced by the merge): shared single-slot `APPLY_PAUSE` race between R1 and R6

`adapter::APPLY_PAUSE` is one `Mutex<Option<ApplyPause>>` slot. Before the merge, A0's R1 was the only
user of A0's hook and main's `r6_shutdown_report_cancelled_proposal` was the only user of main's hook.
After the merge both tests do `*APPLY_PAUSE.lock().unwrap() = Some(..)` on the same static. With the
in-process libtest harness (`cargo test`), the test that sets the slot second replaces and drops the
other test's `ApplyPause`. That test's `entered` receiver then fails at `three_node.rs:777`
(R1) or `three_node.rs:1098` (R6) with `RecvError`. The store-pointer key does not help, because
the write replaces the whole slot.

Runs observed against the `34bf2d6` test binary, all locally on Windows:

| Invocation | Result |
| --- | --- |
| R1 and R6-cancelled-proposal together, default threads | **8/8 FAILED**, failing test alternates, `RecvError` |
| `three_node::r` filter (R1-R6), default threads | **3/3 FAILED**, 7 passed / 1 failed each |
| Each test alone; both with `--test-threads=1` | pass |
| `cluster::` full filter, default threads | 117 passed, 0 failed, 2 ignored |
| `cluster::` with `--test-threads=2` ×3 and `--test-threads=4` ×3 | all pass, 117/0/2 |
| `three_node::` with `--test-threads=4` ×3 | all pass, 13/13 |

Wider filters pass only because they spread out when the tests start. Any timing shift can line the two
hook windows up again.

**Hosted exposure:** `ci.yml` runs `cargo nextest run --workspace --locked` with no `.config/nextest.toml`.
That runs each test in its own process, so the static is not shared and this race cannot fail hosted
attempt 2. It is not a production defect, has nothing to do with the U6 mechanism, and is not a cause
of, or fix for, the natural U6 flake.

**Required:** make the tests independent without changing deadline, permit or assertion values and
without relaxing U6. The smallest change keeps `adapter.rs` identical to main: in `three_node.rs`, add a
test-only `static APPLY_PAUSE_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());`.
R1 and R6-cancelled-proposal each hold its guard from installing the hook until it is released. Use
tokio's mutex: holding a `std` guard across `.await` trips `clippy::await_holding_lock` under
`-D warnings`. A keyed multi-slot hook in `adapter.rs` would also work, but it changes the "exact main"
adapter decision and needs its own justification. The brief's future guard test (brief line 179, which
waits on `APPLY_PAUSE`) would be a third user, so the serialization has to cover it as well. To verify
the fix, the paired run and the `three_node::r` filter should pass repeatedly.

## F2 (Low, docs): A0 brief and evidence are stale after the integration

- Brief line 127 still says "`#[cfg(test)]` async `APPLY_PAUSE`". It is now main's synchronous hook
  inside the blocking closure, with a `std::sync::mpsc` release.
- `docs/evidence/m3-bounded-fresh-read.md` does not record the integration: the 72 cap, the actual
  `read_held` snapshot, the gated R5 attribution assertions, the R1 release-channel change and the
  117-test count. Main's `docs/evidence/m3-shutdown-phase-report.md` says "counts saturate at 64" and
  "read_held is logically zero". That stays correct as a PR150 record, but this branch now supersedes it.
  Add a dated addendum to the A0 evidence rather than rewriting PR150 history.

## Informational (no change requested)

- I1. R5 gated cannot tell a before-abort `read_held` capture from an after-abort one. At the 5 s owner
  timeout the only held read state that can occur is the non-abortable blocking closure. Abort does not
  release that permit, so both captures give 1. The code is still truthful. The before-abort placement
  matters only for an async-stage read unit, and the 1500 ms read deadline plus the engine join make
  that state unreachable at 5 s.
- I2. The 72 cap is a saturating lower bound. Briefly, between a closure dropping its permit and the
  owner task finishing, an unreaped read owner can coexist with a new permit holder. So `owners_*`
  can in principle exceed 72 and read as 72. This is a bounded diagnostic and acceptable.
- I3. R5 gated adds `log_strong == 0`. It holds because the read owner keeps only the state-store clone,
  and the engine's own tasks release the log store within the 5 s owner window. That window has already
  closed when the counts are taken, because the store-release wait gets no extra time. R6-proposal shows
  `log_strong = 1` when the SM worker itself is paused, which is why main does not assert it there. Every
  local run here observed 0. If hosted attempt 2 fails on this line, record it as an assertion
  failure. It is not U6 evidence.

## Tests

Actual runs by this reviewer at `34bf2d6`. Unique target `D:/arxcovo-merge-opus-34bf2d6-4085`,
TEMP/TMP `D:/Temp`, `--locked --offline`:

- `cargo test -p arxd --locked --offline --lib -- …three_node::r5_ …three_node::r6_ --nocapture`: 4 passed,
  build 52.96 s, tests 7.06 s.
  - R5 gated: `Orphaned{first: Owners, engine: Joined, owners_waited: 1, owners_left: 1, propose_held: 0,
    read_held: 1, log_strong: 0, sm_strong: 2, total_ms: 5009}`, 5.0098 s. R5 clean: Ok in 1.62 ms.
  - R6 proposal: `Owners/Joined, propose_held: 1, read_held: 0, log_strong: 1, sm_strong: 2,
    total_ms: 5012`.
  - R6 store: `StoreRelease/Joined, sm_strong: 1, log_strong: 0, total_ms: 5006`.
  - R6 reaped panic: `Owners, owners_done_at_start: 1, owners_panicked: 1`.
- `cargo test -p arxd --locked --offline --lib cluster::`: **117 passed, 0 failed, 2 ignored**, 9.38 s. That
  matches the expected 114 + 3 R6.
- The F1 matrix above.
- `cargo fmt --all -- --check`: exit 0. `git diff --check 01a42d5 34bf2d6`: exit 0.

Not run by this reviewer: workspace Clippy, workspace-wide tests, nextest, and hosted CI. The root's
fresh full cluster, fmt and workspace Clippy run was still in progress when this review started. I
have not observed its result and claim nothing from it. Reused without re-running: the earlier A0
review and recheck, and the PR150 review and recheck, for the parts the merge leaves unchanged.

## Publication prerequisites (unchanged)

Fix F1 and the F2 text. Then a separate exact-head publication review, fresh hosted gates on the
reviewed head (finite attempt 2, no manual reruns), and latest exact-main CI green before merge.
Keep 5 s, 1500 ms, 8 permits and all deadline assertions, and do not relax U6. A green attempt 2 is not
evidence that the natural U6 flake is fixed. M2 stays open (A7 NO-GO, A12 consumed-uncertain, no retry).
Hardware paths are **unverified on real hardware**.
