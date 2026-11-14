# Gate 2 source checkpoints (NO RUN AUTHORITY)

The original partial-plan sections below describe the retained baseline. The
prepared-adapter follow-up at the end supersedes their source inventory; it
explicitly lists the current technical blockers. No native execution is enabled.

RunPinned remains absent. HeldFiles.cs now contains native file-pin imports,
compiled as source only. No check constructs HeldFiles or invokes its imports.
It opens ancestors root down without following reparse points, restricts leaves
to an approved root/volume, checks disk/regular-file type, size, identity and
link count, hashes bytes from held handles and retains handles until disposal.
CopyPinned refuses a source with link count two before copying, creates a new
destination with CREATE_NEW, writes/flushes/verifies using the same retained
handle and FILE_SHARE_READ only. It never closes/reopens that destination for
verification. There is no copy fallback for a rejected original artifact.
All these native properties are UNRUN and unverified on real hardware.

The managed wire decoder now accepts bounded fragmented 40-byte numeric frames,
and latches malformed input, partial EOF and overflow into the row channel.
This is a byte decoder, not a pipe transport. LaunchSpec has an argument encoder
and an explicit four-value UTF-16 environment builder. OwnedSession.Success polls
its own clock immediately before evaluating completion. Pure checks do not
construct that session. There is still no Job owner, native kill loop, Rust event
emission, manifest admission or launch adapter. PowerShell refuses unconditionally.
Do not restore the old string-command/callback/stopwatch launcher.

Remaining integration blocker: no sealed admission object binds a held fixed
manifest (with externally supplied expected SHA256) to ALL source/build/artifact
pins and the two launches. HeldFiles accepts individual external pins; that is
not manifest admission. Two complete held-directory scans and the held sole leaf implement source
admission for both executable directories; kernel semantics remain UNRUN. The Cargo grammar also needs a reviewed way to exclude rebuilding
an unpinned harness under the four-value environment. No manifest or artifact
snapshot may bootstrap its own expected hash. No run-manifest.json is generated
by this partial implementation. No claim of a functional controller adapter.

The coordinator must obtain fresh Grok/Muse source/security review before any
publication or run. A later reviewed adapter needs all of these concrete pieces:

1. A fixed `tools/windows-native-replay/run-manifest.json` whose SHA256 is pinned
   OUTSIDE that manifest in the run disposition. Reject absent manifest, dirty
   source or mismatches. It must name the full reviewed commit and Git blobs plus
   LF SHA256 for every SOURCES entry in Prepare-Provenance.py; exact compiled
   Controller assembly, toolchain cargo.exe, silent helper, libtest executable,
   selectors.json, provenance report and the two lockfiles. Paths must be exact,
   absolute and confined to the disposable workspace; no branch alias, wildcard,
   relative path, UNC/device path or alternate stream. Source report hashes must
   come from the exact build checkout. The provenance utility remains only a
   bounded preparation snapshot, never a runtime pin or admission decision.
2. Open every ancestor and leaf using non-following handles, verify no reparse
   tags and same approved volume, then hold handles through launch, rows and
   teardown. Leaf identity includes volume/file ID, link count exactly one,
   bounded length and hash read FROM that handle. Deny write/delete sharing.
   Reject hardlinks, replacement and identity change; hash the final copied
   cargo.exe helper inside the sole helper directory. Pin directory membership
   so another executable cannot be inserted. Refuse filesystems where these
   guarantees cannot be established. No check-then-open path hash suffices.
3. Bind application to the hashed toolchain cargo.exe, cwd to that pinned source
   checkout, and the 12-element argument vector accepted by LaunchSpec.Valid.
   Permit only Kernel then Matrix, once each. Use one reviewed Windows argument
   encoder with round-trip tests (including spaces, Unicode, trailing slashes and
   quotes), or a reviewed direct libtest grammar replacing this Cargo grammar.
   No shell, caller command string or ad hoc quoting. Validation alone in this
   revision does NOT retain immutable copies or verify any file identity.
4. Build a fresh explicit UTF-16, sorted, double-NUL environment block with
   CREATE_UNICODE_ENVIRONMENT. Exactly PATH=the one pinned helper directory,
   TEMP=TMP=the exact pinned disposable temp directory and SystemRoot=the pinned
   approved Windows directory. No inherited environment, owner credentials,
   CARGO_HOME, alternate PATH component or profile lookup. Profile/debug/default
   features are build pins. The pure environment block builder is implemented;
   it is not passed to any native launch.
