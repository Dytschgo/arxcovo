# Windows main71 native replay: Gate 2 Grok source review

Disposition: the dormant Rust fixture matches the accepted Gate 2 contract.
The controller run contract does not. **Run-readiness is NOT CLEARED.**
No native execution, registration, publication, or budget admission follows
from this review.

Reviewer: xAI / grok-4.7, one independent pass, 2026-11-14. The assignment
forbids subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
Debug Phases 2–6 were not entered: there is still no focused red loop that
reaches that failure, and this task forbids building one with a native run.
Real-Runner PA6 record/replay remains **OPEN**. Native, installed, and
dedicated paths are **unverified on real hardware**. No retry was issued.

## Pins

Initial porcelain was empty. The review cwd was the linked worktree
`<isolated-worktree>`. Its Git directory is a per-worktree directory under the
common Git directory. Paths below are repository-relative.

| Item | Identity |
| --- | --- |
| Branch | `t3/windows-gate2-review` |
| Reviewed head | `a04aaf68dc25cf7cbe8a5a22a5db08638fc1505c` |
| Head tree | `9f65e6643b6485eff648cbe72918bed87fe35921` |
| Evidence commit retained | `afcf804740e1ed3d4f4892ed82224b0b70e70ee8` has the same tree |
| Source commit | `beb590c4e40f225b388bcbebb5d754e8ff0a1369` |
| Source tree retained | `3eedb7736e8876dc1d8907a0d0bfdec3f06ca5ba` has the same tree `8ee82d5639eed05f4cc39f4756702530aed53f12` |
| Actual main / review base | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` |
| Diff scope | `git diff aa49ce1e99475d07d63c6b0d23fa94bdb30bab74 a04aaf68dc25cf7cbe8a5a22a5db08638fc1505c` — 15 files. Windows source numstat is 919 insertions and 3 deletions |
| Windows baseline blob | `4cb039f4742568a3eb3a124ae3626ac9511b8be2` at `aa49ce1` |
| Windows head blob | `6ee36f7b9948429d76d81a8e853477b9581beddd`. 102568 bytes, 2966 LF, CR count 0. SHA256 `581d03e2f360a217b27218c2115e56085d80bc42ae03cf69bce1de58d504e648` |
| Accepted proposal blob | `366876f711595c14bde42763e2a263cfe44ca9c2` |
| Grok `b0b4581` review blob | `7a532db7d96ebb34f34851ea0293c60e7beefe60`, identical at retained `d63ae19`. Those commits differ only in `docs/PROGRESS.md` |
| Grok `b2676a4` review blob | `0d2ce8b6839c40544981a820660a8c158c4d49c3`, identical at retained `0872975` |
| Grok `ab115ba` review blob | `56b0c3d1b12f05001acc2e3051392a5b33ec68d7`, identical at retained `42e7946` |

The other ten evidence SHA256 values match the working-tree bytes at this
head: root `Cargo.lock`, `rust-toolchain.toml`, and the eight files under
`tools/windows-native-replay/` named in `docs/evidence/windows-main71-native-gate2.md`.
Their Git blobs match that evidence table. This pass did not rebuild or hash
the helper executable or the libtest executable, so those two builder hashes
stay builder evidence.

Nine source paths reviewed: `crates/arxd/src/development/windows.rs`;
`tools/windows-native-replay/Controller.cs`;
`Invoke-BoundedReplay.ps1`; `Prepare-Provenance.py`; `Check-Source.py`;
`selectors.json`; `child/Cargo.toml`; `child/Cargo.lock`; `child/src/main.rs`.

Docs read: `AGENTS.md`, full `docs/KICKOFF.md`, `CONTRIBUTING.md`, the Review
and Debug skills, the active review profile, the full accepted proposal, the
three Grok reviews above, the Gate 2 evidence note, and `docs/PROGRESS.md`.
No source, proposal, PROGRESS, workflow, or CI edit was made.

## Standards

**PASS. No hard finding.** One baseline judgement, not a blocker.

The Rust change keeps the new native module, the helper, and the controller
out of release discovery. `diagnostic!` and `stage!` expand to empty under
`not(test)`. The helper manifest is `publish = false`, `test = false`,
`bench = false`, and is not a root workspace member. Root members remain
`crates/arxd` and `crates/arx-guest`. The helper body is `fn main() {}`.
The PowerShell entry throws `GATE2_ONLY` before any process, Cargo, or
`Add-Type` call. Comments and the evidence note describe compilation and
pure checks as preparation, and they do not claim a native pass, a
reproduction, or a hardware result.

Repo rules checked: conventional scoped docs, no credentials, no owner
paths, no product approval flow, and no claim for a check this source did
not run. The three-line product cleanup edit still calls `TerminateProcess`
and then `job.stop()` before quarantine. Release builds do not evaluate the
new fixture arguments.

Fowler baseline: **Duplicated Code**, judgement only. `ReplayController.Budget.Accept`
and `PureControllerChecks.Run` each encode the same index formula for create,
resume, and consumption counts. A documented repo rule does not require that
formula to be shared. The Spec finding below is the schedule disagreement
with the Rust matrix, which this smell does not replace.

## Spec

**1 finding. The 28-row literal oracle matches the accepted proposal. The
controller index schedule does not match the Rust matrix.**

Checked, and not the finding:

- `ORACLE` is a handwritten 28-row table. It is not produced from
  `Stage::ALL`, `resolve`, a captured record, or the fixture selector.
  `Stage::ALL` places child-token stages next to the daemon stages and places
  `Wait` and `ExitCode` last. The literal table places child-token rows after
  `assign_job`, and places `wait` and `exit_code` before the four final rows.
- Rows 0–17, `daemon_token_open` through `resume`, store pass, stdout, and
  stderr as `None`. That is missing, and it is distinct from numeric zero.
- `loop_out_peek` and `loop_out_read`: pass `None`, totals `Some(0)` / `Some(0)`.
  `loop_err_peek` and `loop_err_read`: pass `None`, totals `Some(1)` / `Some(0)`.
  `wait` and `exit_code`: pass `None`, totals `Some(0)` / `Some(0)`. `wait`
  stores `Some(4294967295)`. `exit_code` stores wait `None`.
  Final out peek and read: pass `Some(0)`, totals `Some(0)` / `Some(0)`.
  Final err peek and read: pass `Some(0)`, totals `Some(1)` / `Some(0)`.
- Create counts are 0 for the first 14 rows and 1 afterward. Resume counts
  are 0 through `resume` and 1 for the ten resumed error rows. `attr_size`
  is the only `valid: false` row. Error 55 and the stage variant are fixed
  per row. Labels match `Stage::label`.
- `drain_at!` records totals before the drain call. `capture` copies that
  snapshot. A loop or final failure therefore records the pre-read totals.
  `Invocation::enter` starts pass and totals at `None`, so the eighteen
  pre-loop rows keep missing rather than `Some(0)`.
- Each `one_row` builds a new `Execution::build(settings, false)`, asserts
  `policy_fixture` is false, calls `run_inner` directly inside `diag::observe`,
  and passes fixture `false`. Arguments are empty. `PATH` must equal the
  single `workspace/bin` directory, whose only entry must be `cargo.exe`.
  The helper is hashed with a bounded 16 MiB read on pins held for the row.
- Consumption increments inside `armed.take`. `Invocation::drop` clears armed
  state and, while a `Scope` is active, leaves `consumed`, `resumes`, and
  `calls` in place. `observe` copies them before `Scope` clears TLS.
  `resume_call` sits after `stage!(Resume)` and before `ResumeThread`.
  `create_call` sits after `stage!(CreateProcess)` and before `CreateProcessW`.
  An injected error returns from `stage!` before the matching counter call.
- Duplicates are taken after `UpdateProcThreadAttribute` seals the
  NUL/stdout/stderr list and before `drop(out_w)` / `drop(err_w)`.
  `DuplicateHandle` is called with inherit false. Empty rows retain no
  writers. Loop prefill writes one `X` to each retained writer before the
  first drain pair. Final prefill is reached only after `job.stop()` has
  returned true, and the helper refuses the write unless that result was
  recorded. Peek 109 is consumed and then skipped, so it does not become
  `ExecutionFailed`.
- `bounded_matrix` and `kernel_peek_capture` have no `#[test]` or `#[ignore]`
  attribute and no caller. Four new pure tests are the only new registered
  tests. The pure module's source does not call `run_inner`, process, clock,
  or filesystem APIs.

