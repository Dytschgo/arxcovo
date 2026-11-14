# Windows main71 native replay: held-pin and decoder review

Disposition: the Sol partial source is **accepted** as an unpublished checkpoint.
It adds an uncalled file-pin primitive, a bounded in-memory frame decoder, a
Windows argument quoter, an exact four-value UTF-16 environment block, and an
owned-clock poll immediately before `OwnedSession.Success`. The `links==1`
guard is intact. There is no self-hash admission and no restored launcher.
**Native run readiness is NOT CLEARED.** Manifest admission, Rust telemetry
transport, and a Job-owning deadline supervisor are still absent. The new pin
code is not a TOCTOU-closed identity proof.

Local unpublished retention of this undiscoverable preparation is acceptable.
This review grants no remote publication, PR, CI admission, or native run.

Reviewer: xAI / grok-4.7, one independent pass, 2026-11-14. The assignment
forbids subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
Phase 1 still has no focused red loop that reaches that failure. Real-Runner
PA6 record/replay remains **OPEN**. Native, installed, and dedicated paths
are **unverified on real hardware**. No retry was issued. No run authority
is granted here.

## Pins

Porcelain was empty before the checks and empty after the managed run. The
review cwd was the isolated worktree. Its Git directory is a per-worktree
directory under the common Git directory. Paths below are repository-relative.
No owner path, account, or machine path is recorded.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-held-pin-decoder` |
| Prepared head | `066a7264a2df8bc22d4a4aac1a3ac6bfd0a52fa8` |
| Head tree | `fa1a978ca068d5d87bdf05410376a37c16ad388e` |
| Retained source | `6af00cbd741a2ac02e8498f15cca38b4282140e8`, tree `06271f2f616842046cd280816bbb570d77fd3ad7` |
| Supplied source | `b4ef99bea9d328c0e1129da1e2eae63ffb578dce`, tree `c84cef28f15613601b0f6b7f8b91414f35b05ac4`, parent `065123cffab4683f26b334084157ab172ec854e7` |
| Source patch | `b4ef99b` and `6af00cb` share patch-id `a1bd5bbb121b3d2a81f726a03d885e08a638fb63`. `git diff b4ef99b 6af00cb -- tools/windows-native-replay` is empty |
| Supplied evidence | `10d1a0140f82a7da8727739c18d4d5cd67edd60a`, tree `bd00b68c331b622e27973ae080b6ef2436483833` |
| Evidence patch | `10d1a01` and `066a726` share patch-id `c9da42ef78d5c733f33ad915a14ccba475e37b46`. The evidence file diff between them is empty |
| Prior Grok review | supplied `2e6259a0de20f184e5254435b9a4924f8f311bb4`, tree `41d61fb7ba82ff3b89c6fefb58e8c8b9a707f35e`. Retained ancestor `7e38e441a8a3bbef484c74375b249392dc25c6c9`, tree `91036a066beb731ddb19189b0ca3a4cfdc55d646` |
| Prior review blob | both commits store `docs/reviews/windows-main71-native-controller-contract-grok-recheck.md` as blob `8bc2d3e243adab0c08f547e6b9846a4abcc42c43` |
| Accepted proposal blob | `366876f711595c14bde42763e2a263cfe44ca9c2`, unchanged at this head |
| Source diff | `git diff 065123cffab4683f26b334084157ab172ec854e7 b4ef99bea9d328c0e1129da1e2eae63ffb578dce` — 6 files, 277 insertions, 12 deletions |
| Unchanged Rust blob | `crates/arxd/src/development/windows.rs` blob `c949e47dae5ac6998d6f406b250c761a43ab9d89`, LF SHA256 `48499b5014da7106513f5bd190c0de6300f0e71ce7086c9a70620c7db5980895` |

`2e6259a` is not an ancestor of this head. Its review text is the retained
blob above. `7e38e44` is the ancestor and also carries the earlier PROGRESS
note. This pass does not edit PROGRESS.

Changed-file pins at the prepared head, CR count 0:

| Source | Git blob | LF SHA256 |
| --- | --- | --- |
| `tools/windows-native-replay/Controller.cs` | `57b165d30bad70cce6e603d9c676657665679a83` | `6f9ee1c136f59c95ef0c2d5ea34a23dec3ab9bd3e629f01bbe629cba21f5d6de` |
| `tools/windows-native-replay/HeldFiles.cs` | `e9d888627b253ec273b49b965f4e814e7430a43e` | `c23c4046033bbe566a02f7a32ddd1fc253be3722cf541387aa83e6315a8c8bde` |
| `tools/windows-native-replay/PINNING.md` | `694b1ea7d7f04a8ee717c5705ff4eadea9f98f18` | `01fdb31e107fb3fff27184067783ce8f7f5ec7345c23290bde9f6a47c8d748aa` |
| `tools/windows-native-replay/Prepare-Provenance.py` | `10b16f1bc958d247bc595273eca2008f715836e3` | `bce03583e848cb3c1d62018b94a1eeb8ed84a1b371720683cf562ff600b92b6c` |
| `tools/windows-native-replay/Check-Source.py` | `d744e3fcf818f2569445cad47ba6a0f08f5da7d3` | `cd1fa4ed0ee8ec412dd011e4a3d382e462e138fa85e453a733cae406e0631d5f` |
| `tools/windows-native-replay/Test-PureController.ps1` | `b131931064ad1acf679640e71adc8cb1d417ccd2` | `707d002382db3dd231a13e1915df6a49571b4639f7463393287174087167e522` |

`git diff --check 7e38e44 HEAD` passed. `selectors.json` still has
`registration_enabled` false. `Invoke-BoundedReplay.ps1` still throws
`GATE2_ONLY` at its first executable statement.

## Prior blocker disposition

| Prior item | This tree | Credit |
| --- | --- | --- |
| Immutable launch | `HeldFiles` opens ancestors and a leaf, compares an external SHA256, and can copy under one approved root. No `run-manifest.json` and no admission object | Primitive only. Individual external pins are not manifest admission |
| Hardlink guard | `MetadataValid` requires `links==1`. `CopyPinned` reads through that check before any write. Provenance still refuses `st_nlink != 1` | Guard not relaxed. The old nlink-2 Cargo artifact still has no fallback. The post-read snapshot does not recheck link count |
| UTF-16 environment | `EnvironmentBlock` builds `PATH`, `SystemRoot`, `TEMP`, `TMP` as UTF-16LE with a double NUL | Pure bytes only. Nothing passes the block to a process |
| Argument encoder | `LaunchSpec.Quote` always quotes and follows the MSVC backslash rule. `Run` round-trips eight cases through an independent reader | Encoder only. No command line is joined or launched |
| Owned clock | `OwnedSession.Success` calls `Poll()` before the completion predicate. `Budget.Expired` records the accepted timestamp | Source wiring is present. `Run` still does not construct `OwnedSession`. No Job uses the clock |
| Telemetry | `FrameDecoder` accepts one bounded 40-byte numeric grammar and latches failure into `RowChannel` | In-memory decoder. Rust emits no begin/end records and there is no pipe |
| Open gates | Environment selection, nested-Job proof, measured 300/360/60 budget | Still open. `PINNING.md` says so |

## Standards

**PASS. No hard finding.** One carried baseline judgement.

The diff stays inside `tools/windows-native-replay/`. Root workspace members
are unchanged. The helper manifest and `fn main() {}` body are unchanged.
`windows.rs` is unchanged, so the `#[cfg(test)]` replay functions gain no
`#[test]`, `#[ignore]`, caller, or registration. `Check-Source.py` still
requires `registration_enabled` false and exactly one definition of each
replay function. The PowerShell entry still throws before `Add-Type`, Cargo,
or a process API. No workflow, CI, policy, ADR, proposal, review, or PROGRESS
file is in the source diff.

