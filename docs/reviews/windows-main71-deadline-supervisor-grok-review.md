# Windows main71 native replay: deadline supervisor review

Disposition: the Sol high deadline-supervisor slice is **acceptable as source**.
One `OwnedSession` owns the clock, the `Supervisor`, the ledger, the channel, the
Job, and a failure-only waiter for both filters. Confirmation is bounded by the
absolute row cap and the absolute 360000 ms cap. The extra `now+2000` wait is
gone. Late create, late assignment, and late resume admission cannot authorize
a running child after failure is latched. **Native run readiness is NOT
CLEARED.** This review grants no publication, PR, merge, CI admission,
registration, launch, or selected-environment provenance.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
No historical retry was issued. Phase 1 still has no focused red loop that
reaches that failure. Real-Runner PA6 record/replay remains **OPEN**. Native,
installed, and dedicated paths are **unverified on real hardware**.

## Pins

Porcelain was empty on entry. The review cwd is the linked worktree top. The
Git directory is that worktree's own gitdir. The retained source and evidence
commits are ancestors of this head. The supplied commits are not ancestors;
each pair has one tree. Paths below are repository-relative. No owner path,
account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/windows-deadline-supervisor-review` |
| Reviewed head | `1213687467f505d15ac2c99c1fec43297cbd0b92` |
| Head tree | `c721b1119fbc1d403860c02909d18d8e6785cf84` |
| Head parent | `9c339dcbe558aa70e221743465f87547e463ebd9` |
| Head after evidence | `1213687` changes `docs/PROGRESS.md` only |
| Supplied source | `d381cacd77fba7d95f6530f2eb6012d25cc6b250` |
| Retained source | `045edf94f57ecfc7c5e4e78c1999a67182c343c2` |
| Source tree | `70b95b5e0607b45079e2b31bb79d969a86feb9eb` (both commits) |
| Source parent | `fb680ea6bd09e5f8624baf5d914d0fa96bc793c6` |
| Supplied evidence | `eb068669d6ec49372d0a111b18f933af2d37658f` |
| Retained evidence | `9c339dcbe558aa70e221743465f87547e463ebd9` |
| Evidence tree | `9ae2e4fa95bfca603330f282bbb63ccc97511590` (both commits) |
| Integration review read | `984d345c71ea91dfce069c2000da2caa1f8556d4`, retained `620b3be91ee90dc957b8e03b6f0b53177b6e77a0`, tree `f4f2751ffe109f2a4ba3d0cc3e9ea7525baee3a0` |
| Telemetry review read | `530d9b5514217c84bbc746bbc07787708ab71ca5`, retained `2e1ac8f094ca314a7d9c3a0e348afcc3be6f6af4`, tree `e4c33da81e0dd0a925ce3caf9374a0138fce94e5` |

Source diff, parent through retained source:

```text
git diff --stat fb680ea6bd09e5f8624baf5d914d0fa96bc793c6 045edf94f57ecfc7c5e4e78c1999a67182c343c2
```

```text
 tools/windows-native-replay/Check-Source.py       |  23 ++-
 tools/windows-native-replay/Controller.cs         | 226 ++++++++++++++++++---
 tools/windows-native-replay/NativeJob.cs          |  65 ++++--
 tools/windows-native-replay/PINNING.md            |  62 ++++++
 tools/windows-native-replay/Test-PureController.ps1 |  19 +-
 5 files changed, 354 insertions(+), 41 deletions(-)
