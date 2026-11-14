# Windows main71 native replay: Gate 2 controller contract recheck

Disposition: the Sol correction removes the unsafe dormant launcher and
aligns the 35-row schedule. The owned-deadline, nonblocking channel,
completion predicate, and minimal launch grammar are in-memory contracts.
They are not an immutable launch, a telemetry-to-Job adapter, an explicit
environment block, a nested-Job result, or a measured budget.
**Native run-readiness is NOT CLEARED.**

Local unpublished retention of this undiscoverable preparation is acceptable.
This review grants no remote publication, PR, CI admission, or native run.

Reviewer: xAI / grok-4.7, one independent pass, 2026-11-14. The assignment
forbids subagents, so Standards, Spec, and security are this single reviewer.
Independence is from the Sol implementation, not from a second reviewer.
Label: single-agent review.

M2 is **incomplete**. The cause of run `43545244266` attempt 1 is **unknown**.
Phase 1 still has no focused red loop that reaches that failure. Real-Runner
PA6 record/replay remains **OPEN**. Native, installed, and dedicated paths
are **unverified on real hardware**. No retry was issued.

## Pins

Initial porcelain was empty and remained empty after the checks below. The
review cwd was the linked worktree `<isolated-worktree>`. Its Git directory
is a per-worktree directory under the common Git directory. Paths below are
repository-relative.

| Item | Identity |
| --- | --- |
| Review branch | `t3/review-controller-correction` |
| Prepared head | `065123cffab4683f26b334084157ab172ec854e7`, also the tip of `docs/windows-native-replay-scope` |
| Head tree | `278dbcbdd9d36272218d16e0991e07a4600b85a2`, identical to evidence `b2059eed602e8caf42a026ce57035c21c17195fd` |
| Source correction | `34ec9dc1487e3e5bd24bb19e89541bb11ba64e21` |
| Source tree | `0b2271fe69b939f15bd53ec85aef2586d9965455`, identical to supplied `c1f1255aea3f8b6857688d75e61acbc69868a89a`. Same parent `33f5f4e9be3fde99a7badeee45b3b98a92dc6649` and the same patch-id |
| Evidence patch | `b2059ee` and `065123c` share patch-id `3d50892464f01a86a9da8dc505324296468ab151` |
| Prior Grok review | retained `4518385c9bdbcce100f6d42bfc03f33202142957`, tree `9d1c79d44f3e859059c608b8f83ec5f37e6b6132`, identical to supplied `b2dec922330d040513257317eda15c62dbcb5b23` |
| Prior review blob | `ad3aee74e06fb1ab62217426eb01797c80f3d608` |
| Fixture source retained | `beb590c4e40f225b388bcbebb5d754e8ff0a1369` and `3eedb7736e8876dc1d8907a0d0bfdec3f06ca5ba` share tree `8ee82d5639eed05f4cc39f4756702530aed53f12` |
| Accepted proposal blob | `366876f711595c14bde42763e2a263cfe44ca9c2`, unchanged |
| Correction diff | `git diff 33f5f4e9be3fde99a7badeee45b3b98a92dc6649 065123cffab4683f26b334084157ab172ec854e7` — 8 files. No workflow, root Cargo, policy, ADR, proposal, review, or PROGRESS path |
| Branch since prior review | `4518385..HEAD` also contains `33f5f4e`, the two-line pre-correction PROGRESS note. This recheck does not edit PROGRESS |

Source pins at this head match the evidence table: `windows.rs` blob
`c949e47dae5ac6998d6f406b250c761a43ab9d89`, LF SHA256
`48499b5014da7106513f5bd190c0de6300f0e71ce7086c9a70620c7db5980895`;
`Controller.cs` blob `73569217b56b6b8ca7bbdd85bc981ccc7f713e31`, LF SHA256
`76ad429af198533a1155170a150680b06cf82c24b1fc4a2524d8dab7906806c5`;
`Check-Source.py`, `PINNING.md`, `Prepare-Provenance.py`,
`Test-PureController.ps1`, and `selectors.json` blobs and LF SHA256 values
match that table. CR count was 0 for each pinned file checked here.

## Prior blocker disposition

