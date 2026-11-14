# Gate 2 launch integration plan (PARTIAL SOURCE, NO RUN AUTHORITY)

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
not manifest admission. Directory handles do not prove exclusive helper-directory
membership. The Cargo grammar also needs a reviewed way to exclude rebuilding
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
