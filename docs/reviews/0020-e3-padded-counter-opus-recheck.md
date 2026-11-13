# E3 native harness padded-counter correction: narrow Opus recheck

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
This is a single-agent narrow recheck with no subagents. It covers only the Sol medium
correction `729922a` and its evidence commit `c27a053`, against the prior recheck
`302175a` (retained as `64264ae`, `docs/reviews/0020-e3-native-output-opus-recheck.md`).

**Verdict: PASS for the narrow B1 delta.** Residual B1 is closed. B2 stays closed and is
carried forward. This verdict allows only a coordinator push of the reviewed head
`c27a053` (plus this report-only commit) to collect hosted evidence. It does not
clear merge or activation. **Every native kill remains UNRUN.**

## Identity and scope

- Workspace `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-e92004d3-d06b9b0aa80eee71`.
  This is the top level of an isolated worktree outside canonical `D:/Code/Arxcovo`.
  Branch `t3/e92004d3`. HEAD `c27a053a8a10eac64e57b2886d661fa9c5126848`. Status was
  clean at the start.
- `git diff 64264ae HEAD` touches only `driver.py` (one line), `test_driver.py`
  (+7/-2), PROGRESS (+1) and the evidence doc (+15).
- `git diff 231e6d7 HEAD -- crates` is empty. Against `231e6d7`, the workflow differs only
  by the single `CARGO_INCREMENTAL` line that the prior recheck already reviewed.
  `mutations.jsonl` is unchanged.
- Pins: `mutations.jsonl` has SHA-256 `c8248205…ca3d93`, which equals `MANIFEST_SHA`.
  `SOURCE_SHA` is unchanged and the product source is unchanged. The workflow hash is
  still `7c7850d5…`.
- Refreshed evidence hashes: `driver.py` `2916049a…89c7` and `test_driver.py`
  `2f3fba56…71`. Each matches both the committed blob and the working tree.

## Exact delta

- `driver.py:161`: `(?:\(\d+/\d+\)\s+)?` becomes `(?:\(\s*\d+/\d+\)\s+)?`. The `^…$`
  multiline anchors, the `re.escape(name)` exact-name match, and the single-match
  requirements in `classify` and `baseline_result` are unchanged.
- `test_driver.py` `baseline()` now emits the real nextest format for all ten native
  tests: `        PASS [   0.028s] ({n:2d}/10) arxd <name>`, which gives `( 1/10)` …
  `(10/10)`.
- A verbatim padded line, `PASS [   0.028s] ( 1/11) arxd tests::t3`, is added to the
  real 0.9.151 excerpt test. That test asserts `survived` and `statuses == ["PASS"]`.
- The `baseline_missing_test` negative control changed from deleting a
  `status("PASS", …)` line to renaming `TESTS[1]` to `TESTS[1]_suffix`. This change was
  necessary. The old helper uses `(1/1)`, so after the fixture change the replace would
  match nothing and the control would test nothing. The new form is stronger: the line
  is still present, but the exact-name anchor rejects it. No test name in `TESTS` is a
  substring of another (checked: 0 collisions), so the rename touches only one line.

## Checks run by this reviewer

| Check | Result |
| --- | --- |
| `python -I -B -m unittest discover -s tools/ci/e3-native -p test_driver.py` (Windows, HEAD) | 25 run, OK, 1 skipped (Windows symlink). **Reproduced** |
| Unpadded-only regex mutant (HEAD driver with `\(\d+/\d+\)`) on an owned scratch copy, with HEAD tests | exit 1: 6 failures + 1 error. Failures include `test_real_nextest_0_9_151_status_excerpts`, the baseline-markers test and every `execute()` scenario that uses `baseline()`. **Reproduced** |
| `78f1f3d` driver with HEAD tests (counter-strict control) | Byte-identical to the mutant; same exit 1, 6F + 1E |
| HEAD driver with the `64264ae` tests | OK. The relaxed regex still accepts unpadded counters |
| Direct probe of the HEAD driver | A padded 10-test baseline is accepted, with L7/L8 `passed`. The `_suffix` rename, `(2 /10)` and `( 2/ 10)` each give `baseline_missing_test`. Padded `( 3/10)` FAIL plus one footer at exit 100 gives `killed`. A duplicated footer gives `invalid` |

The format `{n:2d}/10` matches the right-padded counter that the prior recheck observed
from real cargo-nextest 0.9.151 (`( 1/11)` … `(10/11)`). No new real-output probe was
run, and no project Cargo command, mutation driver, native harness, workflow,
fmt/L6 scanner or full suite was run.

## Security / Standards axis

- **B1 closed.** The baseline for the real 10-test layout is now reachable, and a
  counter-strict parser fails both the baseline fixtures and the verbatim padded excerpt.
- **No false-credit path was introduced.** `\s*` widens only the counter interior. A
  kill still requires exit 100, exactly one exact-name `FAIL`, exactly
  `["error: test run failed"]` as the error lines, and no `SKIP`. Because `\s` matches
  newlines, a counter could in theory span lines. The pre-existing leading `^\s*` and
  the `\s+` after the bracket already had this property, so it is not a new risk class.
  The other kill conditions still bound it.
- B2 is closed and carried forward unchanged from `302175a`. The prior advisories (A2
  incremental timing unmeasured, A3 reap-timeout labelling, the reader-alive label, and
  the split `SKIP` markers) still stand as non-blocking.

No blockers.

## Spec axis (prior recheck's required items)

| Item | Status |
| --- | --- |
| 1. `\(\s*\d+/\d+\)` with the exact-name anchor and single-match requirement kept | Met |
| 1. Discriminating padded baseline and verbatim padded fixture; fails at `78f1f3d` | Met (reproduced) |
| 1. Portable suite and negative controls rerun | Met (25/24/1; mutant exit 1) |
| 2. `driver.py`/`test_driver.py` evidence hashes refreshed; source and manifest pins unaffected | Met |
| No production, workflow or manifest change in this delta | Met |
| Evidence doc accuracy | Accurate: it claims no native execution and no kill credit. Wording nit: "recheck302175a" and "Portable25 discovered:24 passed,one" are missing spaces. Cosmetic |
| PROGRESS line | Accurate for the pre-review state |

## Allowed next step

The coordinator may push the reviewed head for hosted evidence, with green required checks
and the native workflow observed as usual. This review does not allow merge, activation,
reruns, settings changes or any native-kill claim. Hosted results need their own
review against the fail-closed labels.

## Provider credits and status

- Correction `729922a`: OpenAI gpt-6.1-sol medium. Evidence `c27a053`: coordinator.
  This recheck: Opus 5.5 high, review only.
- M2 remains incomplete. Native, installed and dedicated paths are **unverified on real
  hardware**. The causes of the privacy flake, `E_PROTECTION` and `ExecutionFailed`
  remain unknown, with no retry or diagnosis. No GitHub push, CI dispatch, rerun, PR
  watch, merge, settings, owner infrastructure, real proc/sys/cgroup, root, account,
  service, VM, private-state, experiment, ignored-fixture, toolchain-install or
  full-suite operation was performed. Mutant and control copies live only in the
  session scratch directory.
