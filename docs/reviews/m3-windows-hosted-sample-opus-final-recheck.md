# Opus final narrow recheck: Sol R1/R2 correction 89652d5

Reviewer: Anthropic / Claude Opus 5.5 / high, 2026-11-16. Single-agent review, no delegates. **R1 and R2 are closed. No blocking findings.** No source edits, publication, status posting, PR, dispatch, hosted run, merge or watch.

## Pinned scope and independence

- Parent `81a37bf`, which is the retained copy of my recheck `a76c223`. Both have the same parent `aff1d65` and the same tree: `git diff a76c223 81a37bf` is empty. Head `89652d5808e33c72c185a26f4b4ae97aa3ac5fbc`, which is also this checkout's HEAD.
- Reviewed `git diff 81a37bf 89652d5`: four files, `tools/ci/windows_diag_sample.py`, `tools/ci/test_windows_diag_sample.py`, `docs/evidence/m3-windows-hosted-diagnostic-sample.md` and `docs/PROGRESS.md`. No Rust, Cargo, workflow, nextest profile, `scripts/` or quarantine guard change.
- Narrow scope: R1 and R2 from `docs/reviews/m3-windows-hosted-sample-opus-recheck.md`, the new tests and the evidence text. S1 and S2 are rechecked only where this delta touches them.
- Credit: Opus authored the original sampler (`c0ae73e`…`a52ed05`). Independent credit here covers **only Sol's `89652d5` delta**. Sol's source review (`a38ae7a`) and its B1/B2 findings are retained separately and are not re-credited here.

## Spec

**R1 is closed.** `run_phase` now returns `Phase("expired")` at all three points before go: before job create, after create and before spawn, and at the final check before `G` (after assign, `IsProcessInJob`, the child poll and the cancel check). `"killed"` is set in exactly one place, the wait loop after `G` has been written (`windows_diag_sample.py:899`). It therefore means only that a test process received go and was stopped at the phase deadline. `sample` maps `expired` to `Guard("deadline")` for both phases. Run `killed` still maps to diagnostic `nextest=killed outcome=diag_incomplete`, exit 0. List `killed` stays `deadline_list`. `deadline` was already in `GUARDS`, so the output grammar is unchanged. The before-go refusal still calls `_reap_unassigned`, and the `finally` still closes the job, so the mandatory kill is kept.

New test `test_pre_go_expiry_inside_step_budget_is_integrity_failure`. I reproduced its timings with the committed Harness:

| Subcase | Delay | Run-phase deadline | Ends at (from start) | Head result |
| --- | --- | --- | --- | --- |
| create (2nd job) | +65 s after create | ≈61 s | 66 s < 90 s | exit 1, `guard`, `nextest=none`, 1 proc |
| spawn (run) | +65 s after spawn | ≈61 s | 66 s < 90 s | exit 1, `guard`, `nextest=none`, run proc go `None`, killed once |
| assign (2nd job) | +65 s | ≈61 s | 66 s < 90 s | exit 1, `guard`, `nextest=none`, go `None`, killed once |
| stale JUnit unlink | +82 s | D − 8 = 82 s | 83 s < 90 s | exit 1, `guard`, `nextest=none`, no run spawn |

Each subcase ends after its phase deadline and before D. The test asserts this window directly (`t − start < STEP_S`), so the deadline guard cannot come from the final `check_deadline(D)`. The test also asserts `guard reason=deadline`. These are exactly the four R1 windows from my recheck.

S2 (a guard from the `finally` block replaces an earlier guard reason) was not in scope and is unchanged. It stays P3 advisory.

## Standards

**R2 is closed.** I compared PROGRESS byte for byte at `a38ae7a` and `89652d5`:

- **Unchanged structure.** 3121 lines at both revisions. No BOM, no CRLF, trailing newline present.
- **Only line 26 differs.** Every other line is byte-identical.
- **Encoding restored.** There are 77 non-ASCII characters, with the same per-character inventory as `a38ae7a` (`Counter` equal), and no mojibake sequences remain (`â€`, `Â¹`). `aff1d65`/`81a37bf` had 230 non-ASCII characters and 74 mojibake sequences.
- **Intended text kept.** Line 26 equals `aff1d65` line 26 after the cp1252→UTF-8 reversal. So the head keeps exactly the intended "Current work" replacement and no other edit.

Tests are stdlib-only and use the existing Harness seams. The source change is a small, single-concern rename of one status. `git diff --check 81a37bf 89652d5` passes. The source-token test (no `taskkill`/`os.kill`/`pid`/psutil) still passes.

The S1 gaps from my recheck, as of this delta:

- **Closed:** M9 (unexpected kind deleted) and M17 (unexpected kind not gated). `test_unexpected_cleanup_kind_is_never_removed_and_gates` kills both. It turns list.json into a reparse point after JUnit is read, then asserts exit 1, list.json not unlinked, privdir still attempted and `guard reason=cleanup`. It passes on the parent source too. That is expected: it pins behavior that was already correct in `aff1d65`, as the evidence says.
- **Still open, P3 advisory, not blocking:**
  - **Redundant pre-spawn checks.** The before-create check and the after-create check (N8/N9, formerly M3/M4) still survive. The pre-go check backs them up, so the outcome still gates `deadline`, no go is sent and the child is reaped. But a process would be spawned after expiry without any test noticing. Asserting `len(h.spawner.procs) == 1` in the create and stale-JUnit subcases would pin this.
  - **List-phase `expired` mapping untested** (N6 drop gives `list_exit`; N7 gives `deadline_list`). Both mutants still gate exit 1, so only the guard reason's precision is unpinned.
  - **Clipping mutants not rerun.** I did not rerun M5/M14/M15/M16 (wait clipping); this delta does not touch them.