### ST-INDEX

`Budget.Accept` treats index 0 as the kernel capture: create 0, resume 0,
consumed 0. It then expects 28 failures at indexes 1–28, four peek-109 rows
at 29–32, and two controls at 33–34.

`bounded_matrix` numbers the 28 failures from 0 through 27, the four peek-109
rows from 28 through 31, and the controls as 32 and 33. `kernel_peek_capture`
has no index. No transport currently connects these numbers. Joining them by
identity would reject a true kernel-prefixed stream, or, if the budget were
relaxed to the Rust numbers, it would drop the kernel row. The repair has to
introduce one reviewed map. The pure managed checker locks only the C# side.

The known open Gate 2 gaps are also unmet, and they are completion work
rather than a second reading of the oracle: immutable launch pins, a
nonblocking row channel, a selected disposable environment, a nested-Job
trial, and a measured 300 s + 360 s + 60 s budget. `selectors.json` records
`registration_enabled: false` and the 35 / 20 / 16 totals, and neither the
PowerShell entry nor `RunPinned` reads that file.

## Security

**3 findings. All three block any native run. None is a release-product
fence change.** `RunPinned` has no caller. The PowerShell entry cannot reach
it. The findings are holes in the dormant launcher, which must not be started
as written.

### SE-DEADLINE