| Prior item | This tree | Credit |
| --- | --- | --- |
| ST-INDEX | Canonical `selectors.json` `row_map`, C# projection, and Rust `matrix_index` agree: kernel 0, failures 1 through 28, peek-109 29 through 32, controls 33 and 34 | Numbering closed. Rust still emits no begin/end events, and `Budget` is not fed by a harness. Do not credit a live index transport |
| SE-DEADLINE | `RunPinned` and its caller-supplied stopwatch are gone. `OwnedSession` starts one `Stopwatch`. `MonitorState.Poll` reads one nonblocking event and fails closed on a 10 s silence or the 360 s global limit | Reachable unowned-clock hole closed by removal. No native kill uses this clock. The native deadline stays open |
| SE-COMPLETE | `Success` requires an unfailed empty channel, `Budget.Complete` (35 / 20 / 16), exit 0, and active processes 0 | Reachable early-success hole closed by removal. `Success` does not read the clock, and nothing calls it against a process. The native completion gate stays open |
| SE-INHERIT | No `CreateProcess` and no null environment block remain in the controller. `LaunchSpec.Valid` accepts only a drive-absolute toolchain `cargo.exe`, a 12-element argument vector, and exactly `PATH` / `TEMP` / `TMP` / `SystemRoot` | Reachable inheritance hole closed by removal. The method is a pure grammar. It retains no immutable path, handle, hash, or UTF-16 block. Immutable launch stays open |
| Provenance hardlink | `bounded_hash` refuses `st_nlink != 1` before the read. Sources and the helper share that function. Reparse refusal is unchanged. The source list only grew | Refusal stands. Builder evidence: the built helper had link count 2 and the utility printed `provenance unavailable`, with no copy and no alternate hash path. This pass did not re-execute it. The utility is still a snapshot, not a held-handle pin |
| Open gates | Environment selection, nested-Job proof, measured 300 s + 360 s + 60 s budget, immutable manifest, native telemetry | Still open. `PINNING.md` labels them unimplemented and grants no run authority |

`OwnedSession`, `RowChannel`, `Budget.Complete`, and `LaunchSpec` are
preparation seams. A green pure test of those seams does not close the
native finding they model.

## Standards

**PASS. No hard finding.** One baseline judgement, not a blocker.

The Rust edit stays inside `#[cfg(test)] mod native_replay`. `bounded_matrix`
and `kernel_peek_capture` still have no `#[test]` or `#[ignore]` and no
caller. The new assertions sit in the existing pure test
`oracle_counts_missing_zero_and_literal_bindings`. Root workspace members
remain `crates/arxd` and `crates/arx-guest`. The helper manifest is still
`publish = false`, `test = false`, `bench = false`, and its body is
`fn main() {}`. `Invoke-BoundedReplay.ps1` still throws `GATE2_ONLY` before
any process, Cargo, or `Add-Type` call. The product `CreateProcessW` path
and cleanup order are outside this diff. No workflow, CI, policy, or release
fence file changes.

Repo rules checked: conventional scoped docs, no credentials, no owner
paths, no product approval flow, and no claim for a check this source did
not run. The evidence note calls the result prepared-partial and says the
native findings are not closed.

Fowler baseline: **Mysterious Name**, judgement only. `OwnedSession` names
the future containment obligation. The type owns a clock, a bounded queue,
and a budget. It does not own a process or a Job. The file comment and
`PINNING.md` already say that. A documented contract requires the seam, so
this is not a hard naming violation.

The earlier duplicated index formula is now `selectors.json`, a checked C#
projection, and an independent pure oracle. The accepted proposal requires
that oracle to stay literal. No documented standard demands a shared helper,
and the smell is not re-filed.

## Spec

**0 findings of wrong or over-claimed behavior.** The prior numbering
contradiction is closed. The run contract's remaining requirements are
unimplemented, and the diff says so.

Checked against the accepted proposal and the prior repair:

- `row_map` has 35 rows. Kernel is index 0 with consumed/creates/resumes
  `0/0/0`. Failures 1 through 14 are `1/0/0`, 15 through 18 are `1/1/0`,
  and 19 through 28 are `1/1/1`. Peek-109 rows 29 through 32 are `1/1/1`.
  Controls 33 and 34 are `0/1/1`. The C# `Rows` array matches every one of
  those entries. Filter 0 is index 0 only. Filter 1 is indices 1 through 34.
  Totals remain 35 cases, 20 children, and 16 resumes.
  `registration_enabled` is false.
