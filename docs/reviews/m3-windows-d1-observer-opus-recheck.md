# Opus independent recheck: Sol D1 observer-join correction

Target: Sol correction `f426eb5` (`test(consensus): join diagnostic observers and surface panics`) on `work/m3-post-quarantine-checkpoint`, parent `8dd128b`. Opus 5.5 high wrote D1 (`6b70689`/`01a2e0d`/`fb84986`, retained as `73cae0c`/`97430a5`/`8dd128b`). This review covers only the Sol correction. It does not count as independent credit for Opus's own D1 work; that credit comes from the root Sol review in `docs/reviews/m3-windows-d1-sol-review.md`.

Scope verified: `git diff --name-only 8dd128b..f426eb5` lists only `crates/arxd/src/cluster/consensus/diagnostics.rs` and `docs/reviews/m3-windows-d1-sol-review.md`. The correction does not change production code, shared-helper outcomes or deadlines, retries, quarantine annotations, workflows or dependencies.

## Standards: PASS

- The diff is small and does one thing: it removes `tasks.retain(|t| !t.is_finished())` from `watch()`, and `close()` now records whether any `JoinError` is a panic.
- `close()` keeps its documented idempotency. A second call drains an empty vector and does not assert.
- `cargo fmt` and the workspace-wide clippy run with `-D warnings` both pass (actual runs listed below).

## Spec: PASS

- **Every handle reaches a join.** Handles are added in only two places: `start_probe` and `watch`. They are removed in only one place: the `drain(..)` in `close()`. So every observer handle is aborted and awaited during normal teardown. The number of handles is bounded by the number of `start()` calls in a fixture (three at startup plus a bounded number of restarts), so `watch()` no longer needs to prune them.
- **Panics are reported only after cleanup.** `close()` aborts all handles, joins all of them, calls `forget` on all three stores and sets `finished = true` before it asserts. A `Cancelled` error from the deliberate abort is correctly ignored. When a task panicked before the abort, or races with it, tokio reports a panic, and the assertion fires.
- **No hidden successes in normal teardown.** `close()` runs at the end of `Cluster::finish()`, after the fixture's own assertions. An observer panic therefore turns a fixture that would otherwise pass into a failure, and it cannot mask an earlier fixture failure, because that failure has already panicked. When `close()` panics, the following unwinding `Drop` still writes the bounded `[DIAG-3n]` dump once (seen in the focused test output).
- **Failure-path `Drop` is abort-only, and that is the correct design.** `on_drop` aborts but never awaits, takes no blocking lock and does not panic. A panic in an observer is not reported on this path, but the test is already red there, so no success is hidden. The Sol review states that `Drop` cannot await cancellation synchronously and makes no hard-termination claim. That matches the code.
- **New test.** `completed_observer_panic_is_joined_and_reported` (`#[should_panic(expected = "diagnostic observer panicked")]`) adds a task that panics, yields until `is_finished()`, then calls `close()`. This reproduces the dropped panicked observer directly.

## Security: PASS

The change is test-only and adds no new output fields. The assertion message is a fixed string, and the `"observer control"` panic text exists only in the test. No secrets, paths, addresses or store contents are rendered. The opt-in controls still never run M2 fixtures.

## Evidence (actual runs by this reviewer on Windows, from a fresh `CARGO_TARGET_DIR` of my own, `--locked --offline`)

| Command | Result |
|---|---|
| `cargo nextest run -p arxd -E 'test(/^cluster::consensus::/)'` | 69 passed (previously 68, plus the new test). `nextest list` shows 9 `diagnostics::tests` unit tests and 11 ignored tests: the 8 Windows-quarantined tests plus the 3 opt-in controls |
| Focused: `-E 'test(=cluster::consensus::diagnostics::tests::completed_observer_panic_is_joined_and_reported)'` | 1 passed |
| Red counterfactual A: `close()` restored to `let _ = t.await;`, in a separate mutant target directory | compiles; **FAIL**: "test did not panic as expected" |
| Red counterfactual B: only the `assert!` replaced with `let _ = observer_panicked;` | compiles; **FAIL**: "test did not panic as expected" |
| Source restored after the mutants | `diagnostics.rs` sha256 `f2eb65b879992699a1d7d34a2275196132fe074791ec8586e9153018c6716bcb` matches before and after; `git status` clean |
| Controls: `--run-ignored only -E 'test(/^cluster::consensus::diagnostics::controls::/)'` | 3/3 passed (these call `finish()`, so they now go through the joined `close()`) |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` | pass |

I did not repeat the 20-iteration natural loop: the instrumentation is unchanged apart from the join, and the brief excludes that loop.

## Non-blocking notes

1. Removing the `retain` from `watch()` has no runtime regression test, because one would need a live `Engine` to restart. The change is verified by inspection only: the drain in `close()` is the only place handles are removed. Counterfactual A covers the `close()` half.
2. In the Sol review doc, many words are run together (for example "Opus6b70689", "the20-run"). This hurts readability, not correctness. Its sentence "Coordinator correction verification remains pending" is superseded for this reviewer's runs by the table above. Root coordinator verification still needs its own record.

## Verdict

**PASS.** I found no blocker in the Sol correction. Publishing still requires the root Sol review to be closed, this complementary review, and a fresh review plus gates on the exact head. Hosted non-gating samples carry no authority in D1. Main `6f6e9d2` CI run `43809711957` was confirmed green by the coordinator; I did not re-check it. Windows three-node remains quarantined: unverified on hosted Windows and **unverified on real hardware**. M3 is unchecked and M2 is open.