Repo rules checked: conventional scoped preparation, no credentials, no owner
paths, no product approval flow, and no claim that a native pin or launch ran.
`PINNING.md` and the evidence note call the result partial and label the
native properties UNRUN.

Fowler baseline: **Mysterious Name**, judgement only, carried forward.
`OwnedSession` still names the future containment obligation. It now polls
its own stopwatch before `Success`. It still owns no process and no Job.
`PINNING.md` says that. A documented contract requires the seam, so this is
not a hard naming violation.

`Quote` and the pure `Unquote` reader are an intentional oracle pair. The
accepted proposal requires an encoder with round-trip cases. Merging them
would remove the independent reader. No documented standard demands one
shared parser, and the smell is not filed.

## Spec

**0 findings of behavior that contradicts the partial contract.** Four
requirements are partial. The diff states that they are unfinished.

Checked against the accepted proposal blob `366876f7` and `PINNING.md`:

- The proposal says the driver must require a reviewed immutable
  authorization manifest, pin its hash in the run disposition, and reject
  absent or mismatched pins before native work. No manifest file, hash pin,
  or admission object was added. `HeldFiles` compares a caller-supplied hash
  and file id. That matches the `PINNING.md` statement that individual pins
  are not manifest admission.
- The proposal says the controller must contain the harness inside an
  inherited outer kill-on-close Job. `PINNING.md` item 6 requires one
  `OwnedSession` preserved across both filters and termination of the owned
  Job. `OwnedSession` still contains a clock, a channel, and a budget. There
  is no Job handle, no `TerminateJobObject`, and no active-process wait.
