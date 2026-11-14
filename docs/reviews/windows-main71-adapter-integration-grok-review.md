# Windows main71 native replay: adapter integration review

Disposition: the Sol high three-integration source is **retained and incomplete**.
Admission deliberately refuses, the PowerShell entry still throws `GATE2_ONLY`,
and `registration_enabled` remains false. Relative held opens, 128-bit identity,
a post-hash link recheck, a closed manifest parser, and dormant Job/frame code
are real source. They are not exclusive helper-directory membership, not a
bound manifest, not a dedicated frame transport, and not a deadline supervisor.
**Native run readiness is NOT CLEARED.** This review grants no publication,
PR, CI admission, registration, or launch.

Reviewer: xAI / grok-4.7, one independent pass, 2026-11-14. The assignment
forbids subagents, so Standards and Spec/security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
No historical retry was issued. Phase 1 still has no focused red loop that
reaches that failure. Real-Runner PA6 record/replay remains **OPEN**. Native,
installed, and dedicated paths are **unverified on real hardware**.

## Pins

Initial porcelain was empty. It was empty again after the pure checks below.
The review cwd equalled the Git top level. The Git directory is a per-worktree
directory under the common Git directory. Paths below are repository-relative.
No owner path, account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-native-replay-adapter` |
| Reviewed head | `bd79550f1dd654d3c6ca5973377b6319a595b331` |
| Head tree | `4dada073c57755730cc12c825bae01e8a3f17000` |
| Source commit | `34a5622d9a38dc4bd3276821978d23562fd8266e` |
| Source tree | `078a4f26923e4e2e197eb2579fedacca4ab50dc9` |
| Supplied source | `eac5a6d2006d5b3242d3ea5460a049755cc071aa`, same tree, not an ancestor of this head |
| Evidence commit | `50cbf0b4f4e8addde0a2d09f6ffe57da0250e49d` |
| Evidence tree | `98d46634d51682621318c57bf697db085e74cab4` |
| Supplied evidence | `188a5c9c611ed76ed5efeb12a6ff8ec1f1255749`, same tree |
| Parent before the source | `ded6dbc716b2991b294f07384655b29c396edbe5` |
| Accepted proposal blob | `366876f711595c14bde42763e2a263cfe44ca9c2`, unchanged |
| Source diff | `git diff ded6dbc716b2991b294f07384655b29c396edbe5 34a5622d9a38dc4bd3276821978d23562fd8266e` — 10 files, 585 insertions, 74 deletions |
| Head after that source | `50cbf0b` adds the evidence note; `bd79550` adds two PROGRESS lines only |

`git diff eac5a6d 34a5622` and `git diff 188a5c9 50cbf0b` are empty.
`git diff --check` on the source diff reported no whitespace errors.
This pass recomputed the 13 evidence blob and LF SHA256 rows, including both
lockfiles. Every blob matched `34a5622`, every LF SHA256 matched the evidence
table, and every CR count was 0. `selectors.json` still has
`registration_enabled` false. `Invoke-BoundedReplay.ps1` still throws
`GATE2_ONLY` at its first executable statement.

## Prior blocker disposition

| Prior item | This tree | Credit |
| --- | --- | --- |
| SE-HOLD path open | Leaves and ancestors below the volume root use relative `NtCreateFile`. The volume root is opened with full share and closed after the first child | Narrowed in source. The transient full-share root open is unrun. Directory membership is still unproven |
| SE-FILEID | `GetFileInformationByHandleEx` class 18 is required. A zero 128-bit id is refused | Source only. The caller still supplies the volume serial. Byte order is not an external oracle. UNRUN |
| SE-NLINK-AFTER | `links==1` is required before and after the hash. `CopyPinned` and the nlink-2 fallback are gone | Pure `LinksUnchanged` mutant killed. Native metadata calls UNRUN |
| Manifest parser | Closed keys, external SHA256, no root self-hash field, fixed toolchain strings | Parser only. See SP-BIND and SP-SYSROOT |
| Frames and Job | `cfg(test)` code can emit 40-byte frames. `NativeJob` assigns before resume and sets kill-on-close | Dormant. stderr is not a dedicated transport. No test constructs `NativeJob` |
| Open gates | Environment, nested-Job proof, measured 300/360/60 | Still open. Not implementation defects and not cleared by this source |

## Standards

**1 hard finding.** Baseline applied. One judgement, not a blocker.

**ST-UNTESTED (hard, AGENTS.md: "Every behavior needs a meaningful test").**
The new refusal and the new supervisor are invisible to the suite that was run.
`PureControllerChecks.Run` calls `Admission.Parse` on bytes it just hashed. It
never constructs `Admission`, `HeldFiles`, `OwnedSession`, or `NativeJob`.
The 21 assertion mutants likewise never call `Revalidate`, `Launch`,
`KillAndConfirm`, or `emit_frame`. Deleting the throw at `Admission.cs:93`,
pointing `Pump` at stderr, or waiting past the row cap would leave this suite
green. `Check-Source.py` only asserts that the throw string is present. This
pass read that script and did not execute it. A string check would not prove
directory membership.

Repo rules that hold: the source diff is a conventional preparation commit;
no credentials or owner paths; no product approval flow; no workflow, ADR, or
root Cargo edit. `PINNING.md` and the evidence note call the adapter blocked
and label native behavior UNRUN. The Rust edit stays in `#[cfg(test)]` and
adds no `#[test]` or `#[ignore]` to `bounded_matrix` or `kernel_peek_capture`.