- `ORACLE` still has 28 literal failure rows. Local 0 through 13 create no
  child, 14 through 17 create and do not resume, and 18 through 27 create
  and resume. `matrix_index` maps those locals to global 1 through 28,
  the four peek calls to 29 through 32, and the two controls to 33 and 34.
  `matrix_index(34)` panics. The 28 labels, pass/totals columns, and
  missing-versus-zero rules are unchanged.
- `kernel_peek_capture` still observes create/resume/consumed `0/0/0` and
  does not carry an index argument. Index 0 is reserved by the map because
  the matrix starts at 1. Begin/end emission is absent, as the evidence says.
- `LaunchSpec` allows `Kernel` or `Matrix` on each call. It does not record
  "once each", retain the strings, or build a Unicode environment block.
  `PINNING.md` leaves those to a later adapter.
- Pure `Success` matches the prior completion predicate for the model:
  35 accepted ends, 20 children, 16 resumes, no open row, no extra event,
  exit 0, and active 0. Missing, duplicate, out-of-order, overflow, and
  unknown-cleanup events fail closed in that model.

Not credited, because the code and `PINNING.md` leave them unimplemented:

- an immutable launch manifest and held reparse-free handles
- a UTF-16 environment block passed to a process
- Rust begin/end records feeding one session across both filters
- an owned Job that the deadline can terminate
- a selected disposable standard account and a nested-Job proof
- a measured 300 s + 360 s + 60 s budget

## Security

**0 reachable findings.** The prior three launcher holes are closed by
deletion. Their native repairs are not implemented. No new launch path
replaces `RunPinned`.

`Controller.cs` contains no `DllImport`, `CreateProcess`, `RunPinned`,
`Func<`, process type, or shell. The only public entry in that file is
`PureControllerChecks.Run`, which uses literal tables and synthetic times.
It does not construct `OwnedSession`. `OwnedSession.Poll` is nonblocking
and reads the session's own stopwatch, but no production caller constructs
the session. The PowerShell entry cannot reach the controller: its first
statement throws. Parameter validation cannot enable execution.

A grammar acceptance from `LaunchSpec.Valid` cannot create a process.
`Prepare-Provenance.py` still sets `runtime_authority` false and exits 1
on the hardlink refusal. There is no second helper-hash path that skips
`st_nlink != 1`. The old reparse-ancestor and reparse-leaf checks remain
before that link-count check. Builder evidence that the compiler output
had link count 2 was refused is consistent with this source. This pass did
not create a link, copy the helper, or weaken the check. Check-then-open
remains insufficient for a later authorization pin; `PINNING.md` already
requires held handles and a copied `cargo.exe` with link count one.

The outer product Job, zero-timeout assign observer, and non-fixture
duplicate refusal are outside this diff. Nested Job behavior was not
executed. No account, service, VM, owner profile, or Lab Manager state
was inspected.

## Local preparation and run readiness

**Local unpublished preparation: acceptable.** Native replay functions
remain undiscoverable. The selected pure filters ran 4 native-replay tests
and 6 diagnostic tests; both reports filtered the rest of the library
(362 and 360) and ignored 0. No workflow or discovery entry was added.
The helper is not a root workspace member. Nothing in the correction can
bypass the `GATE2_ONLY` throw.

**Native RUN READY is NOT CLEARED.** Retaining this head locally is a
separate decision from launching it. No remote, PR, CI, budget admission,
or native execution is authorized here.

## Smallest remaining implementation contract

This is the unimplemented adapter already required by `PINNING.md` and the
prior repair. It adds no account, measurement, registration, or other
authority. Environment selection, the nested-Job proof, and the
300/360/60 measurement stay separate open gates. Writing the adapter does
not pass those gates.