`RunPinned` polls `budget.Expired(global.ElapsedMilliseconds)` and treats ten
seconds without an event as failure. Both clocks come from a caller-supplied
`Stopwatch` and from `tryReadOwnedEvent()`. An unstarted stopwatch stays at
0 ms, so neither the 10 s row limit nor the 360 s limit becomes true, and
the loop continues until the process exits. A callback that blocks postpones
`TerminateJobObject` for the length of that block. `Thread.Sleep(1)` does
not create an external deadline. The pure checker calls `Expired(10000)` and
then `Expired(360000)` on the same failed budget, so the second call passes
because failure is already latched. That checker never calls `RunPinned`.

Repair before any call: `RunPinned` starts and owns one clock, uses it for
both filters, and a blocking read cannot delay the kill of the owned job.

### SE-COMPLETE

After the harness handle is signaled, exit code 0 and `ActiveProcesses == 0`
make `RunPinned` return true. `Budget.Complete` is not part of that result.
A process that exits 0 before 35 accepted rows, or during an open begin
event, is reported as contained success. The `finally` block kills only when
`confirmed` is still false, so this early success also skips that kill.

Repair before any call: success requires exit code 0, active processes 0,
and `Budget.Complete`. Every other return stops the remaining matrix, keeps
containment until retirement, and forbids reuse of the environment. No retry.

### SE-INHERIT

`CreateProcessW` is called with `lpEnvironment` null, so the harness inherits
the controller process environment. The fixture contract allows only `PATH`,
`TEMP`, `TMP`, and `SystemRoot`, with `PATH` equal to the one hashed helper
directory. Inheritance can carry the controller account's other variables
into the test process. The same call takes its executable, command line, and
working directory from unbound arguments. Handle inheritance is false, and
the creation flags do not include breakaway. Those two choices do not
constrain the environment block.

Repair before any call: pass an explicit environment block, and bind the
three launch strings to the two literal Cargo filters and the pinned paths.

Checked, and not filed as further findings:

- The outer job is unnamed. Its limit flag in this dormant source is
  kill-on-close only. Breakaway is not set. The process is created suspended,
  assigned, and then resumed. Failure termination uses the owned process and
  job handles. There is no PID, name, or process-list kill.
- The Rust observer borrows the original process and job handles, uses a
  zero timeout, and requires the recorded stop result. A still-running
  assign child fails that check. The product assign path still uses the
  original outer `TerminateProcess` because `stage!` returns before the
  inner terminate. Adding a wait or a retry here would change the reviewed
  budget.
- Non-fixture test runs call `retain_if_active` and receive no duplicate
  when the fixture guard is inactive. Non-test builds omit the module.
- `parse_case` accepts eight numeric fields, rejects a ninth token, and
  rejects the private-path sentinel covered by the pure test. It does not
  accept raw diagnostic lines.
- `Prepare-Provenance.py` refuses a reparse ancestor or leaf on its fixed
  relative paths and prints a fixed failure string. Its commit argument is
  a claim. `runtime_authority` is false. It does not hold the pins across
  the hash read. That is sufficient for this preparation utility and
  insufficient for a later authorization pin.
- `Check-Source.py` is a text inventory. It is not a call-graph proof, and
  the evidence note already says so.

Nested outer and inner Job compatibility was not executed. No disposable
standard-account environment is selected. No account, service, VM, owner
profile, or Lab Manager state was inspected.

## Required completion before any run

This list is the repair contract. Completing it needs a new source review
and a separate native-run disposition. This file grants neither.