Fowler baseline: **Divergent Change**, judgement only. `Controller.cs` still
changes for the wire decoder, the session loop, and the pure suite in one
file. `NativeJob.cs` and `Admission.cs` are the right split for the Job and
the parser. The accepted contract requires those seams, so this is not a hard
naming or structure violation. The old `OwnedSession` naming judgement is
retired: the type now has a `NativeJob` field. That field is not reachable
from the entry point.

## Spec

**6 findings.** The diff does not pretend to be a finished adapter. Several
required behaviors are present but wrong, not merely absent.

Checked against accepted proposal blob `366876f7` and `PINNING.md`.

### SP-EXCLUDE — directory exclusion is possible, and this tree does not do it

`PINNING.md` item 2: "Pin directory membership so another executable cannot
be inserted. Refuse filesystems where these guarantees cannot be established."
The held-pin review required the same proof before any launch.

There is no Windows contradiction that makes that contract impossible.
A directory handle opened with share `FILE_SHARE_READ` only, and without
`FILE_SHARE_WRITE` or `FILE_SHARE_DELETE`, is checked by the kernel share
mask that includes `FILE_ADD_FILE` (`FILE_WRITE_DATA`). A later open of that
same directory object for create or append fails while the handle is held,
provided this open itself succeeded. That does not enumerate names, and it
does not freeze a child this process does not hold. `FILE_DELETE_CHILD` and
`FILE_WRITE_ATTRIBUTES` are outside that share mask. An existing sibling can
still be opened by its own path and rewritten.

`HeldFiles.Child` already uses share `1` for directories. After
`ReadPinned` of `helper\cargo.exe`, the helper directory is one retained
ancestor. `Revalidate` then throws `helper directory exclusion unproved`.
That throw is an honest placeholder. It is not a proof, and the held ancestor
is not one either: nothing calls `NtQueryDirectoryFile`, so a DLL that was
already in the directory remains and is not held.

Minimal contract, which the next slice must implement in source rather than
replace with another throw or a path listing:

1. Open the helper directory once, relative to its held parent, with
   `FILE_LIST_DIRECTORY|SYNCHRONIZE`, share read only, and
   `FILE_DIRECTORY_FILE|FILE_OPEN_REPARSE_POINT`. A sharing failure refuses.
2. Query that handle to exhaustion, restart the scan, and require the two
   name/file-id/reparse sets to match. Accept only `.`, `..`, and `cargo.exe`.
3. Open `cargo.exe` from that handle, share read only, and require the
   external file id, `links==1` before and after the hash, and the external
   SHA256 read from the handle. Hold both handles until the owned process
   has exited and the job active count is 0.
4. Do the same for the directory that contains the launched image. The
   loader searches that directory before `PATH`. Helper-directory exclusion
   alone does not cover a DLL beside `libtest`. Either that directory's
   allowlist is the one pinned image, or the image lives in the helper
   directory and the allowlist is exactly the pinned names. Choose one in
   source and in the pure oracle.
5. If `FileIdInfo` or a complete scan is unavailable, refuse. Do not fall
   back to `FindFirstFile` or `Directory.Enumerate`.

`Revalidate` must keep failing closed until that function rejects every
illegal snapshot. Removing the throw, or accepting a caller-supplied `true`,
is not the repair.