5. Implement owned bounded telemetry from Rust's actual begin/end row boundaries
   (including kernel 0), with only fixed numeric fields and actual observed
   counters. Current RowChannel is an in-process bounded contract, NOT an IPC
   adapter. An incomplete frame, EOF, missing/duplicate/out-of-order event or
   overflow must latch failure. Capture/discard raw harness output separately;
   do not feed it to the channel or publish it. Do not synthesize expected counts.
6. The native adapter must start ONE OwnedSession before any harness launch and
   preserve it across both filters. Poll its sealed nonblocking channel, never a
   caller callback, with bounded work per iteration and zero-time process waits.
   Any deadline, telemetry or containment failure must terminate the OWNED Job
   before returning unknown cleanup; keep kill-on-close ownership through actual
   zero-active/process-exit confirmation. Success requires Budget.Complete, no
   pending events, harness exit 0 and Job active 0. Unknown cleanup stops all
   remaining rows and prohibits reuse. No PID/name kills or broad cleanup.
7. Prove nested outer/inner Job containment without weakening either fence in a
   separately selected fresh standard-account environment. That environment is
   NOT selected here. Measure budgets separately: 300/360/60 seconds remains
   UNMEASURED; no native runtime or teardown timing is established by pure tests.

These are concrete blockers, not completed implementation. M2 incomplete;
cause unknown; PA6 replay OPEN; runtime UNRUN; unverified on real hardware.

## Prepared adapter follow-up (source only, blocked)

Admission.cs defines a bounded closed JSON schema (65536 bytes, depth 4,
no extra or duplicate keys, complete closed role set). Its SHA256 is supplied
externally to Parse, never read from a manifest property. Pins carry exact
absolute path, external SHA256, 128-bit file identity, exact size and reviewed
source Git blob. Artifact roles are controller, cargo, helper, libtest,
selectors, provenance, workspace_lock and helper_lock. The parser also binds
commit, compiler/Cargo versions, target, profile, debug and features.
This is build-artifact data, never run authority. No usable manifest is fabricated.

HeldFiles now uses relative NtCreateFile child opens, mandatory FileIdInfo,
post-hash metadata checks including links==1, and closes the transient shared
volume-root handle. No copy operation or nlink-2 fallback exists.
NativeJob prepares suspended direct-libtest launch with an explicit inherited
handle list, explicit UTF-16 four-value environment and outer kill-on-close Job
assignment before resume. Toolchain Cargo remains an artifact pin; it is not a
runtime rebuild command. This grammar change requires independent review.
OwnedSession owns this Job and its clock across both filters. Rust dormant row
functions now take a dedicated typed telemetry writer; both raw harness streams
are discarded to NUL (see the dedicated transport follow-up below).
Frame order and complete segment counts are checked independently of Budget.

BLOCKED: Admission.Revalidate still unconditionally refuses. The held binder
below supersedes the old caller-byte/cleanCheckout/blob-map admission path.
Directory membership and separate SystemRoot checks are implemented in source,
not proven against a kernel. The dormant
supervisor has not received a native or independent security review; cleanup
and success/failure races remain unverified. Its 358-second launch-loop cutoff
reserves two seconds for cleanup; per-row teardown containment within ten
seconds still requires a completed supervisor proof. Pure cleanup decision mutants have only mock-state credit; they do not prove
Job termination ordering or kernel cleanup. UNRUN native APIs.

The three-integration assignment is incomplete. This prepared source is a
technical-blocker handoff, not run-ready source or acceptance of the changed
grammar. Environment unselected; nested Job proof UNRUN; costs UNMEASURED.
M2 incomplete; cause unknown; Phase 1 no focused real red loop; PA6 replay OPEN;
all hardware paths unverified on real hardware. All earlier gates still apply.

## Held manifest binding follow-up (source only, not kernel proof)

Admission.Bind is the dataflow used by the dormant constructor and Revalidate.
The coordinator must supply an externally trusted SHA256 and nonzero lowercase
32-hex identity for fixed `tools/windows-native-replay/run-disposition.json`.
That hash is outside BOTH documents. It is future coordinator input, not a
permission, generated self-hash, environment selection, or signature checked by
this code. No real disposition, manifest or provenance is generated here.

