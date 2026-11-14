# Windows main71 native replay: dedicated telemetry review

Disposition: the Sol high dedicated-telemetry slice is **acceptable as source**.
The dormant launcher keeps one telemetry pipe whose write end is neither stdout
nor stderr, discards both raw harness streams on `NUL`, and reads the parent
end with one peek and a capped read. `emit_frame` writes only an explicit
channel. The pure serializer still has an independent literal oracle.
`RequireTelemetryBinding` throws before `CreateProcessW`. **Native run
readiness is NOT CLEARED.** This review grants no publication, PR, merge, CI
admission, registration, launch, or selected-environment provenance.

Reviewer: xAI / grok-4.7, one pass, 2026-11-14. The assignment forbids
subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
No historical retry was issued. Phase 1 still has no focused red loop that
reaches that failure. Real-Runner PA6 record/replay remains **OPEN**. Native,
installed, and dedicated paths are **unverified on real hardware**.

## Pins

Porcelain was empty on entry and empty after the pure checks. The review cwd
equaled the linked worktree top level. The Git directory is that worktree's
directory under the common Git directory. Paths below are repository-relative.
No owner path, account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-dedicated-telemetry` |
| Reviewed head | `27ac2fda63b44b78c493eaed96389c4d0f4bd1bb` |
| Head tree | `c6693cc49198bc11507f9bf3c067441cd20b48f8` |
| Head after evidence | `27ac2fd` changes `docs/PROGRESS.md` only |
| Supplied source | `2acf69b242f9c2633e9bb853ff3a76cea8231995` |
| Retained source | `5f7a99579d6f2d0ce09ad9ab4659131cc0ab04a0` |
| Source tree | `9fd79b0d353f4f86b7cb6a25697bab9ff1c633b3` (both commits) |
| Source parent | `51e4d543f11cb52b3cecf34ba51ada87d10ba351` |
| Supplied evidence | `a554471f10ba898ab45a1c60d25047a92a308960` |
| Retained evidence | `f53f87ac50c1f34d0d2c98b85cc1f8b91aa25ae9` |
| Evidence tree | `d3f3cbb2be9cc02517a21a49e53c4cfebd8cb9a4` (both commits) |
| Source patch-id | `7e34a43f2b6facc357c864c28897377317136af9` (both commits) |
| Held-manifest review | `4d4a1e3bd7de61e15f2b646fbf11ceeaa55bb8f0`, retained `036334acd1a07f543b6f4c71be76a3ac4f3015dd`, same tree `081705fe3bece2e9c2b2a1652743737b4942fe34` |
| Integration review | `984d345c71ea91dfce069c2000da2caa1f8556d4`, retained `620b3be91ee90dc957b8e03b6f0b53177b6e77a0`, same tree `f4f2751ffe109f2a4ba3d0cc3e9ea7525baee3a0` |
| Source diff | `git diff 51e4d543f11cb52b3cecf34ba51ada87d10ba351 5f7a99579d6f2d0ce09ad9ab4659131cc0ab04a0` — 6 files, 339 insertions, 58 deletions |

`git diff 2acf69b 5f7a995` and `git diff a554471 f53f87a` are empty. The
supplied source and evidence commits are not ancestors of this head. Each
retained commit has the same tree. `git diff --check` on the source diff
reported no whitespace errors. `Admission.cs`, `HeldFiles.cs`,
`selectors.json`, `Invoke-BoundedReplay.ps1`, and the helper `main` are
unchanged from the source parent. Every changed file has CR count 0.

| File | LF SHA256 | Git blob |
| --- | --- | --- |
| `crates/arxd/src/development/windows.rs` | `636e0ae8da9e0c9ed6f5914de5fbadc77099950f6825787b7338db48af1865ae` | `03ecfbb614d2bf4af2f6d0fdff45d8d67e7cf278` |
| `tools/windows-native-replay/Controller.cs` | `cd0a193dfc7da7c6c14812343f08fd9526c40b9d2d6f2bb871aa580962cc4405` | `f3325b1a7379dfb1992ba01e0e80335fc5dda809` |
| `tools/windows-native-replay/NativeJob.cs` | `7e15dbf388a128a24ae3fd39b5f680b5917ea281f588d703a685a06342cbd266` | `6f381898d02b9823762de7f561051697cf4a7d6a` |
| `tools/windows-native-replay/Check-Source.py` | `652fa0790d7783a2e7c5bf610c6ac3e45fb61bfd9e11426fdc325992dea884ec` | `81ce1cd6105ab6cf7793594bb3e63e6c794b49df` |
| `tools/windows-native-replay/Test-PureController.ps1` | `0ed3aadde962784dee601c12cfe01992bb84ac80805867a6486e87e3b0a0df11` | `ce8fabe60601cc2951b573380e3bd4155a2bb0d9` |
| `tools/windows-native-replay/PINNING.md` | `ce6fd5f98592e192ba9593d25706b046f1c3ae2ff82c15a0b7c6dcbc4a9b9ef1` | `affcf56d3dd6f8342c011893d822c0ded0783086` |

`HeldFiles.cs` remains blob `9526c04b368664ee23e6a6efbb27e554cf5b10b9`, LF
SHA256 `9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643`.
`Admission.cs` remains blob `f26127281e627d3cca76312846a18243206cd063`, LF
SHA256 `b6c1aa6b39d618a85e2183988cc95b44c965dfbb44670b2c2c4382d336754289`.
These eight rows match the evidence table. This pass recomputed them from the
file bytes with SHA256 and with `git hash-object`.

The evidence file at the source commit is 81318 bytes, SHA256
`cd95127e997946d5041adc299e3276048edd7eb7a76a7f74a33dc991f6a8a0a0`. The current
file begins with those exact bytes. Offset 39689 is `0x96`. The evidence
commit appends the telemetry record. This pass did not rewrite that prefix.

## Prior blocker disposition

| Prior item | This tree | Credit |
| --- | --- | --- |
| SP-TRANSPORT dedicated writer | One pipe write handle, absent from the parent's stdin, stdout, and stderr. Child stdout and stderr are one shared `NUL` write handle | Source shape accepted. `CreatePipe`, `CreateFileW`, and `CreateProcessW` remain UNRUN |
| Parent read | `PipeReader` peeks once and reads `min(available, 4096)`. An empty peek does not read | Policy accepted on the in-memory reader. `PeekNamedPipe` and `ReadFile` remain UNRUN |
| Raw harness bytes | The decoder is attached only to the telemetry reader. The old stdout and stderr `ReadPipe` calls are gone | Accepted as source dataflow. No child was launched |
| `emit_frame` sink | The generic writer is the only sink. `NativeWriter::write_frame` is the only `WriteFile`, and it asks for 40 bytes once | Accepted as source. The executed producer test uses `MockWriter` |
| Serializer oracle | `frame` copies ten little-endian fields. The producer test compares both literal arrays before any writer runs | Accepted. The test does not call `WriteFile` |
| Environment | `Admission.Environment` still builds `PATH`, `TEMP`, `TMP`, and `SystemRoot`. The handle is absent from that block and from argv | Accepted. The four-variable block is still not passed to a process |
| External grammar | `RequireTelemetryBinding` throws `telemetry launch binding grammar unreviewed` before `CreateProcessW` | Intentionally unresolved. The libtest vector stays unaccepted |
| Duration | The end-frame millisecond field remains the constant 0, with a reserved-field comment | Open. No runtime credit |
| Final refusal and entry | `RevalidateBoundary` still throws `remaining admission gates unproved`. The PowerShell entry still throws `GATE2_ONLY`. `registration_enabled` is false. `Run` contains no `new OwnedSession(` | Preserved |
| SP-GRAMMAR, SP-DEADLINE | Unchanged open gates | Not this slice and not closed |

## Standards

Repo rules used: `AGENTS.md` (small commits, a meaningful test per behavior,
no claimed check that was not run, hardware paths labeled unverified) and
`CONTRIBUTING.md`'s review expectation. The smell baseline is applied on top.
Tooling already covers formatting; `git diff --check` was clean, and fmt was
left as builder evidence.

No documented-standard breach. The new pure producer assertions and the five
transport mutants exercise the decidable policy. The Win32 calls stay behind
the two refusals and are labeled UNRUN in `PINNING.md`, which matches the
gate's prohibition on pipe creation. The native module remains `#[cfg(test)]`
with `#[allow(dead_code)]`. `Check-Source.py` still requires exactly five
`#[test]` functions in the pure module and zero `#[test]` or `#[ignore]`
attributes in the native body. `bounded_matrix` and `kernel_peek_capture`
each occur once in `windows.rs`.

Judgement: divergent change in `Controller.cs`. This commit adds the poll
policy and `TransportPureChecks` to the file that already owns the budget,
the launch grammar, and the admission oracles. A later split can wait. The
file's existing shape is the local convention, so this stays a smell.

## Spec

The originating contract is SP-TRANSPORT from the integration review, as
bounded again by the held-manifest review's next slice and by the dedicated
follow-up in `PINNING.md`.

The requirement is met in source.

`NativeJob.Launch` opens `NUL` for generic read and a second `NUL` for
generic write. `STARTUPINFO` output and error both receive the write handle.
The attribute list is `PROC_THREAD_ATTRIBUTE_HANDLE_LIST` (`0x20002`) with
exactly three values: the `NUL` input, the shared `NUL` discard handle, and
`telemetryWrite`. Creation flags remain `0x80404` (suspended, Unicode
environment, extended startup). The pipe's read end is cleared with
`SetHandleInformation(read, 1, 0)` and is absent from the list. The job
handle is absent too. The parent's write copy is disposed in `finally` on
the success path and on the binding throw, before the caller can observe
EOF. `Next` and `Dispose` close the reader the object still owns.

`PumpTelemetry` is a poll. It peeks once. Available 0 returns without a
read. Otherwise it reads at most 4096 bytes and rejects a 0-byte or
over-size result. `PipeReader` maps Win32 error 109 to EOF and forces the
available count to 0 on that failure; every other peek or read failure calls
`TransportFailure`. There is no read callback and no overlapped completion.
`OwnedSession.Run` still pumps that one decoder across kernel segment 2 and
matrix segment 70. The pure session drives the same decoder through a
fragmented kernel reader and a fragmented matrix reader, then rejects a
missing body, a 79-byte partial EOF, a bad magic byte, a wrong index, a
duplicate begin flag, peek and read errors, a zero read, an over-reported
read, a 70-event overflow, and trailing bytes after the segment is closed.

`frame` only writes little-endian integers into a 40-byte array. `emit_frame`
calls `write_frame` once and latches failure unless the result is `Some(40)`.
A latched channel does not call the writer again. `NativeWriter::from_inherited`
is the only constructor used by the fixture API. It rejects a null handle,
`INVALID_HANDLE_VALUE`, the three standard handles, and a non-pipe, requires
the inherit bit, then clears that bit before returning. `one_row` and
`kernel_peek_capture` receive `Telemetry<NativeWriter>`, so that clear is
already done before `prepared_execution` or `run_inner`. Nothing in the
module calls `from_inherited`. No environment key, argument, CLI filter, or
discovery entry carries the handle value. The direct child is the only
process the allowlist would include, and `CreateProcessW` is unreachable
while the binding method throws.

The constant 0 remains the last observed field. `Budget.Accept` still allows
an end frame whose `durationMs` is 0. The session clock is untouched, and
this review gives that constant no runtime credit. The 300/360/60 budget
stays unmeasured.

No requirement from this slice is missing, partial, or implemented in a
form the spec rejects. Grammar delivery and the deadline waiter were
explicitly left open.

## Security

**0 findings reachable from the current entries.**

`Invoke-BoundedReplay.ps1` throws `GATE2_ONLY` before `Add-Type`. Parameter
validation cannot enable execution. `PureControllerChecks.Run` does not
construct `OwnedSession`, `NativeJob`, or `HeldFiles`, and it does not call
`Launch`. Compiling the imports does not call them. `RevalidateBoundary`
still throws after the pin reads. `Launch` throws before `CreateProcessW`
even if that admission throw were removed. `selectors.json` keeps
`registration_enabled` false. The producer test's call graph is `frame`,
`emit_frame::<MockWriter>`, and `MockWriter::write_frame`.

Removing both throws would still leave the child without a reviewed way to
receive the typed handle, the deadline waiter unbuilt, and every native
open unexecuted. Do not remove either throw as a clearance. No account,
service, VM, owner profile, or Lab Manager state was inspected.

## Failure ledger

No three-attempt handoff trigger.

The evidence note records one test-oracle correction: `transport-short`
survived a run because a later malformed frame still failed the stream. The
oracle now requires that over-reported read to fail immediately and leave
the queue empty. This pass killed that mutant with the others. That is one
correction of a test, not a third failed repair of one production defect.
The 67 assertion kills are negative controls.

Historical main71 run `43545244266` stays a separate unknown cause. This
review issues no retry.

## Advisories

- `PINNING.md` item 5 still says the row channel is not an IPC adapter. The
  dedicated follow-up is the section that matches this source. A later docs
  edit can replace the stale sentence. It is not a source repair.
- `Launch` refuses reuse when `process` is already set. A failed attempt
  leaves `process` null, so a second `Launch` would replace `telemetry`
  without disposing the first reader. `Run` does not retry. `Dispose` closes
  the reader that remains referenced. The binding throw still prevents a
  process from being created.

## Checks this pass actually ran

`Run` was read before execution. It reaches in-memory JSON, SHA256 of caller
bytes, synthetic clocks, synthetic frames, and `TransportPureChecks`. The
transport reader is an array with scripted peek and read results. The path
does not reach `NtCreateFile`, `CreatePipe`, `PeekNamedPipe`, `ReadFile`,
`WriteFile`, `CreateProcessW`, or `Stopwatch.StartNew`. `Stopwatch.StartNew`
occurs only inside the `OwnedSession` constructor. The producer test was
read the same way. It calls `frame` and `emit_frame` with `MockWriter`. It
does not call `from_inherited`, `NativeWriter::write_frame`, `one_row`,
`prepared_execution`, `run_inner`, `std::fs`, `std::process`, or `Instant`.

```text
python tools/windows-native-replay/Check-Source.py
```

Passed. Inventory reported 28 oracle rows, five pure tests, zero native
registrations or callers, selectors disabled, binding refused, and the
dedicated telemetry inventory line.

```text
pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1
```

Baseline passed. All 67 mutants were assertion-killed. The printed LF SHA256
lines match the evidence block, including `transport-empty`, `transport-cap`,
`transport-read`, `transport-short`, and `transport-eof`. Controller,
NativeJob, Admission, HeldFiles, and the runner LF SHA256 values were
unchanged. The runner was PowerShell 7 `Add-Type` against its reference
assemblies. No .NET version was re-queried.

With `CARGO_PROFILE_DEV_DEBUG=0`, locked, offline, target
`x86_64-pc-windows-msvc`:

```text
cargo test -p arxd --lib development::windows::native_replay::pure::producer_bytes_have_independent_literal_oracle -- --exact --test-threads=1
```

Result: 1 passed, 0 failed, 0 ignored, 366 filtered out, harness 0.00s.
The test binary was `arxd-5547724799c07633.exe`. Cargo reported an
unoptimized test profile and finished the compile in 52.04s. The harness
0.00s is not a row duration and receives no budget credit. `rustc -Vv` was
not run. The other four pure tests were read and not executed.

Not run: fmt, Clippy, provenance, helper or controller launch, the
PowerShell entry, native file, pipe, or Job APIs, `run_inner`, ordinary or
full Rust tests, account or process inspection, network, GitHub, install,
CI, push, and subagents. Builder fmt and Clippy remain builder evidence at
the matching source tree.

## Next implementation slice

Sol high, one slice, no launch authority. Do not reopen the telemetry
contract unless a later authorized run disproves it. Do not treat the
dormant libtest vector as accepted grammar, and do not pass the handle
through the environment or argv.

Supervisor deadlines, the prior SP-DEADLINE slice:

- Start termination early enough that confirmation ends at the row deadline.
  The extra two seconds after `rowStart+10000` is still outside the cap.
- A waiter may arm `TerminateJobObject` while a synchronous launch call is
  blocked. That waiter may only latch failure. It must not be able to report
  success.
- Keep `Invoke-BoundedReplay.ps1` on `GATE2_ONLY`, keep
  `registration_enabled` false, and keep `RequireTelemetryBinding` in front
  of `CreateProcessW`.
- Do not construct `OwnedSession` from `Run`. Do not remove
  `remaining admission gates unproved`. Do not register either filter.

Handle delivery stays the separate SP-GRAMMAR disposition. Environment
selection, nested-Job proof, budget measurement, hardware, the historical
main71 cause, and PA6 stay open.

## Close

Standards: 0 hard findings. 1 judgement, divergent change in `Controller.cs`.
Spec: 0 findings. The slice separates the frame channel from `NUL` stdout
and stderr, keeps the literal serializer pure, and still refuses launch.
Security: 0 reachable findings. The worst dormant limit is that deleting
both refusals would still leave handle delivery, deadlines, and unexecuted
native opens.
**Acceptable source may stay local. Native run readiness is NOT CLEARED.**

Performed: read-only Git inspection, LF SHA256 and Git-blob recomputation,
evidence-prefix comparison, static reading of the six-file delta and the
pure call graph, `Check-Source.py`, one managed pure run with 67 assertion
mutants whose stdout matched the evidence block, one exact Rust producer
filter, and this file's write and commit. Not performed: PROGRESS, source,
design, or evidence edits; native execution; provenance preparation; launch;
registration; publication.