### SP-SYSROOT — a real SystemRoot cannot pass the parser

`PINNING.md` item 4 requires `SystemRoot` to be the pinned approved Windows
directory. `Admission.Inside` requires `helper_directory`, `temp`,
`system_root`, and every pin path to start with `root + "\\"`. `Absolute`
rejects a path shorter than 4 characters, so root `C:\` is illegal. The only
backslash-terminated prefix of `C:\Windows` is `C:\`. No legal disposable
root is a strict parent of `C:\Windows`, and `Inside` also rejects a path
equal to the root. A real SystemRoot and a disposable workspace root cannot
both pass.

`ManifestChecks` freezes the contradiction: it accepts
`system_root` `C:\fixture\Windows` under root `C:\fixture`. A later correct
pin of `C:\Windows` would fail this test. The repair is a parser change:
SystemRoot is its own absolute, non-following, held directory pin and is not
required to lie under the disposable root. Source, helper, temp, and artifact
pins stay inside that root. This is a contract bug in source, not an
owner-only environment choice. No environment is selected here.

### SP-BIND — hashes and blob ids are caller claims

`PINNING.md` item 1 requires a fixed `run-manifest.json` whose SHA256 is
pinned outside the manifest, the reviewed commit, Git blobs plus LF SHA256
for every `Prepare-Provenance.py` source, and the artifact roles. The
provenance utility "remains only a bounded preparation snapshot, never a
runtime pin."

`Admission.Parse` does compare bytes with an external SHA256 and does reject
an extra root `sha256` field. The constructor then takes `cleanCheckout` as
a bool and `reviewedBlobs` as a dictionary. `Need(cleanCheckout)` accepts
the argument `true`. Source blob ids are compared with that dictionary, not
with `SHA1("blob {length}\0" + held bytes)`. `workspace_lock` and
`helper_lock` are not required to be the held `Cargo.lock` bytes.
`provenance` is not parsed, so it is not required to repeat those hashes or
to keep `runtime_authority` false. The manifest bytes are not read through
`HeldFiles`. `IdText` prints the first `FILE_ID_INFO` qword and then the
second, but no oracle fixes that order. The pure test hashes the JSON it
just built. That is a parser check. It is not admission.

### SP-TRANSPORT — frames share stderr with the harness

`PINNING.md` item 5: capture raw harness output separately and do not feed
it to the channel. `emit_frame` writes the 40-byte record with `WriteFile`
on `GetStdHandle(STD_ERROR_HANDLE)`. `NativeJob.Pump` decodes stderr and
discards stdout. Libtest text, a panic, and a frame are the same byte
stream. Invalid bytes fail closed. Bytes that happen to match the grammar
are accepted as rows. The end frame's duration field is the constant 0.
The controller clock is the deadline authority, and `Budget` does not compare
the frame's duration with that clock, so a 0 here is not a false pass by
itself. It is also not an observed duration.

The smallest transport is one inherited write handle that is neither stdout
nor stderr. The parent reads it with `PeekNamedPipe` and a bounded `ReadFile`.
Stdout and stderr are drained and discarded. `emit_frame` writes only the
telemetry handle. Do not put the handle in the four-value environment block.
The pure `frame()` oracle stays the byte oracle and must not call `WriteFile`.

### SP-GRAMMAR — direct libtest is an unaccepted command change

The accepted proposal's later-authorization commands are `cargo test` with
the two filters. `PINNING.md` item 3 still requires the 12-element vector and
Kernel then Matrix, once each. This diff's follow-up switches the dormant
launcher to `libtest` plus three arguments: the filter, `--exact`, and
`--test-threads=1`. `LaunchSpec.Valid` accepts either filter on every call.
`OwnedSession.Run` is the only once-each loop, and it is unreachable because
`Admission` cannot be constructed.

The direction avoids a cargo rebuild under the four-value environment. It is
not accepted run grammar. Both functions are still undiscoverable, so this
command selects zero tests. The proposal says zero tests selected is a
failure. Do not register either function in the next slice. Registration
remains a separate scope decision.

### SP-DEADLINE — row confirmation sits outside the 10s cap

The accepted proposal: "No extra two-second wait may be added outside the
row deadline." Per-case expiry is failure. "Synchronous Win32 calls are not
assumed mathematically interruptible." `PINNING.md` item 6 requires the owned
Job to be terminated on a deadline before unknown cleanup is returned, and
success requires `Budget.Complete`, no pending events, harness exit 0, and
job active 0.

`OwnedSession` starts its stopwatch in the constructor. `Run` calls
`Revalidate` and then `Launch` before the poll loop. `MonitorState` treats
10s without a frame as failure, including across that file hashing and
across the second filter's `Revalidate`. `Launch`, `AssignProcessToJobObject`,
`ResumeThread`, and `ReadFile` are synchronous. A stuck call never reaches
the 10s or 358s checks.

On the failure path `KillAndConfirm` waits until `min(360000, now+2000)`.
A row that expires at `rowStart+10000` is then given another 2s. The global
358s cutoff can fit its own 2s inside 360s. The row cap cannot. The assign
failure path does call `TerminateProcess` on the suspended process that is
not in the job, and `Run` does reach `Abort` afterward. `CleanupConfirmed`
also requires a zero-timeout wait, so a still-live process is not confirmed
by the job-kill bit alone. That ordering is fail-closed only if `Abort` runs,
and the wait is still outside the row cap. Neither fact has a pure oracle.
Success still goes through `Poll` before `MonitorState.Success`. That part
matches the earlier clock-order repair and remains unrun against a process.

The confirmation deadline is `min(rowStart+10000, 360000)`. Termination has
to start early enough that the wait ends at that deadline, not 2s after it.
A blocked `CreateProcess` is a separate source limit: the thread inside the
call cannot poll. The supervisor slice must arm `TerminateJobObject` from a
waiter that can run while launch is blocked. That waiter may only latch
failure. It must not be able to report success. This review does not make
that waiter the next edit.

## Security

**0 findings reachable from the current entries.** The dormant defects above
are source defects. They are not a launch path today.

`Invoke-BoundedReplay.ps1` throws before `Add-Type`, Cargo, or a process API.
Parameter validation cannot enable execution. `PureControllerChecks.Run` does
not construct `Admission`, `HeldFiles`, `OwnedSession`, or `NativeJob`.
Compiling those files defines `DllImport` and does not call it. `Admission`'s
constructor always throws after the pin reads, so no `Admission` instance can
be passed to `OwnedSession.Run`. `Parse` succeeding is not that constructor.

`selectors.json` keeps `registration_enabled` false. `bounded_matrix` and
`kernel_peek_capture` have no `#[test]` and no caller in the pure module.
The pure module does not contain `emit_frame(`, `run_inner(`, or `unsafe`.
The product `CreateProcessW` path is outside this diff. Nested Job behavior
was not executed. No account, service, VM, owner profile, or Lab Manager
state was inspected.

