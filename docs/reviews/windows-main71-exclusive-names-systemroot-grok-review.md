# Windows main71 native replay: exclusive names and SystemRoot review

Disposition: the Sol high exclusive-name and SystemRoot slice is **acceptable
as source**. `RevalidatePins` wires two bounded `NtQueryDirectoryFile` scans of
the held helper directory and the held launched-image directory into
`ExclusiveNames`, holds each sole leaf, and `SystemRootSeparate` admits an
external `C:\Windows` without retaining the volume root. The final refusal
remains. **Native run readiness is NOT CLEARED.** This review grants no
publication, PR, CI admission, registration, or launch.

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
The review cwd equalled the linked worktree top level. The Git directory is
that worktree's directory under the common Git directory. Paths below are
repository-relative. No owner path, account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-exclusive-names-systemroot` |
| Reviewed head | `9f51d9ad4be91064d282d8438df91813f4081f6d` |
| Head tree | `04baffbce17db3d3f21964bec1d9148dbf045fb7` |
| Supplied source | `37ff6946532fcef93e28de3aea39582d13c4a6f5` |
| Retained source | `77e5dcb0f2be8c5bd076d6880f65e47b19b9d5e6` |
| Source tree | `1674e02f43c7f913d931cd585ea10898222d8946` (both commits) |
| Source parent | `9b52b042856c9e0876c45280c1d559170539c07d` |
| Supplied evidence | `37286b5b6fa004325efc929c7e43482f1a017569` |
| Retained evidence | `69968be2cc91c8a53df2d2368d55a26a67377c12` |
| Evidence tree | `91bd474123f71b81b10f5af2c004de46a64b5de8` (both commits) |
| Head after evidence | `9f51d9a` changes `docs/PROGRESS.md` only |
| Prior review | `984d345c71ea91dfce069c2000da2caa1f8556d4`, retained `620b3be91ee90dc957b8e03b6f0b53177b6e77a0`, same tree `f4f2751ffe109f2a4ba3d0cc3e9ea7525baee3a0` |
| Accepted proposal blob | `366876f711595c14bde42763e2a263cfe44ca9c2`, unchanged |
| Source diff | `git diff 9b52b042856c9e0876c45280c1d559170539c07d 77e5dcb0f2be8c5bd076d6880f65e47b19b9d5e6` — 5 files, 290 insertions, 23 deletions |

`git diff 37ff694 77e5dcb` and `git diff 37286b5 69968be` are empty. The
supplied commits are not ancestors of this head; each retained commit has the
same tree and the same patch. `git diff --check` on the source diff reported
no whitespace errors. The five source blobs and their LF SHA256 values match
the evidence table, and every checked CR count was 0. `selectors.json` still
has `registration_enabled` false. `Invoke-BoundedReplay.ps1` still throws
`GATE2_ONLY` before `Add-Type`.

The evidence file still contains one pre-existing invalid UTF-8 byte, `0x96`,
at offset 39689 (line 588). The new section starts at line 688. This pass did
not rewrite that file.

## Prior blocker disposition

| Prior item | This tree | Credit |
| --- | --- | --- |
| SP-EXCLUDE | Two restarted class-60 scans, exact `.` / `..` / one leaf, relative leaf hold, helper and image directories | Source wiring accepted. Kernel share and query behavior UNRUN |
| SP-SYSROOT | External `C:\Windows` parses; volume root, nested root, equal root, and outside pins refuse; volume bootstrap handle is closed | Source predicate and hold path accepted. Native open UNRUN |
| SP-BIND, SP-TRANSPORT, SP-GRAMMAR, SP-DEADLINE | Unchanged open gates. Final throw is now `remaining admission gates unproved` | Not this slice and not closed |
| ST-UNTESTED refusal | Pure oracle calls `RevalidatePins` and `RevalidateBoundary` | This slice's predicates and mock wiring are tested. Native record decoder is not executed |

No three-attempt handoff trigger was present in `984d345`, and none is present
here. The evidence ledger's three managed failures are three different mutant
definitions: a surviving leaf mutant, an unreachable-code mutant, and a syntax
mutant. The production leaf checks, dot-type test, and leaf-id statement are
still present. Those runs are not three failed repairs of one production
defect.

## Standards

**0 hard findings.** Baseline applied. Two judgements, not blockers.

Repo rules that hold: one conventional preparation commit; a meaningful pure
test for each new predicate and for the mock revalidation wiring; no
credentials or owner paths; no product approval flow; no workflow, ADR, root
Cargo, or Rust edit. `Check-Source.py` remains a string inventory and is no
longer the only check of the new decisions. The pure runner refuses a CR byte
and checks exact LF SHA256 of the five compiled sources before and after the
mutants. Mutants are in-memory copies with renamed types. A compiler error
does not count as a kill.

Fowler baseline:

- **Divergent Change**, judgement only. `Controller.cs` still holds the wire
  decoder, the session types, and the pure suite, and this slice adds
  `AdmissionPureChecks` there. The runner compiles that file as the entry, so
  splitting the oracle would fight the current harness. Not a hard structure
  violation.
- **Mysterious Name**, judgement only. `ExclusiveDirectory` appends a final
  component `held` so `OpenParent` returns the directory itself. That
  component is not opened. The name does not say so. The behavior is the
  relative parent open the contract asked for.

`PINNING.md` was not in this delta. Its prepared-adapter paragraph still says
helper-directory exclusion is unimplemented. That sentence is stale. It is not
a defect in the five reviewed files.

## Spec

**0 findings.** Checked against the prior review's next slice, `PINNING.md`
item 2 and item 4, and the questions in this assignment. Later gates stay open
on purpose.

### Directory exclusion

`RevalidatePins` requires the helper pin to be `helper\cargo.exe`, takes the
launched image's directory from its pin, and refuses to use one directory for
both. It then calls `VerifyExclusive` on each directory. `VerifyExclusive`
scans, requires `ExclusiveNames`, reads the leaf, scans again with a restart,
requires `ExclusiveNames` and `SameScan`, and requires the second scan's leaf
id text to equal the pin.

`HeldFiles.ExclusiveDirectory` opens that directory by a relative
`NtCreateFile` from its held parent (`FILE_LIST_DIRECTORY|SYNCHRONIZE`, share
read, `FILE_DIRECTORY_FILE|FILE_OPEN_REPARSE_POINT`, synchronous nonalert).
`Scan` calls `NtQueryDirectoryFile` class 60, one record, a 1024-byte buffer,
at most four calls. The first call of each scan sets restart. `STATUS_NO_MORE_FILES`
(`0x80000006`) ends the scan. A fourth successful record, a short or oversized
record, a nonzero next-entry offset, or any other status refuses. The record
offsets used for attributes, name length, reparse tag, both id qwords, and the
name match `FILE_ID_EXTD_DIR_INFORMATION`.

`ExclusiveNames` is true only for three distinct names: `.` and `..` with the
directory attribute, and the exact leaf without that attribute. Any reparse
attribute, nonzero reparse tag, or all-zero 128-bit id fails. `SameScan`
compares name, attributes, tag, and both id qwords as a set. The leaf is opened
relative to the same directory handle, share read only, and stored in the
retained file list before `ReadVerified` returns. `ReadVerified` still requires
disk type, not-directory, not-reparse, `links==1` before and after the hash,
and the handle `FileIdInfo` to match the pin. The final
`remaining admission gates unproved` throw does not dispose those handles.
Nothing in this repository constructs `HeldFiles` or `Admission`, so that
retention is source structure, not an observed handle.

### Share and namespace

The source comment is the right strength. Share read on the directory is the
documented share fence for a later `FILE_ADD_FILE` or `FILE_ADD_SUBDIRECTORY`
open of that same directory object, and only while the handle remains open.
It does not fence `FILE_DELETE_CHILD` or `FILE_WRITE_ATTRIBUTES`, it does not
list names, and it does not freeze a child this process has not opened. An
existing leaf can be rewritten until the leaf handle is held. The two scans
plus the held leaf are the source's answer to that gap. This is static
reasoning about the calls that were written.

It is not Windows kernel proof. The pure suite never calls `NtCreateFile` or
`NtQueryDirectoryFile`. A passing `ExclusiveNames` fixture does not show that
a real query is complete, that share enforcement matches this description, or
that a reparse was refused by the filesystem. Those statements stay **UNRUN,
unverified on real hardware**.

System32, KnownDLLs, and the process current directory are outside this
allowlist. This slice does not claim to seal them.

### SystemRoot

`SystemRootSeparate` accepts root `C:\fixture`, helper `C:\fixture\bin`, temp
`C:\fixture\tmp`, and system `C:\Windows`. `Absolute` rejects a volume root
(`C:\` is shorter than 4 and has an empty component). Equality, system-under-root,
and root-under-system are refused, using a backslash after the prefix so
`C:\fixture` does not swallow `C:\fixture2`. Parse still requires every source
and artifact path to be inside the disposable root, and revalidation passes
those paths through the same predicate. The old `C:\fixture\Windows` acceptance
is gone.

`HoldEnvironment` opens temp under the disposable root and opens SystemRoot
only when the manifest string is exactly the constructor's approved string. For
`C:\Windows\held`, `OpenParent` opens `Windows` from the volume and disposes
the volume handle on that first child. The SystemRoot directory handle is
cached and kept; the volume handle is not. `DirectoryValid` and `Identity`
still require the approved volume serial, a non-reparse directory, and the
approved 128-bit id. A zero id is refused. No pure test opens that path.

### Revalidation, tests, and other gates

`Admission.Revalidate` calls `RevalidateBoundary`, which calls `RevalidatePins`
and then throws. `OwnedSession.Run` is not reachable from `PureControllerChecks.Run`.
A repository search found no `new Admission(`, `new HeldFiles(`, or
`new OwnedSession(` in the C#, PowerShell, Rust, or Python sources. The entry
point still throws at its first executable statement. `registration_enabled`
is false.

The pure oracle covers an extra name, a missing leaf, a repeated leaf, a wrong
leaf, reparse attribute and tag, directory-typed leaf, non-directory dot, zero
id, changed low id, high id, and attributes, set reordering, two scans on each
mock directory, a changed second scan, leaf size, leaf id, source CR, volume
root, nested and equal SystemRoot, workspace-under-SystemRoot, and outside
helper, temp, source, and artifact paths. The final throw is asserted by
message. `scan-wire` changes the dot id rather than the leaf id, so the leaf
pin cannot hide a deleted `SameScan`. `scan-first-wire` requires zero leaf
reads. `scan-second-wire` is killed by the scan-count assertion because
skipping the second call leaves the count at 1. The harness treats every
`admission:` exception as a kill; for these mutants the first failing
assertion is the one that states the broken guard.

Manifest binding, the dedicated transport, the accepted launch grammar, and
the deadline supervisor are not claimed closed. The evidence list matches the
code: the throw is unconditional after the new checks.

## Security

**0 findings reachable from the current entries.**

`Invoke-BoundedReplay.ps1` throws before `Add-Type`, Cargo, or a process API.
Parameter validation cannot enable execution. `PureControllerChecks.Run` does
not construct `Admission`, `HeldFiles`, `OwnedSession`, or `NativeJob`, and it
does not start `Stopwatch`. Compiling the files defines `DllImport` and does
not call it. Removing the final throw would still leave binding, transport,
grammar, deadlines, and an unrun kernel query. It would not be clearance.

No account, service, VM, owner profile, or Lab Manager state was inspected.

## Failure ledger

No three-attempt handoff trigger.

Historical main71 run `43545244266` stays a separate unknown cause. This
review issues no retry. The 45 assertion kills are negative controls, not
failed production fixes.

## Checks this pass actually ran

`Run` and `AdmissionPureChecks` were read before execution. They reach
in-memory JSON, SHA256 of caller bytes, synthetic clocks, synthetic frames,
and the mock directory objects. They do not reach `NtCreateFile`,
`NtQueryDirectoryFile`, `CreateProcessW`, `Stopwatch.StartNew`, a filesystem
open, or `emit_frame`. `Check-Source.py` only reads source and
`selectors.json`.

```powershell
python tools/windows-native-replay/Check-Source.py
pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1
```

The inventory command exited 0: 28 oracle rows, five pure Rust registrations,
zero native registration or callers, selectors disabled. The managed command
exited 0. Baseline passed. All 45 mutants were assertion-killed, and each
printed LF SHA256 matched the evidence table, including `names-leaf`
`af947a7d80e7d780e470aad3d198746c199aa9a46b000cc4bf7118c80967f05a` and
`final-refusal`
`cc27faa6738ccc038ab8bfe237b8b3881e85b2401229e21007f8d9a7ca1edd75`. The five
exact-LF restore lines matched the source blobs' LF SHA256 values. No duration
was measured by this pass. No .NET or PowerShell version was queried.

Not run: the PowerShell entry, provenance, fmt, Clippy, helper or controller
launch, native file, directory, process, or Job APIs, `run_inner`, ordinary,
discovery, or full Rust tests, account or process inspection, network, GitHub,
install, CI, push, and subagents.

## Advisories

- `PINNING.md` still describes the pre-slice exclusion blocker. A later docs
  edit can replace that sentence. It is not a source repair.
- The same-directory refusal has no direct fixture. A shared directory still
  fails the one-leaf scan unless it can satisfy both leaf names at once.
- The class-60 decoder has no byte fixture. A nonzero `NextEntryOffset` on a
  single record would refuse, which is fail-closed and unrun.
- Leaf comparison is ordinal. A case difference between the pin and the stored
  name refuses. `HoldEnvironment` also requires an ordinal match to the
  approved SystemRoot string.
- `IdText` is first qword then second qword for both the directory record and
  `FileIdInfo`. That is internal consistency, not an external byte-order oracle.

## Next implementation slice

Sol high, one slice, no launch authority. Do not reopen this directory
contract unless a later authorized run disproves it. Do not implement the
telemetry pipe or the deadline waiter in the next slice.

Held manifest binding:

- Read the fixed manifest only from a held handle. The external SHA256 and the
  reviewed commit come from a second disposition whose hash is outside both
  documents.
- Git blob equality is SHA-1 over `blob {length}\0` plus the held bytes.
- Provenance `source_sha256` must match, and `runtime_authority` must be false.
- Lock roles must be the same bytes as the source roles.
- `id128` is 32 lowercase hex characters, first qword then second, matching
  `IdText`.
- Keep the entry on `GATE2_ONLY`, keep `registration_enabled` false, do not
  construct `OwnedSession` from `Run`, and do not remove
  `remaining admission gates unproved`.
- Do not call native file, query, process, or Job APIs from the pure tests.

Environment selection, nested-Job proof, budget measurement, hardware, the
historical main71 cause, and PA6 stay open.

## Close

Standards: 0 hard findings. 2 judgements, divergent change in `Controller.cs` and the sentinel component `held`.
Spec: 0 findings. The slice wires the two held-directory scans and the separate SystemRoot predicate, then still refuses; kernel enforcement remains unrun.
Security: 0 reachable findings. The worst dormant limit is that deleting the final throw would still leave binding, transport, grammar, deadlines, and an unexecuted query.
**Acceptable source may stay local. Native run readiness is NOT CLEARED.**

Performed: read-only Git inspection, LF SHA256 comparison, static reading of
the five-file delta and the pure call graph, `Check-Source.py`, one managed
pure run with 45 assertion mutants, and this file's write and commit. Not
performed: PROGRESS, source, design, or evidence edits; native execution;
launch; registration; publication.