- `PINNING.md` item 5 requires Rust begin/end records, including kernel 0,
  feeding one session. `FrameDecoder` implements the numeric bounds for a
  future 40-byte frame: magic `0x32585241`, index below 35, flags in range,
  clean equal to 1, stdout and stderr at most 1, duration at most 10000,
  and at most 70 frames. `Finish` rejects a partial frame and an empty
  stream. `windows.rs` was not modified and still emits no such frames.
- `PINNING.md` item 4 requires one fresh sorted UTF-16 environment of exactly
  `PATH`, `TEMP`, `TMP`, and `SystemRoot`, with no inheritance. The builder
  does that and does not call `GetEnvironmentVariables`. The pure check
  compares the exact block. The block is not attached to a process, and
  `CREATE_UNICODE_ENVIRONMENT` has no call site.
- `LaunchSpec.Valid` still accepts `Kernel` or `Matrix` on every call. It
  does not record "once each". `PINNING.md` leaves that to the later adapter.
- The 35-row projection, filter split, and literal oracle are unchanged.
  `registration_enabled` remains false.

`PINNING.md` item 2 says to refuse filesystems where the identity guarantees
cannot be established, and that a check-then-open path hash is insufficient.
`HeldFiles` implements non-following opens, volume, reparse, directory,
disk-type, size, `links==1`, and a hash read from the retained stream. It
does not query a 128-bit file id, does not refuse a filesystem class, and
opens the leaf by path rather than relative to the held parent. The
destination id passed to the copy's second read is the id just observed on
that new handle. The external hash is still required. This is a partial pin,
and `PINNING.md` already withholds run authority for it.

## Security

**0 reachable findings.** The new native imports have no caller. Three
primitive limits are real and are not credited as admission.

`Controller.cs` still has no `DllImport`, `CreateProcess`, `RunPinned`, or
`Func<`. `HeldFiles.cs` is the only new P/Invoke surface: `CreateFileW`,
`GetFileInformationByHandle`, and `GetFileType`. Nothing in the product, the
helper, `selectors.json`, or `Invoke-BoundedReplay.ps1` constructs
`HeldFiles`. The entry throws `GATE2_ONLY` before compilation or launch.
`PureControllerChecks.Run` calls `MetadataValid`, `HashMatches`, `Quote`,
`EnvironmentBlock`, `FrameDecoder`, `MonitorState`, and `Budget` only. It
does not construct `HeldFiles` or `OwnedSession`.

Static native contract, not executed:

- Ancestors use access 0, share `FILE_SHARE_READ` (1), `OPEN_EXISTING` (3),
  and flags `0x02200000` (`FILE_FLAG_BACKUP_SEMANTICS` plus
  `FILE_FLAG_OPEN_REPARSE_POINT`). The walk starts at the drive root and
  holds every directory through the parent. Each must be a disk directory on
  the approved non-zero volume serial, with reparse attribute `0x400` clear.
- Leaves use `GENERIC_READ`, or `GENERIC_READ|GENERIC_WRITE` for
  `CREATE_NEW` (disposition 1). Share is `FILE_SHARE_READ` only. Flags are
  `FILE_FLAG_OPEN_REPARSE_POINT` (`0x00200000`). `MetadataValid` rejects a
  directory, a reparse point, a type other than `FILE_TYPE_DISK` (1), a link
  count other than 1, and a size outside `1..67108864`. A remote type or-ed
  with `0x8000` fails the exact type comparison.