The held disposition read is at most 65536 bytes, closed schema/depth 4:
`schema:1`, `runtime_authority:false`, `reviewed_commit` (40 lowercase hex),
`root`, `manifest:{sha256,id128,size}`, `source_blobs:{<every Sources path>:<Git
SHA1>}`. The externally trusted disposition attests the reviewed commit's blob
mapping; the binder does not invoke Git or claim to verify a commit object.
The fixed run-manifest.json is read only through IAdmissionFiles.ReadPinned at
its disposition-bound hash, identity and exact size (1..65536). Its closed schema
also requires `runtime_authority:false`; neither document can grant execution.

Every pin is held-read and hashed, with exact sizes. Source bytes must have no
CR and be at most 2 MiB; Git SHA1 is computed over ASCII `blob {length}\0` then
those exact bytes and compared with BOTH the manifest and disposition. Artifact
roles controller/cargo/helper/libtest/selectors/provenance/workspace_lock/
helper_lock remain mandatory. The provenance is at most 65536 bytes, closed
schema 1, matching source_commit_claim, runtime_authority:false,
native_executed:false, exact source_sha256 map and matching helper_sha256.
All provenance source hashes refer to the same held bytes used for Git SHA1.
Workspace/helper locks and selectors must match the corresponding source role
in exact path, identity, size, hash AND bytes. Other role path aliases refuse.
File ids are exactly 32 lowercase hex, nonzero, first qword then second as in
HeldFiles.IdText; a literal pure oracle fixes the order.

The returned Facts retain private copied pins and expose only detached copies.
No caller blob dictionary, cleanCheckout boolean or public mutable flag can
replace binding in Admission. Pure mocks exercise this same binder; they do
not exercise HeldFiles, native opens or directory queries. Both exclusive
scans and all native metadata/reparse/link-count checks remain in place.
Revalidate rebinds and still ends in `remaining admission gates unproved`.
GATE2_ONLY and selectors=false remain. Run does not construct OwnedSession.
Dedicated transport, accepted launch grammar, deadline waiter/supervisor,
registration, environment selection and runtime proof remain outside this slice.
Fresh Grok/Muse review is required before publication. M2 incomplete; main71
cause unknown/no retry; Phase 1 no native redloop; PA6 OPEN; hardware unverified.


## Dedicated telemetry transport follow-up (source only)

Supersedes item 5's missing IPC adapter and the old stderr transport inventory.
NativeJob creates one telemetry pipe, clears inheritance on its sole parent
reader, and prepares exactly three unique inherited handles in the attribute
allowlist: NUL stdin, shared NUL stdout/stderr, and a separate telemetry writer.
Both raw harness streams go directly to the null device and are never decoded.
The parent writer copy closes in finally before EOF observation, including on
launch failure. NativeJob owns the reader until Next/Dispose. Its read adapter
performs one PeekNamedPipe and at most one ReadFile of min(available,4096) per
poll; an empty peek never reads. There is one reader and no read callback.
ERROR_BROKEN_PIPE is EOF, all other peek/read errors fail closed. The injected
boundary checks zero/oversize reads, fragmented frames, partial/missing EOF,
order, duplicates, overflow, exact segment completeness and final trailing data.
The same decoder/channel/session spans kernel 0 then matrix 1..34.

Rust's cfg(test) fixture takes an explicit Telemetry<NativeWriter> parameter.
NativeWriter takes sole ownership of a Handle, refuses invalid/std handles and
non-pipes, verifies inheritance and clears it BEFORE any fixture helper can
launch. It closes through Handle's destructor on failure/unwind/return; there
is no duplicate or production field. emit_frame uses that writer only, with a
single exact 40-byte write; short/error writes latch failure and do not retry.
The serializer stays pure with independent literal byte assertions. The pure
producer check calls emit_frame with a mock writer for begin/end and failure
counts; no WriteFile is reachable from that check.

External handle delivery is still proposed and UNWIRED. No CLI argument,
environment variable, libtest registration or discovery entry was added. The
four-variable environment grammar is unchanged. RequireTelemetryBinding throws
before CreateProcessW even if admission were bypassed; it marks the separate
SP-GRAMMAR disposition needed to connect a future internal wrapper. The source
transport is complete within this explicit boundary; it is not launch-ready.
Revalidate's final refusal, GATE2_ONLY and disabled selectors remain in force.
No Run constructs OwnedSession. Fresh Grok/Muse review is required at final
source/evidence pins before publication. This builder does not delegate it.