Removing `Admission.cs:93` would not make the adapter safe. The helper
directory would still be unenumerated, SystemRoot would still be inexpressible,
the manifest would still trust caller claims, and stderr would still be parsed
as frames. Do not remove the throw as a clearance.

## Failure ledger

No three-attempt handoff trigger.

The evidence note records single corrections: reference-assembly compilation,
the `GetStdHandle` import, formatting, the partial-frame fixture, one mutant
label, the self-hash mutant's scope, and one producer begin-flag inversion
that was restored after the literal oracle failed. None of those is a third
failed fix of the same defect. The combined adapter itself is the first
incomplete attempt. It stopped on source blockers. It is not three failed
repairs and it is not a reason to move the implementation to another provider.

Historical main71 run `43545244266` stays a separate unknown cause. This
review issues no retry. Expected assertion kills below are negative controls,
not failed fixes.

## Checks this pass actually ran

`Run` was read before execution. It reaches in-memory JSON, SHA256 of caller
bytes, synthetic clocks, and synthetic frames. It does not reach `NtCreateFile`,
`CreateProcessW`, `Stopwatch.StartNew`, a file open, or `emit_frame`. The five
Rust filters were read the same way. Together they call `frame`, the literal
oracle, `matches_failure` on synthetic observations, and `parse_case`. None
calls `emit_frame`, `one_row`, `bounded_matrix`, `kernel_peek_capture`,
`run_inner`, or `prepared_execution`.

```powershell
pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1
```

Baseline passed. All 21 mutants were assertion-killed, and their LF SHA256
values matched the evidence table. Controller and HeldFiles LF SHA256 values
were unchanged. The runner was PowerShell 7 `Add-Type` against its reference
assemblies. No .NET version was re-queried.