```

`git diff` between each supplied commit and its retained commit is empty.
`git diff --check` on the source range is empty.

Evidence prefix preserved, read only, not rewritten. Prefix length `100633`,
SHA256 `cd8c28a493db92282a0c07c7da45c0e59a6d297df7fc57f68cc1e532c3ee9fc7`,
one `0x96` byte at offset `39689`.

## Standards

Repo rules read: `AGENTS.md`, `CONTRIBUTING.md`, and `docs/KICKOFF.md`.
Behavior needs a meaningful test, and a check that was not run stays unlabeled.
The smell baseline was applied to the five-file diff.

No documented-standard breach. The new pure checks call the same `Supervisor`
the owner uses. The 11 new assertion mutants compile and die on the contract
they weaken. `Check-Source.py` records the structural order: create before
assign, assign before resume authorization, resume authorization before
`ResumeThread`, dispose of the final handles before `Finish`. The waiter text
between `FailureWait` and `Run` contains `Fail` and `Cancel`, and it contains
neither `Finish(` nor `Poll(`.

Judgement, divergent change: `Controller.cs` now also carries the deadline
machine beside admission, decoding, and transport. The repo has no rule that
forbids that layout. A split can wait until a second caller exists.

Skipped as already enforced or pre-existing: formatter absence for this C#
tree, and the string-index style of `Check-Source.py`.

## Spec

Origin: accepted scope proposal `docs/reviews/windows-main71-native-replay-scope-proposal.md`
("No extra two-second wait may be added outside the row deadline"; synchronous
Win32 calls are not assumed interruptible), integration review SP-DEADLINE at
retained `620b3be`, and telemetry review next slice at retained `2e1ac8f`.
`PINNING.md` "Supervisor deadline slice" is the builder's statement of this
edit. It matches the code that was read. It is not a substitute for this review.

The owned path now does the following.

- Initial deadline is `10000`. A BEGIN installs `min(rowStart+10000, 360000)`.
  END, empty polls, launch, and cleanup do not refresh it. After row 34 the
  last absolute cap stays in force. `ConfirmationDeadline` clamps again at
  `360000`.
- `Tick` latches failure when `now >= cap-2000`. That is the work cutoff.
  `CleanupConfirmed` and `KillAndConfirm` accept a sample only when
  `now < cap`. Exact equality confirms neither cleanup nor success. The loop
  no longer computes `ElapsedMilliseconds+2000`.
- A direct `Budget` check still rejects a row at `rowStart+10000` and the
  session at `360000`. Through `Supervisor.Poll` the earlier cutoff wins, so
  the 2 s reserve sits inside both caps. A 9 s row is a supervisor failure
  even though `Budget.Accept` would still allow it. That is the conservative
  cutoff this slice states.
- `BeginOperation` increments before `Revalidate` and `Launch`. Those calls
  do not hold the supervisor gate. `FailureTick` can latch while
  `CreateProcessW` or `AssignProcessToJobObject` is blocked.
- `FailureWait` can call `FailureTick`, `Fail`, `KillIssued`, `RetainOwner`,
  and `Cancel`. `FailureTick` returns without reading the ledger and without
  setting `done`. `Finish` is the only writer of `done`, and it requires a
  fresh `Poll`, `operations==0`, an empty valid channel, `Budget.Complete`,
  exit 0, and active 0, under the same gate. A latched failure makes that
  `Poll` fail.
- `Created`, `Assigned(true)`, and `AuthorizeResume` return false once
  `failed` is set. `Assigned(false)` latches failure. `AuthorizeResume` is
  one-shot and requires a created, assigned, unfailed operation.
  `CheckCancelled` runs before create, after create, and after assign.
  Each false observation calls `Cancel` and throws. `ResumeThread` is reached
  only after `AuthorizeResume` returns true.
- The process handle is published before assignment. Cancellation and the
  assign-failure path call `TerminateJobObject` on the owned job and
  `TerminateProcess` on that handle. `CreateJobObjectW` is unnamed.
  `ProcessInfo.pid` is unused. There is no process-name kill.
- `Next` refuses a failed or cancelled session and retires the exited
  handles instead of closing them. `Launch` refuses a cancelled session, a
  live process slot, or a nonzero active count. `Run` refuses a finished,
  failed, disposed, or retained session.
- Final success disposes handles inside a counted operation, then calls
  `Finish(now, true, 0, 0)`. Both filters have already required exit 0,
  active 0, a closed segment, and an empty channel. The final call's fresh
  work is the ledger poll. No native query is coded after that sample.
- `Abort` latches, records the kill, retains the owner, then confirms inside
  the absolute cap. `Cleanup` also requires `killIssued` and `operations==0`.
  A failed `Join(0)` keeps the owner. `Dispose` and `Cancel` share the
  lifetime lock. Failure latching does not take that lock.

Interleavings read from the owner and waiter, not only from the pure model:

- Create returns after the waiter has latched and returned. `Created` sees
  `failed`, `Cancel` runs on the now-published handle, and `Run` returns
  false. The waiter does not need a second turn for that path.
- Assign returns after the latch. `Assigned` returns false for both a failed
  assign and a successful assign observed after failure. The code terminates
  the job and the owned process and does not call `AuthorizeResume`.
- `AuthorizeResume` can return true and then lose the race to `ResumeThread`.
  The waiter, or the following `Abort`, still requests job and process-handle
  termination. `EndOperation` cannot clear `failed`. `Run` returns false.
  Source order does not prove which kernel call wins.
- `Finish` and `FailureTick` take one gate. The waiter cannot turn a latched
  failure into `done`. After `done` is set, a later waiter sample does not
  reopen success. The success path has already disposed the job.
- A blocked `Dispose` or `TerminateJobObject` holds the lifetime lock while
  the other side waits. `Finish` is not reached while dispose is still
  inside that lock and failure was latched. `Run` does not return true.
- `KillAndConfirm` treats a null current process slot as already exited.
  `Next` clears that slot only after the filter loop observed exit and
  active 0, and it keeps the handle on `retired` until final dispose.
  A live unassigned child is still in `process` from the publish before
  assign. The pure suite does not execute this loop.
- `Join(0)` can retain an owner whose native sample already confirmed
  cleanup, because the waiter may only be in its 1 ms sleep. Retention is
  fail-closed. It does not create a second launch.

Pure contract versus kernel behavior:

- `SupervisorPureChecks` drives the real `Supervisor`, `MonitorState`,
  `Budget`, and `RowChannel` with synthetic times and operation results.
  It proves the modeled transitions listed in the checks section.
- It does not start `OwnedSession`, the waiter thread, `Stopwatch`, or
  `NativeJob`. It does not prove that a Windows call returns, that
  `TerminateJobObject` is a fence against a later assign, that
  `CREATE_SUSPENDED` leaves a suspend count of 1, or that kill-on-close
  reaps an arbitrary tree.
- A stuck `CreateProcess`, assign, resume, query, terminate, or close has
  no finite bound in this source. The waiter can request termination and
  can retain the kill-on-close owner. It cannot cancel the blocked thread,
  certify cleanup, or make the blocked call return. Unknown cleanup stays
  unknown. Only the confirmation poll is capped.

No requirement of this slice is missing. Handle delivery, argv, environment
grammar, and duration measurement are unchanged. `durationMs` remains the
reserved constant. `RevalidateBoundary` still throws
`remaining admission gates unproved`. `RequireTelemetryBinding` still throws
`telemetry launch binding grammar unreviewed` before `CreateProcessW`.
`Invoke-BoundedReplay.ps1` still throws `GATE2_ONLY` before `Add-Type`.
`selectors.json` still has `registration_enabled` false. Pure `Run` still
contains no `new OwnedSession(`.

## Security

**0 findings reachable from the current entries.**

The PowerShell entry throws before compilation or a process API. Parameter
validation cannot enable execution. `PureControllerChecks.Run` does not
construct `Admission`, `HeldFiles`, `OwnedSession`, or `NativeJob`, and it
does not call `Launch`. Compiling the imports does not call them.
`Revalidate` throws before `Launch`. `Launch` throws before `CreateProcessW`
even if that admission throw were removed. The inventory check finds no
`#[test]` or `#[ignore]` on either native filter, and the pure Rust module
excludes `run_inner(`, `unsafe`, `std::process`, and `std::fs`. The helper
body check requires `fn main() {}`.

The dormant limits, once both throws were deleted, would still be real:
grammar is unwired, nested-Job behavior is unexecuted, a blocked syscall is
not bounded, and an in-flight `ResumeThread` is not atomic with the kill.
Do not remove either throw as a clearance. No account, service, VM, owner
profile, or Lab Manager state was inspected.

One dormant ownership window, not a reachable finding: the thread handle and
the process handle are wrapped in two statements. An exception between them
would leave the raw process pointer unpublished, so `Cancel` would not see
it. Both wraps stand before `Created` and before any resume. The practical
trigger is constructor failure. The kill path that does run uses the owned
job handle and the owned process handle.

## Failure ledger

No three-attempt handoff trigger.

The evidence note records in-slice test repairs: an obsolete constructor
string in the structural check, a `CS0414` compile that was not counted as a
kill, and a kill-flag mutant that survived until an outstanding operation no
longer masked it. The final managed run is the one reproduced below. Those
notes are one implementation pass. They are not three failed fixes of the
same defect. Expected assertion kills are negative controls.

Historical main71 run `43545244266` stays a separate unknown cause. This
review issues no retry.

## Checks this pass actually ran

`PureControllerChecks.Run` was read before execution. It reaches in-memory
JSON, SHA256 of caller bytes, synthetic clocks, synthetic frames, and the
in-memory admission and binding fixtures. `SupervisorPureChecks.Run` reaches
the real supervisor machine with integer times. The graph does not reach
`NtCreateFile`, `CreateProcessW`, `Stopwatch.StartNew`, `new Thread`, a
filesystem open, or `OwnedSession`. `System.IO.Path.GetFileName` in the
binding fixture is a string split. `Check-Source.py` reads fixed source text
and `selectors.json`.

```text
python tools/windows-native-replay/Check-Source.py
```

Exit 0. The five inventory lines match the evidence block, including
`supervisor source: one owned clock; failure-only waiter; absolute teardown cap; synthetic machine checks`.

```text
pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1
```

Exit 0. Baseline passed. All 78 mutants were assertion-killed. Their LF
SHA256 values and kill messages match the evidence table. The 11 new kills:

| Mutant | LF SHA256 | Kill |
| --- | --- | --- |
| `supervisor-global-cap` | `d680c72c3b4cffd57e26b547dc359c481abe0c7b336dd9f914978a174ab2c4c7` | `supervisor: global teardown clamp` |
| `supervisor-resume-failure` | `dfcc99395ef73c1a5b206040676e93256b06083476e4fe89be14dc976e81c6bc` | `supervisor: late completion cannot resume` |
| `supervisor-resume-assignment` | `618bbd42fc4d7c668111c53cc441d1b656a65ca2ba2e8fc6f0e9ca4143503a59` | `supervisor: unassigned cannot resume` |
| `supervisor-assignment-latch` | `21663e6e2c806d009e89ead6f05372c22f85d9a5b632921a259b7cfbfc28fdd5` | `supervisor: containment irrevocable` |
| `supervisor-resume-once` | `f6783b068ff8833c54bb25f78875968502ce085309877f84430c295b4ceed04c` | `supervisor: resume only once` |
| `supervisor-reserve` | `5cb87d7d8d3437114ec849893893090ffc4729566bcfe116d1fb1ab3ed04a1b8` | `supervisor: launch stuck` |
| `supervisor-late` | `cf7773c245e761f8950a747aac0611de12a6b26d60611dbfd7d9a4f6297504a7` | `supervisor: no resume on failure` |
| `supervisor-operation` | `5d2f05efcdcdf4aa883964acff26cacea9581eb4a0d0ca05b5379050326a3d45` | `supervisor: outstanding prevents cleanup` |
| `supervisor-kill` | `5f437a5542cd05f162981a21a36fbaee849fb4b4ddb2f4c485dcb8eb83c547ee` | `supervisor: kill precedes empty cleanup` |
| `supervisor-final-operation` | `0cd4204aafa683e2077c64ed96d0d3ffa2da16d52ae7ad7a731b99f13dfd5c07` | `supervisor: final outstanding race` |
| `supervisor-final-poll` | `9746edaafef6990a7b4ff6e5fe46c20a65ceeeea2d93fa2af187b0132559177b` | `supervisor: last row cannot succeed late` |

Unchanged LF SHA256 after the run:

| File | SHA256 |
| --- | --- |
| `Controller.cs` | `d1155dcce5464270a1f98fc08fcfe116365e355694220d9eeb9d835824f4fda5` |
| `HeldFiles.cs` | `9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643` |
| `NativeJob.cs` | `7c41bca70616f2ed6a0aeed5d30ff9700d1e9a2586b0a3e8771bd1d5665efab7` |
| `Admission.cs` | `b6c1aa6b39d618a85e2183988cc95b44c965dfbb44670b2c2c4382d336754289` |
| `Test-PureController.ps1` | `f13089b413634559d69c948215cfb20e467c5534a89cced9a010aa13ee0037fe` |
| `Check-Source.py` | `ffe03dc462928c0746ba4bdea0dfcfa4d0a6e196441e962390cff91977aeb0ff` |
| `PINNING.md` | `be261f32e527efd333c66c994d5f621cccc44e2731500186edf9a76bd647b725` |

The runner was PowerShell 7 `Add-Type` against its reference assemblies.
No .NET version was re-queried. Offline Cargo was not run.

Not run: fmt, Clippy, provenance, helper or controller launch, the PowerShell
entry, native file, pipe, or Job APIs, `run_inner`, ordinary or full Rust
tests, discovery tests, account or process inspection, network, GitHub,
install, CI, push, and subagents. Builder fmt and Clippy remain builder
evidence at the matching source tree. This pass gives them no new credit.

## Next implementation slice

This acceptance does not open an edit by itself. The coordinator can assign
one source slice, with no launch authority: SP-GRAMMAR, the handle handoff
that `RequireTelemetryBinding` still refuses.

- Own the telemetry writer inside the process-attribute list and clear
  inherit before any fixture child could see it.
- Keep the handle out of the environment block and out of argv.
- Keep `Invoke-BoundedReplay.ps1` on `GATE2_ONLY`, keep
  `registration_enabled` false, and keep `remaining admission gates unproved`.
- Do not construct `OwnedSession` from pure `Run`. Do not register either
  filter. Do not treat the dormant libtest vector as accepted grammar.
- Leave duration as the reserved 0. Leave environment selection, nested-Job
  proof, and the 300/360/60 measurement unselected and unrun.

The deadline machine stays a source contract. Re-opening it needs a failed
pure check or a later authorized native run. Hardware, the historical main71
cause, and PA6 stay open.

## Close

Standards: 0 hard findings. 1 judgement, divergent change in `Controller.cs`.
Spec: 0 findings. The absolute caps include confirmation, the waiter cannot
finish success, and a blocked call remains unknown with its owner retained.
Security: 0 reachable findings. The worst dormant limit is that deleting both
refusals would still leave grammar unwired and kernel cleanup unproved.
