# Opus review: shutdown phase report (`4c909a6`)

2026-11-16. Opus 5.5 high, independent review of Sol's builder commit `39bbbd9..4c909a6`
on `work/m3-shutdown-phase-report`. Base is main `6e9edd5a51d1df41cdb412af6031f959de2d9d60`.
Head is `4c909a625efaf4347355b0096452c786911443ab`. Branch log from the base:
`af34581`, `d1ad3b3`, `ed69859`, `3ed100b`, `4c909a6`.
The Opus design (`af34581`) is a derivative Opus artifact, not an independent review of the design.
Sol's review (`d1ad3b3`, `3ed100b`) is the independent design review. This document reviews
source only. It makes no source change, push, PR, status, merge, watch or child-agent operation.
At most one reviewer is active.

**Verdict: CHANGES REQUESTED. There is one concrete test-only blocker (B1).**
Standards PASS, Spec PARTIAL (B1), Security PASS. The source behavior is correct and truthful.

## Scope checked

- Source diff against the base touches only `consensus.rs`, `consensus/adapter.rs` and
  `consensus/three_node.rs`. Builder commit 4c909a6 adds `docs/evidence/m3-shutdown-phase-report.md`.
  Design, Sol review and PROGRESS come from the earlier docs commits.
- `Cargo.lock`, every `Cargo.toml`, `rust-toolchain.toml` and `.github` are identical to the base.
  No new dependency, `unsafe` code, workflow, ADR, M2/native, VM, admin or owner-data change.
- The U6 body and `Cluster::stop` are unchanged: `three_node.rs` changes are appended after T4,
  and `Cluster::stop` still calls `shutdown().await.unwrap()`.
- The final blobs match the evidence table: `6233249…`, `aca96cd…` and `3e56411…`.
  All 15 evidence-log blob ids resolve locally. A grep of them found no user path,
  drive-letter path or `@`. The evidence correctly says the blobs are local only and not published.

## Source findings (truthfulness)

- `Orphaned(pub ShutdownReport)`: only `Copy` enums and integer fields. Its Debug output prints no
  path, SQL, address, command data or error string. The success path returns `Ok(())` with a stack
  value and no allocation and no global state.
- `first` semantics: `fail()` records only the first phase. A poisoned first lock returns the
  default `Lock`. The second lock goes through `fail(Lock)`, so an earlier Engine failure is kept.
  Engine `JoinFailed`/`Deadline` → `Engine`. A reaped or awaited `JoinError` or the owner timeout
  → `Owners`. Weak-poll timeout → `StoreRelease`. This matches the design.
- The 5 s absolute deadline, the timeout_at engine/owners/weak sequencing, `abort_all` plus
  join-all, `raft.take()`/`sm.take()` and the weak polling are semantically unchanged. The new
  nonblocking `try_join_next` pre-reap keeps the old `failed |= is_err()`, because the old
  `join_next` also returned completed tasks first. Nothing is detached. No forced cleanup is added,
  and Orphaned is not accepted anywhere.
- `owners_left` and `propose_held` are captured on the timeout branch **before** `abort_all` and
  the join loop. `last_owner_join_ms` is updated only by pre-reap and timed joins, not by
  cancellation joins. **Confirmed by mutant A below.**
- `log_strong`, `sm_strong` and `total_ms` are read after `raft.take()`/`sm.take()` and after the
  weak poll, so they count only holders outside this node. The source is correct, but no test pins
  this (see B1).
- Bounds: `count(n) = n.min(64) as u8`, and running counters re-enter through `count(prev+1)`.
  `elapsed` is `min(u16::MAX)`. `64.saturating_sub(available)` cannot underflow because the
  semaphore is never topped up. No overflow is possible. A value of 64 means "≥64"; this saturation
  is documented.
- `read_held` is always 0 on main and has a comment. This follows Sol's dependency split. Actual
  read-semaphore attribution and A0 R5 assertions stay with PR149 after this parent merges.
- In production, `adapter.rs` apply is equivalent: the same clone, the same `blocking`, lock then
  `apply`. `pause_apply` is `#[cfg(test)]`, runs before the state-store lock, and is keyed by the
  live `Arc` pointer. A panic during a test drops the `release` sender, so `recv()` returns and the
  blocking thread cannot leak.
- Tests: the proposal test pauses apply, waits for `entered`, and checks 63 permits. It then aborts
  **and joins** the caller (`is_cancelled`), re-checks 63 permits and one pending owner, and unwraps
  the exclusive node. Shutdown reports Owners/Joined, waited 1, left 1, held 1 and read 0, with
  `total_ms` in 5000..=5500. Only then does the test release the gate, and `report_cleanup` bounds
  real weak-ref and permit cleanup to 2 s. The release happens before any report assertion. The
  store-only test holds one `SmAdapter` clone and gets StoreRelease/Joined with waited 0 and left 0.
  `owners_done_at_start=1` there is the founders-write owner left unreaped, and the test does not
  assert it. The normal leg uses unchanged `Cluster::stop` and gets `Ok`. The reaped-panic leg
  gets Owners with `owners_done_at_start=1`, `owners_panicked=1` and waited 0.
