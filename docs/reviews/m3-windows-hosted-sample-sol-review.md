# Sol independent implementation review: Windows hosted diagnostic sample

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-16. Single-agent independent review of Opus implementation; no delegates. **BLOCKED: B1 and B2 below.** No source edits, publication, status posting, dispatch, rerun, merge or native control execution.

## Pinned scope

- Base: `50087fefc2ee5a10bb9732df70c9a70b02c47f16`.
- Head: `a52ed053c9cd29a55a23ff9df9b06303a6299ac6`, also the prepared checkout HEAD at review start.
- Reviewed `git diff 50087fe...a52ed053c9cd29a55a23ff9df9b06303a6299ac6`: six files, 2119 additions. Commits: `c0ae73e`, `8a59468`, `4deb2f0`, `1b5fa0d`, `a52ed05`.
- Files: `.github/workflows/ci.yml`, `scripts/ci-workflow.test.mjs`, `tools/ci/windows-diag-sample.nextest.toml`, `tools/ci/windows_diag_sample.py`, `tools/ci/test_windows_diag_sample.py`, `docs/evidence/m3-windows-hosted-diagnostic-sample.md`.
- Read AGENTS.md, CONTRIBUTING.md, docs/KICKOFF.md, the dytschgo-review skill and its active profile, `docs/briefs/m3-windows-hosted-diagnostic-sample.md`, and `docs/reviews/m3-hosted-sample-sol-contract-review.md`. The explicit review assignment overrides kickoff coordinator/babysitter actions. PROGRESS remains coordinator-owned; only this report is committed.

## Standards

No separate blocking finding. The patch stays within the six-file scope, uses stdlib Python and injectable process/filesystem/time seams, preserves conventional focused commits, and leaves Rust, Cargo, the quarantine guard and other workflows untouched. No actionable Fowler baseline smell warrants another abstraction here. The meaningful-test requirement is only partially satisfied for the deadline and filesystem-cleanup behaviors: the passing tests miss the concrete cases in B1/B2; those findings remain on their respective axes.

The evidence accurately labels incomplete npm testing, fake-only cancellation and diagnostic failures, local versus hosted ownership, and the absence of acceptance credit. This review does not turn builder-reported native evidence into reviewer-executed evidence.

## Spec

**B1 — P1: the absolute wrapper deadline does not govern all work or prevent go after expiry.** `tools/ci/windows_diag_sample.py:847` creates/spawns/assigns and writes `G` before checking the supplied deadline (lines 849–890). `sample` clips phase deadlines at lines 980–981 and 1000–1001, but validation/import/read/parse/final file cleanup have no deadline enforcement. `_wait_exit` and ActiveProcesses polling create fresh relative windows rather than sharing the absolute deadline. The final report can be successful after 90 seconds. The brief requires “One monotonic step deadline D = start + 90 s” and “Validation ≤ 2 s”; the Sol recheck explicitly includes “child launch/assignment and error cleanup.”

Reviewer pure reproductions, using the committed Harness and only in-memory replacements:

- Advance FakeClock by 82 s while returning the list read. The run phase still receives `G` at elapsed 83 s, after its clipped 82 s deadline, then returns diagnostic success (`diag_incomplete`, `run_ms=0`). Even after a synchronous delay returns, expired work is launched.
- Advance FakeClock by 95 s in run spawn: exit 0, `diag_incomplete`, `wall_ms=96000`, and `G` was written. This is a model of a delayed call, not a claim that Windows Popen actually took 95 s.
- Advance FakeClock by 95 s in the final JUnit read: exit 0, `all_pass`, `wall_ms=98000`.

The documented interpreter-stall residual means an absolute real-time guarantee cannot be inferred from Python polling. It does not justify starting cargo after the clock is already past its deadline or reporting success without checking elapsed validation/postprocessing time. Carry the common deadline through launch, cleanup and finalization; refuse go when the phase deadline has expired; enforce the validation allowance and budget postprocessing. Keep mandatory child reaping and the diagnostic/integrity split. Add pure tests for these cases and delayed cleanup, beyond the current run-wait clipping test. A bounded ownership-preserving approach must still describe unavoidable OS/interpreter-stall limits honestly.

Other contract checks pass by source inspection and pure fixtures: exact eight matched names, only arxd/lib/listed and ignored=true, target binding, fixed argv/profile, four threads/zero retries, timeout profile, stale JUnit removal before run, and closed JUnit type/inventory handling. All declared test-caused outcomes remain exit 0: fail/abort/timeout/leak/exec_fail/not_run, malformed/oversize JUnit, and external run kill without parsing incomplete XML. Selection, ownership, list deadline, unexpected nextest exits and wrapper exceptions gate. This is fixture/source evidence, not a real failing nextest run.

Accepted implementation deviations: run-only `--no-tests fail` is appropriate because list's exact inventory already rejects no matches; reuse the documented nextest 0.9.151 flag rejection rather than rerun cargo here. `guard` and `skipped_event` are closed enum additions with tests; unsupported events release `event=-`, validate the anchor and spawn nothing. These extend the brief's grammar explicitly and safely for existing workflow_dispatch. The additional leaked-pass type, target-directory check, verified-byte EXPECTED import, and per-phase jobs are reasonable contract-preserving choices.

