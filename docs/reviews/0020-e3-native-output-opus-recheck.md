# E3 native harness output-parser correction: narrow Opus recheck

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
This is a single-agent narrow recheck with no subagents. It covers only the Sol medium
correction `78f1f3d` and its evidence commit `5113a3b`, against the prior review
`59525d6` (retained as `fe5fc99`, `docs/reviews/0020-e3-native-harness-opus-review.md`).

**Verdict: CHANGES REQUIRED before push or hosted execution.** B2 is closed. B1 is
**not closed**: real cargo-nextest 0.9.151 right-pads the progress counter when the
total has two or more digits (`( 1/10)`), and the corrected regex accepts only
`\(\d+/\d+\)`. The 10-test baseline therefore still raises `baseline_missing_test` on
every hosted run. The parser still fails closed, so it awards no false credit. But a
hosted run would again spend a run for one `failed_closed` line. The prior review's own
suggested regex had the same gap, because its probe saw only single-digit totals. This
recheck owns that miss.

## Identity and scope

- Workspace `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-bfd18e9e-d459f3981ec74a04`.
  This is the top level of an isolated worktree outside canonical `D:/Code/Arxcovo`.
  Branch `t3/bfd18e9e`. HEAD `5113a3bac8ab55e2990e0c773c279bd5093b70bc`. Status was
  clean at the start.
- `git range-diff` shows `59525d6` = `fe5fc99`. `git diff fe5fc99 HEAD` touches only
  the workflow (+1 line), `driver.py`, `test_driver.py`, PROGRESS and the evidence doc.
  `git diff 231e6d7 HEAD -- crates Cargo.toml Cargo.lock .github` shows only the
  workflow line. No product, native-fixture, manifest or other harness file changed.
- Pins: the HEAD `linux.rs` has SHA-256 `e5b6ac15…54eaff`. That equals the `231e6d7` blob
  and `SOURCE_SHA`. `mutations.jsonl` is `c8248205…ca3d93`, which equals `MANIFEST_SHA`.
  The evidence-table hashes for the workflow (`7c7850d5…`), `driver.py` (`e3597232…`)
  and `test_driver.py` (`8b3bb14a…`) match both the committed blobs and the working tree.

## Checks run by this reviewer

| Check | Result |
| --- | --- |
| `python -I -B -m unittest discover -s tools/ci/e3-native -p test_driver.py` (Windows, HEAD) | 25 run, OK, 1 skipped (Windows symlink) |
| Negative controls on owned scratch copies of HEAD `test_driver.py` | `fe5fc99` driver: exit 1, 7 failures + 1 error, including the new real-excerpt test. Counter group removed: exit 1, the same 8. Footer check removed: exit 1, 4 subtest failures in the real-excerpt test. **Reproduced.** |
| Padding control: HEAD driver with `\(\s*\d+/\d+\)` | Suite still OK. **No test discriminates a padded counter.** |
| Real output probe: cargo-nextest 0.9.151 / rustc 1.95.0 already installed, throwaway 11-test crate named `arxd` in the session scratch directory, the driver's exact flags, **on Windows** | Baseline (all 11) exit 0. Single named FAIL exit 100. Single PASS exit 0. No match exit 4. Compile error exit 101. Full captured text fed to the HEAD driver's `statuses`/`classify`/`error_lines` |

Probe results against the HEAD driver:

- Baseline lines are verbatim `PASS [   0.028s] ( 1/11) arxd tests::t3` … `(10/11)`.
  `statuses()` found only the two-digit-counter tests (t1, t9) and returned `[]` for the
  other 9. With the 10 real E3 tests (10 `#[test]` in `linux.rs`, total 10), entries
  `( 1/10)` through `( 9/10)` would all miss, so the result is `baseline_missing_test`.
  With `\(\s*\d+/\d+\)`, all 11 return `["PASS"]`.
- The full real FAIL output (Starting `(10 tests skipped)`, `stdout`/`stderr` blocks,
  panic text, `Summary`, footer) gives `killed`, and `error_lines` is exactly
  `["error: test run failed"]`. The full single PASS output gives `survived`. Lowercase
  `skipped` never triggers the uppercase `SKIP` guard.
- No tests (exit 4, `error: no tests to run`) gives `invalid`. The compile failure
  (exit 101, `error[E0308]`, `error: could not compile`, and on Windows an extra
  `error: command … exited with code 101` that is never emitted) gives `invalid`, and
  stays `invalid` even when relabelled as exit 100 with a named FAIL appended.
- The indented `SKIP L7: user xattrs refused` stderr marker appears in the baseline,
  and the strip-based allow-list accepts it.

Counter formatting is not platform-specific nextest behaviour. Even so, the probe ran
on Windows and is **not a Linux run**. Not run: any project Cargo command, the
mutation driver, the native harness, the workflow, the fmt/L6 scanner, and the full
suite. **Every native kill remains UNRUN.**

## Security / Standards axis

### Blocker