- The evidence admits that `EngineEnd::JoinFailed`/`Deadline` (and `Lock`) are uncovered and does
  not fake them.

## Blocker

**B1. Store-count timing is untested (Spec: "reverting the seam must make those assertions fail
on missing/misattributed diagnostic data").** Mutant B reads `log_strong`/`sm_strong` before
`raft.take()`/`sm.take()`. It compiles and passes all three R6 tests (exit 0). It reports
`sm_strong` 2 instead of 1 (store-only) and 3 instead of 2 (proposal), but both tests assert only
`sm_strong >= 1`. That inflation counts the node's own handle as an external holder. It would
mislead the hosted reading "StoreRelease = detached holder outlived owners". Required minimal
correction: in `r6_shutdown_report_store_only`, replace `assert!(r.sm_strong >= 1);` with
`assert_eq!(r.sm_strong, 1);`, because the test's `held` clone is the only remaining holder after
the 5 s poll. I checked this in a throwaway copy: unchanged source plus the tightened assertion
passed 3/3, and mutant B with the tightened assertion failed `left: 2, right: 1` (exit 101). This
is a test-strength correction of the diagnostic seam, not a behavior fix. It does not count toward
the three-fix rule. It is Sol's second of two seam attempts.

## Non-blocking notes

1. The proposal test's 5.5 s outer bound leaves about 500 ms for abort/join after the deadline on
   hosted runners. Observed `total_ms` was 5003–5011. This is acceptable per design, but it is a
   residual hosted-timing risk, not a deadline increase.
2. `last_owner_join_ms=0` means both "no owner joined" and "joined at 0 ms". Read it with
   `owners_waited`/`owners_left`.
3. Commit `4c909a6` repeats its subject indented in the body. A squash merge rewrites it.
4. Smells (judgement only): the `propose_held` expression is duplicated in both branches, which the
   capture-before-abort timing needs. The literal 64 recurs with `Semaphore::new(64)`. Both are
   left as they are.
5. The builder evidence lists commands with `--offline` and without `--locked`. The lockfile is
   unchanged. My own runs below used `--locked --offline`.

## Standards

PASS. The change follows AGENTS/CONTRIBUTING: one concern, a conventional subject, meaningful
tests, evidence written as actual output, hardware paths unchanged and **unverified on real
hardware**. There is no assertion weakening, deadline increase, broad scope or false claim of a
hosted cause. fmt and Clippy are clean.

## Spec

PARTIAL until B1 is fixed. All other items in the disposition and in Sol's dependency and borrow
corrections are implemented: the report fields as designed, read_held 0 on main, count cap 64, a
minimal cfg(test) `APPLY_PAUSE` before the state-machine lock, the cancelled-caller seam,
Owners/StoreRelease/normal/reaped legs, and U6 and `Cluster::stop` unchanged. The natural U6 cause
stays unknown. Induced state-machine latency is only a compatible mechanism. Behavior fixes used:
0/3.

## Security

PASS. No secrets or private data in source, Debug output or evidence blobs. All diagnostic fields
are bounded integers and enums. The test hook is not compiled into production. No new dependency,
no `unsafe`, no path handling and no MCP surface.

## Actual checks run by this reviewer

The test copy was a fresh `git archive 4c909a6` with a unique D: target and TEMP/TMP. It was
deleted after the runs. Logs are kept as local blobs only, with path prefixes replaced, and are not
published.

| Check | Result | Blob |
| --- | --- | --- |
| `cargo test -p arxd --lib --locked --offline r6_shutdown_report -- --nocapture` | 3 passed | `67a0560b` |
| `cargo test -p arxd --lib --locked --offline cluster::` | 112 passed, 0 failed, 2 ignored; 8.60 s | `a9153009` |
| Mutant A: owners_left/propose_held captured after abort/join | proposal test fails `owners_left` left 0, right 1; exit 101 | `69ed358b` |
| Mutant B: store counts captured before `take()` | **survives**, 3 passed; exit 0 | `eb11f354` |
| Tightened `sm_strong == 1`, unchanged source ×3 | 3/3 pass | `357089bf`, `2f9868e4`, `477e4656` |
| Tightened assertion + mutant B | fails left 2, right 1; exit 101 | `2da19050` |
| Source restored byte-identical to `4c909a6` after the mutants | cmp equal | n/a |
| `cargo fmt --all -- --check` | exit 0 | `0c84f08c` |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | exit 0; 24.79 s | `c3095ba9` |

Not run: the workspace-wide nextest suite, U6 repeats (the builder's 8/8 is sanity only), and any
hosted CI. I did not reproduce the builder's two label mutants; their logs were inspected.

## Next

Sol applies B1 as a test-only commit. Then a fresh independent Opus recheck of that exact head
is required. Publication then needs a separate exact-head review, the five required gates and a
green exact-main run. PR149 stays blocked until this parent merges.
