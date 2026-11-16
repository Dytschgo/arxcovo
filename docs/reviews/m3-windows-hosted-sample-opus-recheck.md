# Opus complementary recheck: Sol sampler correction aff1d65

Reviewer: Anthropic / Claude Opus 5.5 / high, 2026-11-16. Single-agent review, no delegates. **BLOCKED: R1 (Spec) and R2 (Standards) below.** No source edits, publication, status posting, PR, dispatch, rerun, merge or watch.

## Pinned scope and independence

- Parent `a38ae7a` (Sol review retained). Head `aff1d65f6a48ab17677d819fceb8af5d4a36f8d7`, which is also this checkout's HEAD. Reviewed `git diff a38ae7a aff1d65`: four files, `tools/ci/windows_diag_sample.py`, `tools/ci/test_windows_diag_sample.py`, `docs/evidence/m3-windows-hosted-diagnostic-sample.md`, `docs/PROGRESS.md`. No Rust, Cargo, quarantine guard, nextest profile, workflow or `scripts/` change (verified with `--stat`).
- Read: AGENTS.md, `docs/reviews/m3-windows-hosted-sample-sol-review.md` (B1/B2), `docs/briefs/m3-windows-hosted-diagnostic-sample.md`, and `docs/reviews/m3-hosted-sample-sol-contract-review.md` as cited there.
- Conflict: Opus authored the original sampler (`c0ae73e`…`a52ed05`). This review grants independent credit **only to the Sol delta**. Claims about the original code are Sol's source review, retained here without re-crediting.

## Standards

**R2: P2, blocking. aff1d65 corrupts the encoding of the whole of `docs/PROGRESS.md`.** The file was decoded as cp1252 and re-saved as UTF-8. At `a38ae7a` it has 77 non-ASCII characters: 43 en dashes, 30 em dashes, 3 arrows and 1 `¹`. At `aff1d65` all 77 are mojibake (`–`→`â€“`, `—`→`â€”`, `→`→`â†’`, `¹`→`Â¹`) on 70 of 3121 lines, including the retained C1 timing record on line 49 (`07:26:06â€“07:32:42`) and acceptance time ranges throughout. This is why the stat shows 142 changed lines. Once the encoding is undone (`text.encode("cp1252").decode("utf-8")`), the only real change is line 26 (the "Current work" status), and its wording is accurate. Reversing the encoding restores the parent's exact non-ASCII inventory, so the damage is lossless to repair. PROGRESS is the coordinator's authoritative record, and this edit is out of the correction's concern. Restore the parent encoding and keep only the line 26 change.

**S1: P3, not blocking alone. Some claimed behaviors have no test.** Mutation results are below. These survived the suite: final cleanup's refusal to delete an unexpected kind, and gating on it (M9, M17); clipping of the child-exit wait (M5) and of the unassigned reap wait (M14) to the common deadline; the pre-create and post-create expiry checks (M3, M4); and sample-level passing of `deadline` as the phase cleanup deadline (M15, M16). A direct probe confirms the head code is correct for an unexpected kind: JUnit turned into a reparse point after it is read gives `guard reason=cleanup` and the file is not unlinked. The evidence text claims it, yet no test pins it. Add these tests with the R1 fix. M10, M11 and M12 each survive alone because the three deadline checks back each other up (the finally block, `run()`, and the check after rendering). They are equivalent one at a time and need no test.

Otherwise conforming: stdlib-only code, the seams are unchanged, the commit is small and conventional, and the diff adds no `kill`, `taskkill`, `os.kill`, psutil, signal or subprocess use. `git diff --check a38ae7a aff1d65` passes.

## Spec

Sol B1 is **closed for late go and late success**:

- **Validation.** Allowance is 2 s: `check_deadline(start + VALIDATION_S)` runs after the last validation `require`.
- **Before go.** `run_phase` refuses expiry before job create, before spawn and immediately before `G`. The last check comes after assign, `IsProcessInJob`, the child poll and the cancel check, and nothing blocks between it and the write.
- **After inventory.** `check_deadline(D − 8)` runs after the inventory check.
- **Cleanup.** The child-exit wait, the `ActiveProcesses` poll and the unassigned reap are clipped to `D` (the sample passes `deadline` as `cleanup_deadline`).
- **Final.** The finally block checks `D` after file cleanup, and `run()` re-checks after `sample` and again after rendering.