**B1 (residual). The counter regex rejects the padded nextest counter.**
`driver.py:161` `(?:\(\d+/\d+\)\s+)?` does not accept `( 1/10)`. *Fix:*
`(?:\(\s*\d+/\d+\)\s+)?`, keeping the exact-name anchor and the single-match
requirement. *Tests:* add the verbatim padded line `PASS [   0.028s] ( 1/11) arxd tests::t3`
as a fixture, and make the synthetic `baseline()` emit realistic padded counters
(`( 1/10)` … `(10/10)`), so that a counter-strict parser fails the baseline tests. The
new test must fail against HEAD `78f1f3d` and pass after the fix. Optionally, add the
full real FAIL/PASS outputs (paths redacted) as constants.

### B2: closed

`error_lines()` matches only stripped lines with the exact prefix `error:` or `error[`.
`classify()` returns `killed` only when all four hold: exit 100, exactly one named
`FAIL`, `errors == ["error: test run failed"]`, and no `SKIP` anywhere. `survived`
requires exit 0, exactly one named `PASS` and zero error lines. `baseline_result()`
uses the same classifier and rejects any error line. Missing, duplicate and
compiler-error cases are tested and fail closed.

Truth table: a kill requires an exact-filter run (one test) to exit 100, which nextest
reserves for test failure. Compile errors (101), no tests (4), timeout, signal or leak
statuses (not `FAIL`), a non-matching name, an extra footer or any other error line all
give `invalid`. **No false-kill path was found.** A test whose panic text began a line
with `error:` would give `invalid`, which fails closed.

### Advisories, checked for safety

- **`CARGO_INCREMENTAL: '1'`** is job-level, with the same 10-minute job and 180/40/60 s
  deadlines and no prebuild. It changes only rustc's query cache. Rustc incremental
  fingerprints source content. Cargo freshness compares source mtime with the
  dep-info written at build start, and every mutate or restore write follows at least
  one test run, so a stale binary is not expected. An incremental ICE would be exit 101,
  which gives `invalid`. The claim that `dtolnay/rust-toolchain@683b588` respects a
  preset `CARGO_INCREMENTAL` is **unverified at the pinned SHA**. If that action
  overrides it, the only effect is a slower build. Cold-build timing is still
  unmeasured, and the evidence doc says so.
- **Final JSON reason:** every `GuardError` is built from fixed literals (line 156
  combines literals only), so `reason=str(error)` emits only fixed categories.
  `OSError`/`ValueError`/`TimeoutExpired` map to the fixed labels `os_error`,
  `value_error` and `reap_timeout`, and no exception text is emitted. Safe.
- **A3 scope:** `TimeoutExpired` is now caught only at top level. A reap timeout during
  a mutant still restores source in `mutated()` and in `execute()`'s `finally`, and runs
  `baseline_after`, but it emits no per-row label. A stuck leader is not reaped. The
  evidence doc claims only fixed categories, not a process-reap fix, which is accurate.
  Optional: map the error to `GuardError("reap_failed")` inside `run_bounded`.
- Minor, pre-existing: a reader still alive after `join(timeout=5)` is labelled
  `output_bound` rather than `capture_failed` (`driver.py:155`). Fail-closed and cosmetic.
- An uncaught exception type outside the three listed would still give a traceback with
  a nonzero exit. With a pinned manifest, none is reachable in practice.

Smell: the shared `error_lines` partly resolves the prior duplicated-marker note. The
`SKIP` markers remain split between `classify` and `baseline_result`. Non-blocking.

## Spec axis (prior review's required items)

| Item | Status |
| --- | --- |
| 1. B1 counter-tolerant exact regex | **Not met**: padded counters rejected; the baseline is unreachable |
| 1. B2 line classifier, exactly one footer, other error lines rejected | Met |
| 2. Real 0.9.151 fixtures, portable suite rerun | Partially met: single-digit excerpts only; 25/24/1 reproduced; negative controls reproduced |
| 3. A1 categories | Met (fixed labels) |
| 3. A2 incremental | Partly applied (env only, no prebuild); timing unmeasured |
| No other harness, product or native-fixture change; source pin is `231e6d7` | Met |
| Evidence doc accuracy | Accurate as written, except "B1 … accepts the optional numeric counter", which does not cover padded counters. Update after the fix |

## Required before push or hosted execution

1. Fix residual B1 with `\(\s*\d+/\d+\)` and add a discriminating padded-counter
   baseline test, as above. Rerun the portable suite and the negative controls,
   including HEAD `78f1f3d` as a counter-strict control.
2. Refresh the evidence hashes for `driver.py` and `test_driver.py`. `SOURCE_SHA` and
   `MANIFEST_SHA` are unaffected.
3. Get another narrow different-provider recheck of that delta. **This review allows no
   push now.** Even after a passing recheck, it would at most allow a coordinator push
   for hosted evidence, never merge clearance or activation.

## Provider credits and status

- Correction `78f1f3d`: OpenAI gpt-6.1-sol medium. Evidence `5113a3b`: coordinator.
  This recheck: Opus 5.5 high, review only.
- M2 remains incomplete. Native, installed and dedicated paths are **unverified on real
  hardware**. The causes of the privacy flake, `E_PROTECTION` and `ExecutionFailed`
  remain unknown, with no retry or diagnosis. No GitHub push, CI dispatch, rerun, PR
  watch, merge, settings, owner infrastructure, real proc/sys/cgroup, account, service,
  VM, private-state, experiment, ignored-fixture, toolchain-install or full-suite
  operation was performed. Probe and control files live only in the session scratch
  directory.
