# M2 A4 runtime-blocker resolution: standalone prebuilt helper

2026-11-15; design sub-agent (other-provider handoff after repeated A4 helper
blockage). Base `bf89f30` (Sol evidence, same blob as
`1d54a54545d285dd90d3359c8c08c0adb46e8c7e`), containing merged PR101
`9d5be07` and [linked-runtime audit blocker](m2-a4-linked-runtime-audit-blocker.md).
Read AGENTS/KICKOFF, ADR0020 D9a, brief0020 A4, the single-task helper proposal
and both Muse rechecks. Document only; no source, build, fork, native run,
owner step, dispatch, publication or push. This resolution amends only ADR0020
D9a binding 1 alongside this file; it grants no implementation or run authority.

## Determination: the linked closure cannot be discharged from here

The settled contract requires proving the entire linked Linux
atfork/errno/handler closure of the A4 test binary. Available evidence cannot
practically discharge that proof:

- The checkout has no `target/` directory and no linked Linux A4 test
  executable, so there is no linked artifact whose registrations, accessor
  implementation or handler bodies could be enumerated.
- The pinned 1.95.0 toolchain contains Linux cross-target rlibs but no
  `lib/rustlib/src/rust/library` source tree (re-checked read-only on this
  host: that path is absent), so no complete matching libtest/std source
  closure exists to review handler bodies against.
- The locked `libc` 0.9.2 declaration of `__errno_location` is an extern
  declaration, not the loaded Linux libc implementation; no sysroot or loaded
  libc identity is established. `signal-hook-registry` evidence covers one
  crate's guard, not the inherited handler set at the fixture's fork point.
- A normal hosted compile (`cargo test --no-run` on Linux) would produce a
  linked binary, but discharging the contract would still require a new audit
  pipeline over it (registration enumeration plus per-handler body and exact
  accessor review against matching libc/std sources, which the pinned
  toolchain does not ship). That is optional audit infrastructure, against the
  root priorities: proportionate protections and functioning M2 acceptance with
  no owner approval overhead or optional audit layers.

Sending Sol back to the same unavailable bundle, or opening another
documentation-clarification loop on the fork-branch wording, is therefore
rejected. The resolution is a concrete contract amendment below, not a proof
attempt and not a generic harness.

## Chosen approach: one tiny fixed standalone prebuilt helper

Replace D9a binding 1's in-test post-fork waiting branch with one fixed
standalone prebuilt waiting executable. Rejected without further analysis:
keeping the in-test fork (same unavailable bundle), `current_exe` re-entry
(already refused by `d8e5332`: harness thread plus worker breaks `pids.max=1`),
and a hosted binary-forensics pipeline (optional audit infrastructure).

- **What it is.** One waiting program built once from one pinned source file
  using only direct async-signal-safe syscalls
  (`close`/`clock_gettime`/`poll`/`read`/`write`/`_exit`), the same call list
  already accepted for the child in the [helper review](m2-a4-single-task-helper-muse-review.md)
  against [fork(2)](https://man7.org/linux/man-pages/man2/fork.2.html) and
  [signal-safety(7)](https://man7.org/linux/man-pages/man7/signal-safety.7.html).
  It creates no thread or process, calls neither fork nor exec, registers no
  atfork handler, takes no commands, opens no network/storage and runs no
  probes. Because it never forks, no atfork handler ever runs in its image;
  because it is entered by exec, no libtest/std/transitive code exists in its
  address space. Its full closure is therefore enumerable from its own pinned
  source, unlike the test binary's.
- **Build/provenance.** Exact build command (pinned compiler, flags, source
  hash), resulting SHA-256, size and symbol inventory (no thread/fork/atfork
  surface) are recorded by the implementing source slice before use; the
  parent fixture verifies SHA-256 and size before spawn and spawns nothing on
  mismatch. The binary is delivered with the disposable-host driver assets
  outside the Cargo workspace (not a Cargo target, no production entry), and
  the fixture references it by absolute path constant plus hash.
- **Invocation.** The ignored fixture spawns it only through the E4-reviewed
  descriptor-bound fork+execveat spawn shape against a validated executable
  descriptor (no PATH, no `Command`, stdio on `/dev/null`, close-on-exec
  error pipe, 5 s polling deadline with own-child SIGKILL and reap per the E4
  S8/S13 precedent). No new post-fork Rust branch is added. Parent-image
  atfork exposure across that window is exactly the already-accepted E4 basis,
  disclosed in D9a, not a new proof.
- **Handoff contract.** Only the two handshake fds are inherited non-CLOEXEC
  (every other fd CLOEXEC); the absolute monotonic deadline and the two fd
  numbers travel as fixed bounded-decimal argv under a strict parser. The
  helper sends one fixed ready byte, waits on the release pipe with
  EINTR-only retries that recompute a saturating monotonic remainder without
  extending the deadline, and `_exit`s with fixed codes otherwise.
- **Isolation from normal discovery.** The helper lives outside the workspace,
  is never on PATH, is referenced only by the ignored A4 fixture's absolute
  path constant, and performs no tool output; `discover_tools`, inventory and
  the tool cache cannot surface it. Pure tests keep the existing mock-only
  seam and never touch it.
- **Preserved bounds (unchanged, bindings 2–5).** Single-task `pids.max=1`
  with 64 MiB / swap 0 written before placement; literal `arx-0-0`,
  Created-only, no adoption; spawn outside the limited leaf, migrate exactly
  one task, verify live membership/population plus all parent/supervisor
  preconditions before release with >6 s of the 15 s lifetime remaining;
  only S7 kills on the success path; parent-held identity with
  `waitpid`-proven SIGKILL; exact postconditions; failure-only owned cleanup.

## Amendment status and remaining blocker

This file plus the D9a binding-1 edit is an explicit contract amendment: the
prior fork-branch proof obligation is superseded, not silently weakened. It
takes effect only after Sol independent review plus green required checks and
coordinator merge. Implementation (helper source/build pin, fixture spawn
slice) needs its own small reviewed PRs under normal Sol-builds /
different-provider-review routing; native A4 execution still needs a separate
bounded disposition and acceptance review. The brief's A4 bullets need a
conforming follow-up edit outside this bounded scope, as does PROGRESS.
No A4, M2, full F/G or PA6 acceptance is granted here; native, installed and
dedicated paths remain **unverified on real hardware**.