Sol's three B1 reproductions now gate: list read +82 s gives `guard reason=deadline` with no run spawn; run spawn +95 s gives `deadline`, `G` is not written and the owned child is killed once; JUnit read +95 s gives `deadline`. Synchronous calls that overrun are not preempted, and the code comment and the evidence say so honestly. An ordinary run killed at its phase deadline remains diagnostic `diag_incomplete` with exit 0 (`test_run_deadline_is_clipped_by_absolute_step_budget`, now driven by a real inventory delay). `deadline` is a closed addition to `GUARDS`.

**R1: P2, blocking. When the run phase expires before go, it is still reported as a diagnostic test stall.** `run_phase` returns `Phase("killed")` for expiry before create, before spawn and before go. `sample` maps any run-phase `killed` to `nextest=killed outcome=diag_incomplete`, exit 0. When the stall ends after the run-phase deadline `min(start_run + 60 s, D − 8 s)` but before `D`, the wrapper publishes a diagnostic outcome for a stall in wrapper or OS launch, and it claims `nextest=killed` although nextest never started (`G` was never written, or nothing was spawned). This is the shape Sol called "diagnostic success (`diag_incomplete`, `run_ms=0`)" in B1. The added `test_expired_launch_never_sends_go` passes for `run` only because its 95 s delay also passes `D`. Reviewer probes, using the committed Harness with in-memory replacements only:

| Delayed call (run phase) | Result |
| --- | --- |
| spawn +60/70/75/80 s | exit 0, `diag_incomplete`, `nextest=killed`, `run_ms`=delay, go `None` |
| assign +75 s | exit 0, `diag_incomplete`, `nextest=killed`, go `None` |
| job create +78 s | exit 0, `diag_incomplete`, `nextest=killed`, `run_ms=0`, no run spawn |
| stale JUnit unlink +85 s | exit 0, `diag_incomplete`, `nextest=killed`, `run_ms=0`, no run spawn |
| stale JUnit unlink +70/78 s (control) | exit 0, `all_pass`: the run still had time and finished inside its phase deadline. Correct. |

The brief defines `diag_incomplete` as "the wrapper's external deadline killed `run`" and reserves exit 0 for what the eight tests can cause. The grammar already has `nextest=none`. Fix: give expiry before go its own status (for example `Phase("expired")`, distinct from killed after go). Map it to `Guard("deadline")` for the run phase; the list phase may keep `deadline_list` or use `deadline`. Keep the reap/kill. Add pure tests in the window between the phase deadline and `D` for create, spawn, assign and stale-JUnit unlink.

**S2: P3, not blocking. Guard reasons get masked.** When the finally block's `Guard("cleanup")` or `check_deadline` fires, it replaces an earlier guard. In a probe, run exit 101 plus a slow final rmdir gives `reason=deadline`, not `nextest_exit`. The result still gates, but it loses the first cause. Consider keeping the first guard and adding the cleanup/deadline status, or document which reason takes precedence.

Mandatory kill despite expiry is kept: before-go refusals always call `_reap_unassigned` (own `Popen.kill`), and normal, killed and cancel paths always call `terminate(job)` once, check `ActiveProcesses` at least once and `close(job)` in `finally`. After expiry the reap and exit-confirmation waits shrink to zero. An unconfirmed exit then gates `cleanup` rather than being waited out. This is conservative, and `KILL_ON_JOB_CLOSE` covers assigned children. It does mean that a child killed before assignment after `D` is not confirmed reaped. See the limits section.

## Security