With `CARGO_PROFILE_DEV_DEBUG=0`, locked, offline, target
`x86_64-pc-windows-msvc`, each command was `cargo test -p arxd --lib` plus
one exact filter and `-- --exact --test-threads=1`:

- `development::windows::native_replay::pure::producer_bytes_have_independent_literal_oracle`
- `development::windows::native_replay::pure::counters_survive_invocation_and_scope_clears_even_on_unwind`
- `development::windows::native_replay::pure::oracle_counts_missing_zero_and_literal_bindings`
- `development::windows::native_replay::pure::mocked_failure_and_contract_mutants`
- `development::windows::native_replay::pure::controller_rejects_unknown_cleanup_extra_counts_and_private_payloads`

Each result was 1 passed, 0 failed, 0 ignored, 366 filtered out, harness
0.00s. The test binary was `arxd-5547724799c07633.exe`. First compile 44.48s.
Later commands finished in 0.24s to 0.28s. Cargo printed an unoptimized test
profile. `rustc -Vv` was not run. This is pure credit. It is not a native row
count, a child-safety result, or a cost measurement.

Not run: `Check-Source.py`, provenance, fmt, Clippy, helper or controller
launch, the PowerShell entry, native file or Job APIs, `run_inner`, ordinary
or full Rust tests, account or process inspection, network, GitHub, install,
CI, push, and subagents. Builder fmt, Clippy, and the producer inversion
remain builder evidence at the matching source tree.

## Next implementation slice

Sol high, one slice, no launch authority. Do not implement the transport,
the git-blob binder, or the waiter in this slice. Those stay specified below
so they are not redesigned ad hoc, and they stay unbuilt until this slice
has its own review.

Write the pure predicates and wire `Revalidate` to them:

- `ExclusiveNames` is true only for exactly `.`, `..`, and one expected leaf.
  Mutants for an extra DLL, a missing leaf, a repeated leaf, and a reparse
  name must fail. The native scan must pass names into this function. A scan
  that returns success without it is not this slice.
- `SystemRootSeparate` accepts disposable root `C:\fixture`, helper
  `C:\fixture\bin`, and SystemRoot `C:\Windows`. It rejects root `C:\`, a
  SystemRoot under the disposable root, and a pin path outside the root.
  Replace the current `C:\fixture\Windows` acceptance.
- Keep `Invoke-BoundedReplay.ps1` on `GATE2_ONLY` and keep
  `registration_enabled` false. Do not construct `OwnedSession` from `Run`.
  Do not call the directory query, `NtCreateFile`, or `CreateProcessW` from
  the pure tests.

Later slices, still without launch, registration, or a selected environment:

1. Held manifest. Read the fixed manifest only from a held handle. The
   external SHA256 and the commit come from a second disposition whose hash
   is outside both documents. Git blob equality is SHA-1 over
   `blob {length}\0` plus the held bytes. Provenance `source_sha256` must
   match, and `runtime_authority` must be false. Lock roles must be the same
   bytes as the source roles. `id128` is 32 lowercase hex characters, first
   qword then second, matching `IdText`.
2. Dedicated telemetry pipe, as SP-TRANSPORT. Do not register the filters.
3. Supervisor deadlines, as SP-DEADLINE, including the failure-only waiter
   for a blocked launch. Nested-Job proof and the 300/360/60 measurement
   stay owner-gated and unselected.

Environment selection, nested-Job proof, budget measurement, hardware, the
historical main71 cause, and PA6 stay open. They are not closed by writing
the predicates.

## Close

Standards: 1 hard finding, the untested refusal and supervisor. 1 judgement, divergent change in `Controller.cs`.
Spec: 6 findings. The worst is SP-EXCLUDE: current handles do not prove exclusive membership, and the contract can still be met by a held-handle scan plus a held leaf.
Security: 0 reachable findings. The worst dormant limit is that deleting the admission throw would still leave membership, SystemRoot, manifest binding, and stderr framing unresolved.
**Incomplete source may stay local. Native run readiness is NOT CLEARED.**

Performed: read-only Git inspection, blob and LF SHA256 recomputation, static
reading of the adapter and the pure call graph, one managed pure run with 21
assertion mutants, five exact Rust producer filters, and this file's write and
commit. Not performed: PROGRESS, source, design, or evidence edits; native
execution; launch; registration; publication.
