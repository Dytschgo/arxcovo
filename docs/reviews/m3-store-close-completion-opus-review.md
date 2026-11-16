# Opus review: store-close advisory 1 (destructor-completion signal)

Disposition: **APPROVE source (no blockers); two non-blocking advisories, one nit.** This is not merge permission.
Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-16. Single reviewer, no subagents. Independent
different-provider review of a Sol (gpt-6.1-sol high) build. Standards and Spec axes are reported separately.

## Exact pins

| Item | Identity |
| --- | --- |
| Review checkout HEAD, clean before and after all runs | `5f0c723d7340730d7b609d54ffe45b5cadf2349e` |
| Prepared base | `71607f53caf69f1f6a4582d61fa8e993076b717a` (docs-only descendant of live main) |
| Live main (`origin/main`, merge-base of base) | `6231da98b4aa902669e268be89de86b273d1bd16` |
| Source commits | `474a8c4` (ack, gates, tests, OLD wait), `fa49b72` (one-line wait fix), `5f0c723` (evidence only) |
| Main blobs (match brief pins) | state `90474881230e`, log `02db51fc2409`, consensus `c93c58f624e9`, adapter `181e9dce6765` (adapter unchanged by this change) |
| Reviewed paths (7) | `cluster/{state_store,log_store,consensus}.rs`, their three `tests.rs`, `docs/evidence/m3-store-close-completion.md` |
| Head file SHA-256 | state `e1c9c534…5402`, log `265a10bb…cb60` (equal to the evidence originals) |
| Spec | `docs/briefs/m3-store-close-completion.md`, including the root Sol contract check |

`git diff 474a8c4 fa49b72` is exactly the StoreRelease loop condition. `git diff fa49b72 5f0c723 -- crates` is empty.
`71607f5..5f0c723` changes no Cargo manifest, lockfile or `.github` file and adds no `unsafe`, `ManuallyDrop` or file unlock.
TLS source, adapter, `three_node`, quarantine and `peer_registry` were not reviewed or touched. `peer_registry` does not exist on
this tree, so the source verdict does not depend on PR B's weak-handle design.

## Checks run by this reviewer

Windows local, `CARGO_TARGET_DIR`/`TEMP`/`TMP` under the unique `D:/arxcovo-close-5f0c723-opus-review`, `--locked --offline`,
`-p arxd --lib`, `--test-threads=1`. Logs remain in that directory and are not committed.

| # | Tree | Command filter | Result |
| --- | --- | --- | --- |
| 1 | head | `store_close_completion` | exit 0, 4 passed, 566 filtered, 5.63 s (`head-green.log`) |
| 2 | head with `fa49b72` reverse-applied (OLD Weak wait) | same | **compiled**, exit 101: both shutdown tests FAILED at runtime with `shutdown returned while store destructor held its lock` (tests.rs:907 deadline, :916 waits), both store tests passed, 0.09 s (`old-wait.log`) |
| 3 | `release` moved directly before `gate` in both stores | `--skip shutdown` | compiled, exit 101, both FAILED on `ack preceded field destruction`, 0.02 s. Mutant SHA-256 state `5446e413…5d36`, log `a728743f…80f1` (`misplaced-ack.log`) |
| 4 | `gate` moved directly after `_lock` in both stores, ack still last | `--skip shutdown` | compiled, exit 101, both FAILED on `gate must hold the real file lock`, 0.02 s. Mutant SHA-256 state `491910e1…dcc9`, log `269c8242…b0a` (`gate-after-lock.log`) |
| 5 | wait condition reduced to `!self.sm_released.is_set()` (extra mutant) | same as 1 | exit 0, **4 passed**, so the mutant survives (see Spec P1) (`log-wait-dropped.log`) |
| 6 | restored head | same as 1 | exit 0, 4 passed, 5.61 s (`restored-green.log`) |
| 7 | head | `cargo fmt --all -- --check`; `git diff --check 71607f5 5f0c723` | both exit 0 |

After each mutant, `git checkout --` restored the files. The SHA-256 values matched the originals above (consensus blob `5b83422`),
and `git status` was clean. My mutant hashes differ from Sol's because my edits also removed the "must stay last" comment line. The
behavior under test is the same.

**Reused, not rerun:** workspace Clippy `-D warnings` (Sol, evidence step 8, exit 0). Reuse is justified because the crate tree at
`5f0c723` is byte-identical to the restored `fa49b72` tree Sol linted. **Not run:** full cluster, `three_node`, D1/quarantined
tests, integration binaries, CI, native M2, VM or hardware. No full-suite claim.

## Contract checklist (source)

- **Last-field ack.** `release: ReleaseAck` is declared last in both stores, after `_lock` and the cfg(test) `hook`, with a "Must
  stay last" comment. The test-only `gate` sits immediately before `_lock`, after `db`. Runs 3 and 4 kill both placement mutants.