## Security

**B2 — P2: final filesystem cleanup failures are silently accepted.** `tools/ci/windows_diag_sample.py:1021` (the `sample` finally block) suppresses every exception while removing list.json, JUnit and privdir, and skips unexpected kinds. The contract says “remove it and privdir afterwards” and requires wrapper integrity failures to gate. A private JUnit contains precisely the raw paths/panic/output that the public renderer suppresses; an ordinary deletion failure can retain it while publishing `all_pass`.

Reviewer pure reproduction: replace FakeFs.unlink to raise PermissionError only for the final JUnit path, with no stale file present before run. Result: exit 0, `outcome=all_pass`, `junit_retained=True`. No native filesystem permission or reparse mutation was performed. Attempt all owned cleanup safely, preserve reparse/path refusal, then surface failure through a closed integrity guard rather than silently report success; add deletion/rmdir failure tests. Never compensate with recursive or unchecked removal. This is a cleanup-integrity/retention finding; no public raw-output leak was observed.

Ownership otherwise matches the reviewed design: unnamed non-inheritable kill-on-close job verified before spawn; assignment and IsProcessInJob before go; signal handlers installed first; no breakaway flags or process walks. Assignment/check failure and cancellation before go kill and poll the wrapper's own child; job closes on error. Child EOF and 5 s watchdog prevent indefinite pre-assignment wait after parent death; descendants start only after go. Normal/timeout/cancel paths terminate once and verify ActiveProcesses=0. Pure tests exercise these boundaries; hard parent death and real cancellation remain unexercised by this reviewer.

Reads are capped at cap+1 to detect oversize; expected-source bytes have a separate 64 KiB bound. Static path-kind/reparse checks cover the declared target chain, JUnit, temp/private directories and guard source; guard realpath and opened-file identity checks add protection. Typed D1 parsers re-render closed enums and bounded ASCII numbers; arbitrary paths, panics and exception text do not reach output. Per-test and aggregate output limits are exercised. These lstat/open checks are not a general handle-relative defense against concurrent hostile ancestor replacement; no stronger race-free filesystem guarantee is credited. No secret-bearing native output was collected or released in this review.

## Actual checks and retained evidence

Reviewer ran on this pinned checkout:

- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_diag_sample.py'`: 49 tests PASS.
- `python -I -B -m unittest discover -s tools/ci -p 'test_windows_cluster_quarantine.py'`: 1 test PASS, exact eight-ignore guard unchanged.
- `node --test scripts/*.test.mjs`: 29 PASS, including the four workflow cases.
- `npm test`: Node 29 PASS, then UI suite FAILS before execution because `vitest` is not recognized. Overall exit 1; no full npm pass claimed and no dependencies installed.
- `git diff --check 50087fe...a52ed05`: PASS.
- Four deadline/cleanup in-memory probes above: all reproduced the reported gaps. They import the committed test Harness using an explicit tools/ci sys.path, with bytecode disabled; no repository source was modified.

Retained, not independently rerun: the implementation evidence's local real wrapper `all_pass` in 9578 ms, orphans=0; grandchild/orphan/hang controls with ActiveProcesses=0; closed-job assign failure reaped in 0.11 s; list flag rejection and exact-eight real inventory; 23/24 mutation kills with one stated equivalent. These reports support local feasibility only. No new cargo, process-tree, VM, admin, native M2, cache deletion, install or hosted action was needed for this review. No rustfmt/clippy/UI/browser/full workspace checks were run.

The checked-in PROGRESS lines 49–63 retain the explicit revised C1 timing disposition and repaired-main `87c9940` run `43823823164` (Windows 396 s, install 7 s). This is reused recorded admission context, not independently fetched hosted evidence or a replacement for the original failed C1 confirmation. Hosted ownership/timings and failing real nextest diagnostic paths remain unverified here; Windows three-node remains **unverified on real hardware**.

## Before/after and reversibility

Before: the 26 post-C1 steps, one 12-minute job cap, existing env block, Playwright branches, smokes, quarantine and five gates. After: exactly those steps and their order plus a Windows anchor first and the Windows one-line `unittest && wrapper` last. The frozen test verifies all 28 identifiers and exact new text; the source diff adds no continue-on-error or cap/env/CARGO override. The new required step can gate wrapper integrity and consumes admission headroom; its absolute-bound claim is blocked by B1. Passing samples carry no product or re-enable credit.

Two-way door: remove the two added CI steps and their sampler-specific workflow assertion, then retire the wrapper/profile/unit tests in a focused reviewed revert; retain evidence/review history. Existing post-C1 steps, quarantine, C1 optimization and required gates need no change. No state migration, deployed service, VM, static resource or backup is affected. Rollback remains subject to the normal independent review/checks; this report grants no merge or dispatch authority.

Windows stays quarantined; M2 remains open and M3 unchecked. B1 and B2 require correction and fresh independent review; full npm/hosted checks remain pending in an equipped authorized environment.

Standards: 0 separate findings; Spec: 1 finding (worst B1, P1 absolute deadline); Security: 1 finding (worst B2, P2 silent cleanup retention).