Duration remains OPEN: the legacy frame ms value 0 is reserved, never a measured
runtime. No entry/row clock was added and no supervisor race/deadline fix is
claimed. Environment selection, nested Job compatibility and budget are UNRUN.
M2 incomplete; main71 cause unknown; no historical retry; Phase 1 no focused
native red loop; PA6 OPEN; **unverified on real hardware**.


## Supervisor deadline slice (source only, 2026-11-14)

Supersedes the old now+2s cleanup loop above. One OwnedSession owns the monotonic
Stopwatch, Supervisor, ledger/decoder/channel, Job and failure-only waiter for
both filters. The waiter consumes neither frames nor ledger and cannot finish
successfully. No delegate, caller callback or caller clock can authorize success.
Pure checks drive the very same Supervisor with synthetic elapsed times and
native-operation return observations; they never instantiate OwnedSession,
NativeJob, HeldFiles or a real thread/clock/process.

The initial no-frame cap is 10000ms. Each accepted BEGIN installs its absolute
rowStart+10000 cap, clipped to the session's absolute 360000 cap. END and empty
polls never refresh that cap, including after row 34. Failure begins at cap-2000:
2000ms is explicitly reserved INSIDE the cap for termination and confirmation.
This is a conservative work cutoff, not a measured guarantee that native calls
finish in 2000ms. KillAndConfirm receives that same absolute cap, and its polling
loop cannot add a fresh two seconds. Exact deadline equality cannot confirm
cleanup or success. The global cap also bounds the retained last row's teardown.

Launch/admission is counted outstanding before synchronous work starts. A
blocked CreateProcess or assignment does not hold the Supervisor gate. The
waiter can latch failure and invoke Job termination independently. Created and
Assigned observations from a late return cannot authorize resume after failure.
The suspended owned process handle is published before assignment; late-created
unassigned suspended children are explicitly terminated by owned HANDLE, after
the Job termination attempt. A completed Job termination is not treated as a
persistent fence against future assignment. NativeJob rechecks cancellation
at each late completion, then kills again rather than resuming. Resume admission
is a one-shot owner transition, never a waiter transition. An already-authorized
in-flight ResumeThread is subject to Job/process termination; pure scheduling
does NOT prove an atomic relation between that transition and kernel execution.

Failure is irrevocable. Roots are retained before potentially blocked kill
calls; this is ownership bookkeeping, not a cleanup result. Native cancellation and final handle disposal share a lifetime gate; failure
latching never waits on that gate. Exited filter handles are retired until
final teardown; no second filter launches after failure. Successful final
admission requires fresh Poll, the full ledger, empty valid channel, exit 0,
active 0, and zero outstanding launch operation, under the same state gate as
failure latching. Both filters must first have observed exit 0 and active 0. Native handle
teardown is counted outstanding and finishes before the final clock sample;
no native query follows that sample. The waiter
cannot change a completed owner state into a successful result.

Technical limit: arbitrary synchronous kernel calls (including termination,
query, resume, close, CreateProcess and assignment) and OS thread scheduling
have no proven finite upper bound. The waiter requests termination; it cannot
kill a blocked thread, certify kernel cleanup, or return a blocked owner call.
A stuck call remains failed/cleanup UNKNOWN, retaining its kill-on-close owner
and handles; there is no success, row reuse or claim of complete cleanup within
the cap. Only confirmation polling is capped; native operation wall time is NOT
bounded by this source. This is a source state-machine slice, not a proof of
Windows runtime semantics or native readiness. Fresh independent Grok review
is still required; none is performed by this implementation sub-agent.

Unchanged: final admission refusal, RequireTelemetryBinding throw before
CreateProcessW, GATE2_ONLY, registration false. No handle-delivery/argv/env grammar,
duration producer measurement, environment selection or registration is solved.
Native behavior UNRUN; environment/nested Job/budgets unmeasured and unselected;
M2 incomplete; main71 cause unknown; no historical retry; Phase 1 has no native
red loop; real-runner PA6 OPEN; **unverified on real hardware**.