- **Atomics.** `ReleaseAck::drop` stores `true` with `Release`. `Released::is_set` loads with `Acquire`. Each flag is one fresh
  `Arc<AtomicBool>` per store. This is sufficient for preceding field destructors to happen-before shutdown observing `true`.
- **No ack clone or early drop.** `ReleaseAck` is not `Clone`, and its tuple field is private to `state_store`. It is created only
  inside each `open`'s final `Ok(Self { .. })` literal, after every fallible step, so no ack drops on an open-error path. `Released`
  (Clone) is observe-only. `ReleaseAck::drop` cannot fire before its owner's destructor runs.
- **Open, error, unwind and poison honesty.** Earlier `?` paths drop `lock`/`db` locals synchronously. Drop glue still drops later
  fields after a panicking field destructor, so the doc comment's "completion of preceding field destructors" covers unwinding.
  A poisoned `Mutex<T>` still drops `T`. The `Engine::new`/`Fatal` path makes no release claim. This matches the brief.
- **Token and store identity.** There is no `Default` for `Released`/`ReleaseAck` and no shared or always-set token.
  `log_released = ls.released()` and `sm_released = sm.released()` are captured in `start` (the sole `ConsensusNode` literal) before the
  `Arc::new` wrapping and the `Weak` downgrade. No production caller of `ConsensusNode::start` exists outside tests/three_node.
- **Shutdown otherwise unchanged.** It keeps the same `timeout_at(deadline, …)`, the 5 s deadline and the `yield_now` loop. `log_strong`/`sm_strong` still come from
  `Weak::strong_count` after the wait. Phase order, owner abort/capture-before-abort and `ShutdownReport` shape are unchanged.
- **Scope honesty.** The evidence states that the flag means only that preceding destructors finished. It does not mean SQLite close or OS unlock succeeded, and
  immediate real reopen stays a separate checked result. No retry, file unlock, `unsafe`, new dependency or production restart caller
  was added. The scenario 17 retry is unchanged.
- **Test placement.** All four tests are ordinary all-OS `--lib` unit tests outside `three_node` and the quarantine. The node tests use
  `mem_net` single-node.

## Standards

No documented-standard violation found. The commits are small and conventional, and each behavior has a runtime test. No unsafe code or new
dependency was added. Fmt is clean, and Clippy is reused as above.

- **S1 (nit, possible Speculative Generality / Mysterious Name).** `release_pair()` returns `(ReleaseAck, Released)`, but both call
  sites take `release_pair().0` and discard the `Released`. Callers get a `Released` through the separately added `ReleaseAck::released()`.
  There are two ways to obtain the observer, and the pair's second half is unused. A `ReleaseAck::new()` would be clearer. The brief
  named `release_pair`, so this is judgement only. Non-blocking.
- **S2 (info).** `474a8c4` is intentionally red and carries unused-field warnings, per the brief's sequencing. Bisects that land on it
  will see the controls fail. Merging by squash or keeping the commits is the coordinator's choice.

## Spec

All brief requirements checked above are implemented as written. The runtime controls behave as the root clarification requires:
compiled OLD wait gives 2 shutdown red and 2 store passes, the fix gives 4 green, and both placement mutants fail with their intended assertions.

- **P1 (advisory, test gap).** The node-level controls gate only the `StateStore`, as brief regressions 2 and 3 specify. As a
  result, deleting the log half of the `&&` in the wait condition survives all four tests (run 5). The store-level log test proves the
  log ack seam, but no runtime test proves that `shutdown` waits for the log destructor. This is spec-conformant and non-blocking. I
  recommend a log-gated variant of the shutdown control before the first production restart caller.
- **P2 (advisory, timing).** The deadline variant asserts `start.elapsed() < 6s`, which leaves about 1 s of slack after the 5 s
  deadline. This bound is not in the brief. It holds locally (5.6 s total for the whole filter). It is a plausible source of hosted-runner
  timing noise, so watch it in hosted runs before treating a failure there as a product regression.

## Blockers, unproven limits, reversibility

- **Blockers for this source review:** none.
- **Remaining merge gates, not satisfied here:** root full cluster verification, queued after the parallel TLS round. Also required:
  all five required gates on the exact PR head, independent review bound to that exact head, and a green main.
- **Unproven:** SQLite `sqlite3_close` success and Windows `LockFileEx` release timing after the ack. Immediate reopen was observed
  only in these local runs. Hosted Windows and real hardware are **unverified on real hardware**, and hosted Windows three-node is unverified.
  This is advisory 1 source correctness. It is not a Windows recovery attempt and makes no historical-flake fix claim. M2 stays open, M3 unchecked,
  and the eight Windows three-node ignores are unchanged.
- **Reversibility: two-way door.** The production change is two captured fields and one loop condition. Hooks are `#[cfg(test)]`,
  and there is no persisted-format, dependency or API change. Reverting `fa49b72` restores the old wait. Reverting both source commits removes the
  seam.

No source, PROGRESS, ADR, brief or evidence edit. No cache cleanup, VM/admin, native M2, CI, push, PR, status, merge or watch.