- The `Info` layout is 13 sequential `uint` fields, 52 bytes, matching
  `BY_HANDLE_FILE_INFORMATION`. The hash is lowercase SHA256 compared with a
  caller-supplied 64-hex string. `HashMatches` never returns a computed hash
  as an authority. `CopyPinned` has no second path around `links==1`.
- Refusal for the pin checks is `IOException("file pin refused")`. That
  string contains no path. Stream exceptions on the failure path are
  rethrown unchanged; that path was not executed.
- On the installed .NET 10.0.14 runtime, the
  `FileStream(SafeFileHandle, FileAccess, int, bool)` constructor stores the
  same handle, and its disposing path calls `SafeHandle.Dispose`. That is
  IL inspection of `System.Private.CoreLib`. No handle was opened. The
  success path therefore has one owner for the leaf. The raw handle is
  disposed only when that constructor throws.

Uncredited limits:

- **SE-HOLD.** The leaf and each child are opened by path after the ancestor
  checks. There is no relative open from the held parent and no final-path
  comparison. Whether access 0 plus `FILE_SHARE_READ` freezes the directory
  namespace was not tested. The walk also retains the drive root. A later
  adapter must not keep a deny-write hold on the volume root.
- **SE-FILEID.** Identity is the legacy 64-bit index plus volume serial.
  `PINNING.md` requires refusing filesystems where that guarantee does not
  hold. This primitive does not. The copy's expected id is taken from the
  handle just created. That checks stability of the retained handle. It is
  not an external identity pin and it is not a self-hash.
- **SE-NLINK-AFTER.** `links==1` is enforced before the bytes are read. The
  snapshot after the read compares file id and size only. A link count that
  changes during the read is not a second acceptance path, and it is also
  not rechecked.

`Prepare-Provenance.py` still refuses `st_nlink != 1` before its read, sets
`runtime_authority` false, and only gains `HeldFiles.cs` in its source list.
It remains a snapshot utility. `FrameDecoder.Feed` and `Finish` contain no
wait. Overflow, a bad magic, an out-of-range field, a partial EOF, and a
second `Finish` latch `RowChannel` invalid. `OwnedSession.Success` reads the
session stopwatch through `Poll` before the predicate. No production caller
constructs that session, so the clock cannot terminate a Job that does not
exist.

The outer product Job and the non-fixture duplicate refusal are outside this
diff. No account, service, VM, owner profile, or Lab Manager state was
inspected.

## Partial source and run readiness

**Partial source: accepted.** The checkpoint may remain locally. The nlink-2
guard is not relaxed. The decoder, quoter, environment block, and clock order
fail closed in the pure checks below. New native paths stay unreachable:
`GATE2_ONLY` is unconditional, `registration_enabled` is false, and no
product file gained a caller.

**Native RUN READY is NOT CLEARED.** Retaining this head is a separate
decision from launching it. No remote, PR, CI, budget admission, or native
execution is authorized. The pin primitive must not be treated as admission
until the contract below is implemented and reviewed again.

## Smallest remaining integration contract

This is the unimplemented adapter already required by the accepted proposal
and `PINNING.md`. It adds no account, measurement, registration, or other
authority. Environment selection, the nested-Job proof, and the 300/360/60
measurement stay separate open gates. Writing the adapter does not pass
those gates.