- **Nit: stale comment.** The `Phase.status` comment (`windows_diag_sample.py:808`) still lists `exit | killed | cancelled` and lacks `expired`.
- **Advisory: PROGRESS line 26 is stale.** As instructed, it is reconstructed verbatim from the intended `aff1d65` line. It still says "55 pure sampler tests" and "fresh complementary Opus review pending". Under AGENTS.md the coordinator should update it to 57 and to this recheck's outcome in a later bookkeeping commit. This does not affect R2, which concerned byte integrity.

## Security

This delta adds no termination path, filesystem operation, external call or secret. The only process control is still `Popen.kill` on the wrapper's own handle, plus `TerminateJobObject`/`close` on its own job. Expired runs still reap the owned unassigned child before returning (N10: removing the reap is killed by 8 failing tests). The final cleanup still refuses to remove an unexpected kind and gates on it, and that is now pinned by a test. The `lstat` checks still cannot rule out a concurrent swap of an ancestor directory; this is unchanged and stays retained from Sol's review, not credited here.

## Evidence text

The evidence section is accurate:

- **Red/green logs.** `D:/Temp/arx-sampler-r1-red.log` shows 57 tests with 4 failures, which are exactly the four `seam=` subcases. `r1-green` shows 57 OK, `r1-node` 29 pass, and `r1-guard` 1 OK.
- **Earlier red log stated honestly.** The evidence says the earlier red log covered four methods and that two were added after that run. That matches my recheck's count of six new methods red against `a38ae7a`.
- **Parent SHA.** `a76c223` as the recheck SHA, retained as `81a37bf`, is correct.
- **No inflated claims.** The section claims no native or hosted evidence.

## Actual reviewer checks (this checkout and bounded scratch)

- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py'`: 57 PASS.
- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_cluster_quarantine.py'`: 1 PASS.
- `node --test scripts/*.test.mjs`: 29 PASS.
- `git diff --check 81a37bf 89652d5`: PASS.
- **Red check:** I extracted `git archive 89652d5` to `D:/Temp/arx-opus-final-recheck-89652d5/red` and swapped in `81a37bf:tools/ci/windows_diag_sample.py`. Result: 57 run, 4 FAIL. All four are `test_pre_go_expiry…` subcases (create, spawn, assign, stale_junit), each `AssertionError: 0 != 1`, so the old code exits 0. The new cleanup-kind test passes there, as a pin. All other tests pass.
- **12 single-line mutants** against the head suite in `D:/Temp/arx-opus-final-recheck-89652d5/head`:
  - **Killed:** each of the three `expired`→`killed` reversions (N1, N2, N3), dropping the run `expired` mapping (N4), remapping run `expired` to `diag_incomplete` (N5), M9, M17, and dropping the before-go reap (N10).
  - **Survived (P3 advisory above):** N6, N7, N8, N9.
- **Timing probe:** I reproduced each subcase's end time and output line with the committed Harness (table above).
- **PROGRESS analysis:** byte, line and non-ASCII inventory comparison of `a38ae7a`, `aff1d65`, `81a37bf` and `89652d5`.
- **Scratch cleanup:** the mutant/probe scripts stayed in the session scratchpad. The D: scratch tree was removed after the run. Nothing was installed.

Reused, not rerun by this reviewer: the coordinator `r1-*` logs, of which I read only their summary and failure lines. Also reused are all earlier evidence: the original Opus native wrapper/Job Object controls, the hosted timing `87c9940`/`43823823164`, and the earlier coordinator logs. No native wrapper, cargo, Job Object, native M2, VM, admin, install, hosted dispatch or rerun, full `npm test`/UI, rustfmt or clippy was performed.

## Remaining limits (unchanged)

- **Deadline is checked, not enforced in real time.** The overrun bound is checked after control returns from a call. A stalled synchronous call can exceed 90 s, and the 720 s job cap is then the backstop.
- **Unconfirmed reap after D.** After D, the exit of an unassigned child that was killed before go is not confirmed; it gates `cleanup` instead. This is **unverified on real hardware** and on hosted Windows.
- **Hosted paths unexercised.** Hosted ownership, timings, failing real nextest paths and cancellation remain unexercised. Windows three-node remains **unverified on real hardware**.

## Before/after and reversibility

- **Before:** a run that expired before go, after its phase deadline but before D, exited 0 with `diag_incomplete nextest=killed`, and PROGRESS was mojibake.
- **After:** that window gates exit 1 with `guard reason=deadline nextest=none`, and PROGRESS is byte-identical to `a38ae7a` except for the intended line 26.
- **Reversibility:** this is a two-way door. A revert restores `81a37bf`. No state, workflow, VM or resource is touched.

Windows stays quarantined; M2 stays open and M3 unchecked. This report grants no merge, publication or dispatch authority. Merge still requires green required checks and the coordinator's gates.

Standards: R2 closed. Remaining items are P3 advisories (N8/N9 spawn-after-expiry not pinned, N6/N7 list reason precision, stale `Phase.status` comment, stale PROGRESS line 26 counts). Spec: R1 closed; S2 P3 advisory retained. Security: 0 findings.