Sol B2 is **closed in behavior**. The final cleanup visits list.json, JUnit and privdir in order, keeps going after a failure, removes only the expected kind (`file`/`file`/`dir`), never deletes an unexpected kind, treats any exception or unexpected non-missing kind as a failure and raises `Guard("cleanup")`. It does no recursive or unchecked removal. Sol's B2 repro (final JUnit unlink raises `PermissionError`) now gives exit 1, `guard reason=cleanup`, and the remaining paths are still attempted. The new test checks this for each of the three paths, and M7/M8 show the test is effective. Retained raw JUnit can no longer go out with `all_pass`. No signal or process API reaches anything the wrapper does not own. `proc.kill` on the wrapper's own Popen handle and `TerminateJobObject` on its own job are the only termination paths, as before. The lstat checks still give no race-free guarantee against concurrent replacement of an ancestor (unchanged from Sol's review; not credited). The only Security gap is the S1 test gap for unsafe kinds.

## Actual reviewer checks (this checkout)

- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py'`: 55 PASS.
- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_cluster_quarantine.py'`: 1 PASS.
- `node --test scripts/*.test.mjs`: 29 PASS.
- `git diff --check a38ae7a aff1d65`: PASS.
- **Red check:** the head test file run against the parent's `windows_diag_sample.py` in a scratch tree (the D1 source pin was excluded because that tree had no Rust sources). All **six** new methods are red: five FAIL plus `test_phase_cleanup_poll_uses_common_deadline` ERROR (new signature). This is stronger than the evidence's "four". The coordinator red log `D:/Temp/arx-sampler-red.log` shows 53 tests with 4 failures, which is consistent with the two remaining methods being added after that run.
- **17 single-line mutants** against the head suite, in a scratch tree with a D1 source copy. Killed: M1 validation check, M2 pre-go check, M6 active-poll clip, M7 swallowed cleanup exception, M8 stop at first failure, M13 post-inventory check. Survived: M3, M4, M5, M9, M14, M15, M16, M17 (S1), and the equivalent M10, M11, M12.
- Pure probes for R1, S2, Sol B1 ×3 and B2, plus the unsafe-kind final cleanup probe, as reported above. The probe and mutant scripts stayed in the session scratchpad and are not committed.
- Byte and character analysis of PROGRESS at both revisions, for R2.

Reused, not rerun by this reviewer: the coordinator's 55/29/guard logs on `D:/Temp`, of which only the red and corrected logs were read; the original Opus native evidence (real wrapper `all_pass`, Job Object grandchild and assign-failure controls, list flag rejection, 23/24 mutants); the PROGRESS hosted timing `87c9940`/`43823823164`. No native wrapper, cargo, Job Object, install, VM, admin, native M2, hosted dispatch or rerun was performed, and no full `npm test`/UI, rustfmt or clippy run was done.

## Remaining limits

- The overrun bound is checked after control returns; it is not enforced in real time. A stalled synchronous OS call or the interpreter can still exceed 90 s, and the 720 s job cap then remains the backstop (brief residual).
- Once the deadline has passed, the confirmation that a killed, unassigned pre-go child has exited is skipped (zero wait). It then gates `cleanup` instead of being confirmed. A real Windows `TerminateProcess` behaves this way, but that is **unverified on real hardware** and on hosted Windows.
- Cleanup taking close to 8 s after a run killed at `D − 8` can turn a test-hang `diag_incomplete` into a `deadline` gate. This is the same gate exposure the brief already accepts for `cleanup`.
- Hosted ownership, timings, failing real nextest paths and cancellation remain unexercised. Windows three-node stays **unverified on real hardware**.

## Before/after and reversibility

Before, a run could start or report success after 90 s, and a failed final cleanup was silent. After, both gate, except for the R1 misclassification window. The change is a two-way door: a revert of `aff1d65` restores `a38ae7a`, and no state, workflow, VM or resource is affected. Required fixes: R1 (pre-go expiry gates, never `nextest=killed`), R2 (restore the PROGRESS encoding), and the S1 tests alongside them. Then a fresh independent review of the corrected delta is needed.

Windows stays quarantined; M2 stays open and M3 unchecked. This report grants no merge, publication or dispatch authority.

Standards: 2 findings (worst R2, P2 PROGRESS encoding corruption; S1 P3 test gaps). Spec: 2 findings (worst R1, P2 pre-go run expiry reported as diagnostic `nextest=killed`; S2 P3 guard-reason masking). Security: 0 blocking findings (B2 closed; unsafe-kind test gap tracked as S1).