1. Immutable launch. Add the fixed manifest whose SHA256 is pinned outside
   the manifest. Hold non-following ancestor and leaf handles across launch
   and teardown, with link count exactly one, and hash from those handles.
   Bind the application, working directory, and the existing 12-element
   vector, Kernel then Matrix, once each. Build one fresh sorted UTF-16
   double-NUL environment containing only the four `LaunchSpec` values.
   `LaunchSpec.Valid` does not do this. Do not put profile variables into
   that block and do not let a clean-environment `cargo test` rebuild an
   unpinned binary. No shell and no null `lpEnvironment`.
2. Nonblocking telemetry. Emit actual begin/end records for every row,
   including kernel index 0, with the observed counters. Feed one
   `OwnedSession` created before launch and kept across both filters.
   Poll its channel; do not add a callback. Incomplete, duplicate,
   out-of-order, and overflow frames latch failure. Discard raw harness
   output. `Success` does not read the clock, so the adapter's last
   observation before `Success` must be `Poll` on that owned clock.
3. Owned-Job integration. On any deadline, telemetry, or containment
   failure, terminate the owned outer Job before returning, keep
   kill-on-close until active processes are 0, stop the remaining rows,
   and forbid reuse. Success requires the predicate already in
   `MonitorState.Success`. No PID or name kill. The nested-Job proof in a
   selected standard account remains a separate open gate.

## Evidence from this pass

Compiler: rustc 1.95.0, commit `59807616e1fa2540724bfbac14d7976d7e4a3860`,
LLVM 22.1.2. Cargo 1.95.0 (`f2d3ce0bd` 2026-03-21). Target
`x86_64-pc-windows-msvc`. `CARGO_PROFILE_DEV_DEBUG=0`. Locked and offline.
No install and no network.

The pure tests were read first. `native_replay::pure` uses TLS diagnostics,
the literal oracle, `matrix_index`, and `parse_case`. `diag::pure` is
unchanged by this diff and stays on tables, formatting, and in-memory
counters. Neither filter calls the native replay functions. The managed
script compiles `Controller.cs` in memory, calls `PureControllerChecks.Run`,
and applies five source substitutions in memory. It does not construct
`OwnedSession` or call a process API.

```powershell
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure:: -- --test-threads=1
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::native_replay::pure:: -- --test-threads=1
```

The first filter passed 6 tests, with 360 filtered out. The second passed
4 tests, with 362 filtered out. Both reports were 0 failed and 0 ignored.
The harness filename was `target/x86_64-pc-windows-msvc/debug/deps/arxd-5547724799c07633.exe`.
This pass did not re-hash that executable or the helper, so the builder
artifact SHA256 values stay builder evidence.

`Test-PureController.ps1` passed its baseline and assertion-killed all five
mutants: row 15 create count, removed `Budget.Complete`, `>=` changed to
`>`, arbitrary filter, and removed PATH equality. The Controller LF SHA256
was unchanged, `76ad429af198533a1155170a150680b06cf82c24b1fc4a2524d8dab7906806c5`.

Builder evidence, credited because the source blobs match and not re-executed
here: `cargo fmt --all -- --check`, the helper manifest fmt check, and
`cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings`,
all recorded against source tree `0b2271fe69b939f15bd53ec85aef2586d9965455`.
Also builder-only: `Check-Source.py`, the helper build, and the hardlink
refusal. Native capture, the 35-case matrix, cleanup, nested jobs, and
native mutants are **UNRUN**.

## Close

Standards: 0 hard findings; 1 judgement, `OwnedSession` names a Job it does not own.
Spec: 0 findings of wrong or over-claimed behavior. The open run-contract gap is the unimplemented immutable launch and owned-Job telemetry adapter.
Security: 0 reachable findings. The prior launcher holes were removed; the worst remaining gate is the absent owned-Job termination.
**Native run-readiness is NOT CLEARED.**

Performed: read-only Git inspection, blob and LF SHA256 checks of the pinned
correction files, the two pure Rust filters, one managed pure run with five
assertion mutants, and this file's write and commit. Not performed: source,
proposal, or PROGRESS edits; fmt or Clippy re-execution; helper, controller,
or PowerShell launch-entry execution; provenance re-execution; native
functions; Windows API experiments; child launches; ordinary tests; full
suite; owner, account, service, VM, or private-state inspection; network;
GitHub; install; CI; push; subagents.