1. Manifest admission. Add the fixed `run-manifest.json` whose SHA256 is
   pinned outside the manifest. Reject an absent manifest, a dirty checkout,
   and any mismatched blob. Bind the reviewed commit, every `SOURCES` blob
   and LF SHA256, the compiled controller, toolchain `cargo.exe`, the silent
   helper, the libtest executable, `selectors.json`, the provenance report,
   and both lockfiles. Paths are exact, absolute, and inside one disposable
   workspace root. No branch alias, wildcard, relative path, UNC or device
   path, or alternate stream. The provenance utility stays a preparation
   snapshot. `HeldFiles` may receive only hashes and ids from that external
   manifest. It must not compute an expected hash from the bytes it just
   read. Before any call can authorize a launch, correct SE-HOLD,
   SE-FILEID, and SE-NLINK-AFTER: open each child relative to the held
   parent, use a 128-bit file id or refuse the filesystem, recheck
   `links==1` after the hash, and do not hold the volume root. Keep
   `links==1` with no nlink-2 copy fallback. Directory handles must also
   prove an exclusive helper-directory membership. The Cargo grammar still
   needs a reviewed exclusion so the four-value environment cannot rebuild
   an unpinned harness. Join arguments only with `LaunchSpec.Quote`. Pass
   the existing environment block with `CREATE_UNICODE_ENVIRONMENT`. No
   shell, no null `lpEnvironment`, and no inherited block.
2. Rust telemetry transport. Emit the actual begin and end record for every
   row, including kernel index 0, as the 40-byte numeric frame this decoder
   already bounds. Feed one `FrameDecoder` on the `OwnedSession` created
   before the first launch and kept across Kernel and Matrix. Poll that
   session. Do not add a callback or a second clock. `Success` already polls
   first. Incomplete, empty, duplicate, out-of-order, and overflow input
   must leave the channel failed. Discard raw harness output. Do not publish
   it and do not synthesize the expected counts.
3. Job-owning deadline supervisor. The same session owns the outer Job. On a
   deadline, telemetry failure, or containment failure, terminate that Job
   before returning unknown cleanup. Keep kill-on-close until the owned Job
   reports zero active processes. Stop the remaining rows and forbid reuse.
   Success stays the predicate already in `MonitorState.Success`: complete
   budget, empty unfailed channel, exit 0, and active 0, after the owned
   poll. No PID or name kill and no broad cleanup. Nested-Job behavior in a
   selected standard account remains a separate open gate.

## Evidence from this pass

The pure call graph was read before execution. `Run` reaches string quoting,
dictionary checks, `SHA256.Create` on caller-supplied bytes, synthetic
timestamps, and in-memory frame bytes. It does not reach `CreateFileW`,
`GetFileInformationByHandle`, `GetFileType`, `Stopwatch`, `Process`, or a
file API. `OwnedSession` is the only stopwatch, and `Run` does not construct
it. Compiling `HeldFiles` defines its imports. The baseline and the mutants
do not call them.

The managed runner was then executed once:

```powershell
./tools/windows-native-replay/Test-PureController.ps1
```

Compiler: PowerShell 7, .NET 10.0.14, in-memory `Add-Type`. No install and
no network. Baseline passed. All eleven mutants were assertion-killed:
index, complete, timeout, arguments, environment, held-pin, link2,
argument-escape, frame-eof, frame-output, clock-order. After the run the
Controller LF SHA256 was
`6f9ee1c136f59c95ef0c2d5ea34a23dec3ab9bd3e629f01bbe629cba21f5d6de` and the
HeldFiles LF SHA256 was
`c23c4046033bbe566a02f7a32ddd1fc253be3722cf541387aa83e6315a8c8bde`.

The builder's earlier red invocations are builder evidence. This pass
reproduced the final green script only. `Check-Source.py` was read and not
executed. Provenance was not executed. Rustc, Cargo, fmt, and Clippy were
not run. The unchanged `windows.rs` blob keeps the earlier pure-filter
evidence on the prior source tree; this pass does not refresh it.

## Close

Standards: 0 hard findings; 1 judgement, `OwnedSession` names a Job it does not own.
Spec: 0 contradictory findings; 4 partial requirements. The worst is the absent sealed manifest.
Security: 0 reachable findings. The worst uncredited limit is the path-based open, whose directory hold was not proven.
**Partial source is accepted. Native run readiness is NOT CLEARED.**

Performed: read-only Git inspection, blob and LF SHA256 checks, static
P/Invoke and FileStream-ownership inspection, one managed pure run with
eleven in-memory assertion mutants, and this file's write and commit. Not
performed: source, evidence, proposal, or PROGRESS edits; `Check-Source.py`;
provenance; Cargo or Rust tests; helper or controller launch; the PowerShell
entry; native file, process, or Job APIs; account, service, VM, or
private-state inspection; network; install; GitHub; CI; push; subagents.
