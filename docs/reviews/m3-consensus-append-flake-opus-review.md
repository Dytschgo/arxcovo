# M3 consensus append flake hotfix: independent Opus review

Reviewer: Anthropic / claude-opus-5-5 / high. Builder: OpenAI / gpt-6.1-sol / high (different provider).
Date: 2026-11-16. Method: single-agent source/security review on separate Standards, Spec and Security axes. No build or test was run by this reviewer; every behavioral conclusion below comes from reading the source plus the read-only CI/git checks listed.

## Pinned target

- Base: `0ab5038d89e86e5f726fbed7e5ef6d76e64fa306` (live `origin/main`; `gh api repos/.../commits/main` returned the same SHA).
- Head: `a5d6fa66647d6de2844c926d48482fbbe10b7915`.
- Commits: `ebdecee` fix(consensus), `b2be4b2` docs(evidence) s9c, `a5d6fa6` docs(progress).
- `git diff --name-only 0ab5038 a5d6fa6` lists exactly five paths: `crates/arxd/src/cluster/consensus/{adapter.rs,tests.rs}`, `docs/evidence/m3-consensus-append-flake.md`, `docs/evidence/linux-s9c-intermittent-diagnosis.md`, `docs/PROGRESS.md`. No registry, Cargo, workflow, M2 native or ADR path. `git diff --check` is clean.

## Identity and transplant checks (read-only, run by reviewer)

- `consensus/tests.rs` blob `168873a8…` and `consensus/adapter.rs` blob `f16f1c25…` are identical at `765df2b` (the failing main commit), `0ab5038` and the builder parent `388573e`, as the evidence states.
- Builder commit `d51d3c0` and head `a5d6fa6` have identical post-fix blobs (`adapter.rs` `fee90a64…`, `tests.rs` `73289a50…`). The source diffs of `d51d3c0` and `ebdecee` are byte-identical, and `m3-consensus-append-flake.md` matches with no diff. The transplant to exact main is clean. Exact-main *execution* is still pending with the coordinator, so this review does not count it.
- CI (`gh run view`): `43757400254` = CI/main/push `38750cf…` failure; `43767515928` = CI/main/push `765df2b…` failure; `43768671593` = CI/main/push `0ab5038…` success. The `43767515928` failed log shows `append_spawned_cancellation_panic_and_poison_complete_once` FAIL in 0.015s, panicked at `consensus/tests.rs:550:59`. The PROGRESS records match.

## Spec

The goal is to fix the red-main append flake without weakening anything, keep the callback eager/exactly-once/commit/poison/cancel contracts, add a regression that really reopens the store while the worker is parked, keep the evidence honest, and record both red main runs plus an exact-main green rule in PROGRESS.

**Mechanism.** `open_db` takes `File::try_lock` on `cluster-log.lock` and maps failure to `StoreError::Io` (`state_store.rs:195-225`). That lock lives until `VoteLogStore` drops. Before the fix, the detached `spawn_blocking` closure owned a strong `Arc` clone (`store`) and called `guard.finish` while still holding it. A cancelled caller that receives the callback, then drops its own handles and calls `disk()`, can therefore run `VoteLogStore::open` while the worker's captured Arc keeps the old lock open. That produces exactly `unwrap()` on `Err(Io)` at `tests.rs:550:59` (inside `disk()`, called from the first block of the failing test). On Linux, std `try_lock` uses `flock`, which belongs to the open file description, so a second in-process open conflicts there too. The deterministic race is therefore a *compatible* explanation for the Ubuntu failure.

**Fix (`adapter.rs:188-197`).** `drop(store)` now runs after the append result is computed and before `guard.finish`. `r` is `Result<_, StoreError>` and does not borrow `store`; the `MutexGuard` is released when the `and_then` closure ends. Contracts:

- *Eager:* the callback is still published inside the blocking worker, before the closure returns. The join future is unchanged.
- *Exactly-once:* `Completion::finish` consumes the guard and `Drop` sees `None`. If dropping the last `Arc` ever panicked (a `VoteLogStore` destructor panic), the guard would still fire `Err` once during unwind.
- *Commit:* `s.append(&b)` completes, including the durable write, before the drop and before the callback. The result mapping is unchanged.
- *Poison / panic:* the injected-panic path panics inside `and_then`, before the new lines, so its behavior is unchanged. That subtest awaits the future, so the join completes after the closure has fully unwound and its `disk()` reopen is unaffected.
- *Cancel:* cancellation still drops only the join future, and the worker completes as before.
- *Last-owner case:* in production, if the worker holds the last `Arc` (node shut down while an append is in flight), the file close/unlock now finishes before the callback, which only tightens the guarantee.

**Regression (`tests.rs` `cancelled_append_callback_releases_store_before_worker_returns`).** It uses the real `LogAdapter::append`, real `VoteLogStore`, real lock file and real reopen through the existing `disk()` helper. The worker is parked by a store-address-keyed `cfg(test)` gate placed *after* `guard.finish`, and the gate stays closed through the reopen assertion. Counterfactual by source: without `drop(store)`, the parked worker holds the only remaining `Arc` after the test drops `ls`/`held`, so the lock stays held and `disk()` must fail with `Io`. That is deterministic and matches the reported 20/20 pre-fix failures. With the fix, the test's own `drop(held)` is the last owner and runs the destructor synchronously before `disk()`. There is no retry, no added sleep, no timeout increase and no weakened assertion. The 2s timeout guards only gate entry, exactly as in the original test. The original test's assertions are byte-unchanged.