1. Close SE-DEADLINE, SE-COMPLETE, and SE-INHERIT as specified above.
2. Publish one reviewed index map for the kernel row, the 28 failures, the
   four peek-109 rows, and the two controls. Feed `Budget` from actual row
   events. Reject a missing, duplicate, or malformed event.
3. Add the owned nonblocking channel the evidence note already requires.
   The channel reports begin and end for each row and preserves one global
   deadline across both filters.
4. Select one fresh disposable standard, unelevated environment. Prove the
   outer kill-on-close job contains the harness and the suspended helper
   before the inner job assignment, without breakaway and without weakening
   the existing inner limits. If that containment is unavailable, native
   execution stays blocked.
5. Pin the reviewed source, helper, test executable, selector manifest, and
   report. Hold reparse-free ancestor and leaf handles for the run. Do not
   treat `Prepare-Provenance.py` or a branch name as that pin. Discard raw
   Cargo output; a failed path assertion can stringify a fixture path.
6. Keep the assign-row observer on its zero timeout. A live unassigned
   process is unknown cleanup and stops the matrix.
7. Measure the non-native share and the native envelope before any
   ordinary-discovery or CI admission. The 300 s + 360 s + 60 s split is
   not measured. Historical durations are not that measurement.
8. Leave native marker, prefill, create-counter, and suspended-resume
   mutants unrun until a separate reviewed budget names them. The four
   compiled pure mutants in the builder evidence are not those mutants.

## Blockers

- **Run blocker SE-DEADLINE.** The dormant launcher's deadline is not owned
  by the function that would have to enforce it.
- **Run blocker SE-COMPLETE.** Contained success is reported without the
  35-row budget.
- **Run blocker SE-INHERIT.** The harness would inherit the controller
  environment, and its command is unbound.
- **Integration blocker ST-INDEX.** The C# and Rust case indexes disagree.
- **Open gates, still blocking authority:** no selected environment, no
  nested-Job result, no measured budget, no immutable launch manifest, and
  no nonblocking telemetry. Native functions remain undiscoverable and
  unexecuted.

No release fence, registration, or product cleanup decision is a blocker.
The preparation can stay on this unpublished head. It cannot be launched.

## Evidence from this pass

Compiler: rustc 1.95.0, commit `59807616e1fa2540724bfbac14d7976d7e4a3860`,
LLVM 22.1.2. Cargo 1.95.0 (`f2d3ce0bd` 2026-03-21). Target
`x86_64-pc-windows-msvc`. `CARGO_PROFILE_DEV_DEBUG=0`. Locked and offline.
No install and no network.

```powershell
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure:: -- --test-threads=1
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::native_replay::pure:: -- --test-threads=1
```

The first filter passed 6 tests, with 360 filtered out. The second passed
4 tests, with 362 filtered out. Both reports were 0 failed and 0 ignored.
The harness filename was `target/x86_64-pc-windows-msvc/debug/deps/arxd-5547724799c07633.exe`.
Those 10 tests were inspected first: their transitive calls stay in TLS,
literal tables, formatting, and the numeric parser. They do not call Win32,
a clock, or a process. The compile of that test target is the compile this
pass performed. The helper was not built or started here.

`PureControllerChecks.Run()` was loaded with in-memory `Add-Type` and
passed. The call uses `Budget` and `Event` only. `RunPinned`, its P/Invoke
imports, `Stopwatch`, and process APIs were not called.

Builder evidence, not re-executed here: the four pure assertion mutants,
helper SHA256, libtest SHA256, `Check-Source.py`, `Prepare-Provenance.py`,
fmt, and Clippy. Native capture, the 35-case matrix, cleanup, nested jobs,
and native mutants are **UNRUN**.

## Close

Standards: 0 hard findings; 1 judgement, duplicated controller index formula.
Spec: 1 finding, ST-INDEX, the two case schedules disagree.
Security: 3 findings; the worst is SE-DEADLINE, an unowned clock can wait
without killing the job.
**Run-readiness is NOT CLEARED.**

Performed: read-only Git inspection, SHA256 of the eleven pinned source
files, the two pure Rust filters, one managed pure budget run, and this
file's write and commit. Not performed: source, proposal, or PROGRESS edits;
helper, controller, or PowerShell execution; native functions; Windows API
experiments; child launches; ordinary tests; full suite; owner, account,
service, VM, or private-state inspection; network; GitHub; install; CI;
push; subagents.