**Evidence honesty.** `m3-consensus-append-flake.md` says plainly that the natural flake did not reproduce locally (200/200 before and after on Windows), that the gate reproduces a compatible interleaving, and that it "does not prove no other cause contributed to the Ubuntu CI incident". PROGRESS repeats "no hosted natural-cause claim". `linux-s9c-intermittent-diagnosis.md` records zero Linux executions, zero fix attempts and blocked Phase 1, and gives a bounded handoff. s9c remains unresolved and is not claimed fixed.

**PROGRESS.** Both red main runs are recorded with SHA, run ID, test and line, and both IDs check out. The PR145 merge omission is acknowledged. Latest main `43768671593` is recorded as green on exact `0ab5038`, which matches. The new rule requires the latest CI run for the live main SHA to be green on that exact SHA, re-read immediately before merge and recorded in the merge checkpoint. It says `ci-time-budget` is no substitute and allows no bypass or stacked merge onto red main. The task history adds a 2026-11-16 entry with provider, effort and outcome.

Findings:

1. **S1 (low, process, non-blocking): the merge rule can block its own hotfix.** "a red, pending, cancelled or missing main run blocks the merge … No bypass" has no defined path for a PR whose purpose is to fix the flake that turned main red. Concrete case: the next main push flakes red on this very test before this PR merges. Read literally, the rule forbids merging the fix, while "No test is silently retried" leaves it unclear whether a *recorded* rerun of the failing main job counts as "completed successfully". It does not block this PR today, because main `0ab5038` is green. Recommendation: the coordinator adds one sentence that defines the allowed recovery, such as a recorded, non-silent rerun on the same SHA, or a babysitter-acknowledged hotfix exception. Do not leave it to interpretation.
2. **S2 (low, evidence identity, non-blocking): the evidence doc names a feature-branch parent and leaves exact main implicit.** `m3-consensus-append-flake.md` gives "Starting parent: `388573e`" and says "this parent includes registry tests". The 65-test, fmt and Clippy figures were therefore measured on the registry parent, not on `0ab5038`. The blob identity makes this low risk, and PROGRESS marks exact-main checks pending, but the doc does not say it was transplanted. Recommendation: when the coordinator's exact-main validation lands, record it, with SHA, commands and counts, in PROGRESS or the merge checkpoint, not in place of the existing evidence.

## Standards

The standards checked are AGENTS.md, CONTRIBUTING.md and KICKOFF review/merge rules, plus the smell baseline.

- The commits are conventional and each covers one concern. The change carries a meaningful behavior test. Hardware labels are present ("**unverified on real hardware**"). No secrets or private data appear in the diff; the only paths are a unique `D:\arxcovo-consensus-hotfix-<hex>` build root and repository paths. The checks the builder claims (65 consensus, fmt, Clippy, diff-check) are listed with commands. This reviewer re-ran only `git diff --check`.
- Builder Sol and reviewer Opus are different providers, as required.
- **Smell (judgement): a test hook inside production code.** The append closure now has two `#[cfg(test)]` insertions (`store_id` and `pause_append_completion`) next to the existing `PANIC_APPEND` hook. This follows established precedent in the same function and compiles out of release builds, so it is acceptable. Note it only so a third hook prompts a small test-seam abstraction.
- **Observation (non-blocking): stale hook entries.** `APPEND_COMPLETION_PAUSE` is keyed by `Arc` address. If the test panics before the worker consumes the hook, a stale entry stays in the static, and a later test whose `Arc` reuses that address would take it. Its `send` and `recv` would both return `Err` at once because the channel ends are dropped, so nothing hangs. `PANIC_APPEND` already carries the same risk. No change required.
- No other baseline smell applies. `drop(store)` before `guard.finish` is explicit, carries a comment explaining why, and is the minimal change.

## Security

- No change to persistence format, lock semantics, private-path checks, reparse/symlink validation, MCP surface, networking or authentication. The production delta is a single `drop` that moves the release of one reference earlier. It cannot widen access: it only shortens the time the worker holds the store lock, and it cannot drop a lock early while the caller still uses the store, because the caller holds its own `Arc`.
- The hooks are `cfg(test)` only and use no unsafe code. They cannot be reached in release builds.
- Evidence and logs hold no credentials, owner data or external requests. The s9c investigation's `multipass.exe list` was a bounded read-only inventory that timed out. Only that client process was terminated, and no guest, Hyper-V VM or old Lab Manager resource was touched or started. That stays within AGENTS fences.
- Residual, documented and out of scope: on the **cancelled + panic** path, the store `Arc` and `Completion` are dropped during unwind in closure-capture order, which the language does not specify. Release-before-callback is therefore not guaranteed there. No test or production path reopens after a cancelled panicked append, and the fix claims only the ordinary completion path, so this is not a regression.

## Verdict

**PASS.** The source fix is correct and minimal, and it keeps the eager, exactly-once, commit, poison/panic and cancel contracts. The regression really reopens the store while the worker is parked, through the real lock path, and by source it fails without the fix and passes with it. Nothing is bypassed through retries, timeouts or weakened assertions. The evidence separates the deterministic compatible race from the unproven natural Ubuntu cause. Linux s9c stays open and blocked with zero executions. PROGRESS records both red main runs and the exact-main green merge rule, and the latest main run `43768671593` is confirmed green on exact `0ab5038`.

Merge preconditions this review does not satisfy: coordinator exact-main build/test results, fresh exact-head publication review, all five hosted gates green, and a re-read of live main CI immediately before merge.

Findings: Spec 2 (worst: S1, the merge rule has no defined recovery path when main is red on the flake being fixed, low). Standards 1 judgement smell plus 1 observation (worst: test hooks in the production closure, acceptable by precedent). Security 0 blocking (one documented, out-of-scope residual on the cancelled + panic ordering).
