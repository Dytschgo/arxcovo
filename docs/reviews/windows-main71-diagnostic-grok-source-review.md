# Windows main71 diagnostic source review

Disposition: **no source blocker.** Security, standards, and spec are separate
below. The cfg(test) capture wiring matches the accepted contract, including
the coordinator's missing-elapsed disposition. Two spec items become blocking
only before a native replay is authorized. This review grants no publication,
merge, CI, root, or run authority.

Reviewer: xAI / grok-4.7 / high, one independent pass. The assignment forbids
subagents, so both review axes and the security check are this single reviewer.
Independence is from the Sol implementation, not from a second sub-reviewer.
Debug phases 2–6 are skipped: the incident still has no authorized red-capable
native loop, and this assignment forbids a native repro, a historical retry,
and source edits. No cause and no fix are claimed.

## Pins

| Item | Identity |
| --- | --- |
| Review cwd | Isolated linked worktree. Git top level equals the review cwd. Status was empty before this commit |
| Governing HEAD | `19e7463d73c0e4271d7a7e9aaa3bd96809188619` |
| Main base | `e3f70ef56007ce689d58e4c7fe024e456a80d997` (`git merge-base --is-ancestor` succeeded) |
| Accepted proposal at HEAD | `docs/reviews/windows-main71-feedback-proposal.md` blob `eaf4e7afa87eebb12a2e6fe3a9c14923adb95e0e` |
| Elapsed disposition | `19e7463`, parent of the evidence commit. `ms=-` is allowed. No new success-path native call. Timeout clocks stay put |
| Source commit | `b9aa1258967b19486fd4ee6954cf4affa5047239`, parent `1741419efbe8add6195b723bdeeb5d6ba0714142` |
| Retained source commit | `cf915a4518d1a94a2baa6175335fde88fd5fa2fd`. Same tree `3670743a66df65144023fce26102719c19138065` |
| Reviewed file | `crates/arxd/src/development/windows.rs` blob `0f61e58a92177d78d4aad4fc0e03292b90cfb554` (1917 lines, 69369 bytes) |
| Baseline blob | `85f0c0eca19950fc65a02b621ff752eb6ba61551` (1085 lines, 41037 bytes), present at `e3f70ef` and unchanged until the source commit |
| Code diff | That file only: 853 insertions, 21 deletions (874 changed lines). Same blob at `b9aa125`, `cf915a45`, and HEAD |
| Evidence | `3d39d87afeaa90bd14bab8e82c3538f95577e7ec`, tree equal to retained `e696717bd98b84d99f33e8edadc897a72ff1dd16` |
| Prior reviews read | Sol `43bb26d0cd8342b9c1d9887e6bc84b2cd18b3fac`, Opus `0da16cd693742ad11d1116af5e10657084d786f3`, Grok `0ad3dea6dbbd53f53610857b3dfd9904cd334dc0` |
| Retained Grok commit | `7acf278ff52075c0021c2079c634862ba591f1bd` is not an ancestor of HEAD. Its review blob equals `0ad3dea`: `cc7cee3887ad70f7e6a796bb581868d1797c0b6f` |
| Lockfile | `Cargo.lock` blob `1c5c0fcf2b577f89ed998826ddf6608338b4b523`, unchanged |
| Toolchain this review | rustc 1.95.0 `59807616e1fa2540724bfbac14d7976d7e4a3860`, 2026-04-14, host `x86_64-pc-windows-msvc`, LLVM 22.1.2. cargo 1.95.0 `f2d3ce0bd` |

The 21 deletions are the broken-pipe save, the daemon `facts` block, the two
`drain` call shapes, the `0..32` loop binding, ten expected-success
`run_inner` unwraps, and the `0..8` loop binding. Refusal matches at the
junction and identity-mismatch tests still call `run_inner` and still expect
`InvalidArguments` or `IdentityMismatch`.

## Standards

**PASS. No finding.**

Docs read: `AGENTS.md`, `CONTRIBUTING.md`, the Review skill, and the Debug
skill. The change is one conventional source concern in one file. The pure
module has meaningful tests for its own behavior. The spec forbids a native
site test in this change, so the missing native test is not a standards
breach. Clippy `-D warnings` is clean for the package. No credential or owner
path was added.

Fowler baseline: no smell is filed. A second responsibility inside
`windows.rs` and the three capture shapes would otherwise look like divergent
change and duplicated code. The accepted proposal requires the macros at the
real sites and requires the assign snapshot to differ, so those shapes stay.

## Spec

**PASS for the capture contract. Two non-blocking findings. Both block a
later native replay until closed. Neither blocks this source.**

Checked against the proposal at HEAD, including the elapsed-field section.

Implemented and inspected:

- Non-test `diagnostic!`, `stage!`, and `drain_at!` expand without evaluating
  their discarded arguments. `drain_at!` keeps only the original call.
  `stage!` and `diagnostic!` expand to an empty block.
- The diag module, `run_ok`, `diagnostic_failure`, `diagnostic_snapshot`,
  `Invocation`, and the peek injection branch are `cfg(test)`.
- `Scope::enter` resets state. `Invocation::enter` clears captured state and
  leaves `armed` in place. Both guards carry `PhantomData<Rc<()>>`, so they
  are `!Send` and `!Sync`. Re-entry asserts. Drop clears an unwinding scope.
  `capture` calls its reader before it touches the `RefCell`. `enrich` copies
  the record out before the snapshot closure, so the native queries do not
  run under a live borrow.
- Actual `GetLastError` reads happen on the failure branch before cleanup.
  `ok` reads it before returning `ExecutionFailed`. Drain stores the peek
  error once and reuses that value for the 109 decision; 109 still returns
  success. Attr-size records the observation with `valid=false`. Wait stores
  the raw return and sets `valid` only for `WAIT_FAILED`. Assign calls
  `capture(|| GetLastError())` before `TerminateProcess`, then snapshots.
  The `stage!` injection path returns before `AssignProcessToJobObject`, so
  its snapshot is still before the outer terminate. Those two snapshots are
  different observations.
- Snapshots run only after a record was stored, which requires an active
  invocation. They use the current-process pseudo-handle and the borrowed
  process and job values already stored. Query failure leaves the field
  missing. No `OpenProcess`, `DuplicateHandle`, new handle, or wait with a
  non-zero diagnostic timeout was added. There is one `CreateProcessW`, and
  `create_call` sits after the injection return and before that call.
- `line` keeps the actual `Reason` token. Any other reason replaces the
  record with empty fields. The fixed layout has no path, argument, SID,
  environment, output, or handle value. The pure bound test accepted every
  stage and context at `u32::MAX` / `u64::MAX` under 384 bytes. `ms` is
  stored as `None`, so the line emits `ms=-`. `Instant::now()` after resume
  and the `timeout_s` comparison are the baseline statements. The `0..8`
  loop passes its index into `run_ok`.
- The literal manifest has 18 baseline origin numbers and 28 labels. This
  review re-read those baseline lines in blob `85f0c0e`. The test compares
  the label set and each `resolve` pair. It does not read the source file.

**SP1. Non-blocking now. Blocking before native replay.** `run_ok` creates
the scope and drops it before the caller resumes. `Scope::drop` zeroes
`calls`. `create_calls()` after `run_ok` is therefore 0 even when
`CreateProcessW` ran. An assertion written against `run_ok` would false-pass
the required zero-call check. The working seam is the one the pure lifecycle
test uses: hold `Scope`, `arm`, call `run_inner`, then read `create_calls`
before the scope drops. `arm` also asserts `scoped && !running`, and a second
`Scope` inside `run_ok` asserts on nesting, so `run_ok` itself cannot be
armed.

**SP2. Non-blocking now. Blocking before native replay.** Actual wait and
attr-size failures record raw wait validity and `valid=false`. Generic
`stage!` injection always captures `valid=true` and `wait=None`, and it
returns before the API. A future injected wait or attr-size case would not
match the actual-capture contract. Peek 109 is special-cased and still
returns success.

## Security

**PASS. No finding.**

Release and non-test library code gain no diagnostic module, label, control
string, TLS, formatter, or executable diagnostic logic. The controlling
evidence is the `cfg` and the macro bodies above. This worktree has no
retained LLVM IR artifact. This review did not rebuild one, did not execute
one, and makes no binary-equality claim. Clippy compiling the non-test
library is supporting compile evidence only.

Success-path diagnostic work is in-memory bookkeeping: role, totals, pending
stage, borrowed handle numbers, and the create counter. Failure snapshots are
test-only queries of handles the function already owns. They cannot replace
the primary error. The panic line is closed vocabulary plus numbers. Handle
values stay in the thread-local state and are cleared with the scope. Job
limits, token and path checks, cancel and shutdown order, quarantine store,
and the two-second `job.stop` clock are the baseline statements. Injection
that returns `Err` from inside the post-create closure still reaches the
existing outer terminate, `job.stop`, and quarantine store.

Token-info still has one label for User, Elevation, Integrity, and Groups.
That merge is the accepted contract. `preflight` and the sentinel helper do
not enter `Invocation`, so a failure there reads `GetLastError` through `ok`
and then discards it. The published line exists for `run_inner`.

## Tests this review ran

Transitive inspection of `development::windows::diag::pure` came first. The
four tests call `Stage`, `resolve`, `Scope`, `arm`, `Invocation`, `stage`,
`capture`, `failure`, `create_call`, `create_calls`, `record`, `line`, and
`Reason::as_str`. The ordering test passes a `Cell` reader (`123`) and a
closure that then stores `999`. Its observe closure is not
`diagnostic_snapshot`. The module contains no `GetLastError`,
`SetLastError`, `Instant`, process launch, or `unsafe` call.

Final source, exact filter, cold `target/`:

`cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure::`

Result: **4 passed, 0 failed, 356 filtered out, harness 0.00 s, compile
54.47 s.** Names: `independent_mapping_and_module_mutants`,
`lifecycle_one_shot_unwind_and_counter`,
`error_reader_precedes_mock_cleanup_and_preserves_first`,
`complete_format_bounds_indices_validity_and_privacy`.

`cargo fmt --all -- --check` passed. `cargo clippy -p arxd --all-targets
--locked --offline --target x86_64-pc-windows-msvc -- -D warnings` passed in
28.78 s. Clippy type-checked the package targets, including test targets, and
executed none.

The builder's four passing tests were on a precursor. This filter ran the
final blob `0f61e58`. Credit is the pure module on that blob.

## Limits

- The pure filter does not reach Win32, prove marker reachability, deletion
  or mislabel mutations of the real sites, loop-index call-site mutations,
  native last-error order, quarantine under injection, a zero `CreateProcessW`
  count, never-resume behavior, cleanup equivalence, timing, or the historical
  failure.
- Debug phase 1 for the main `ExecutionFailed` incident remains open. No
  focused native loop was built. `ms=-` leaves entry elapsed unavailable. It
  is not a timing measurement.
- Evidence `3d39d87` still calls the missing elapsed field a contract blocker.
  That sentence predates `19e7463`. The source matches the later disposition.
- The IR inspection in the evidence stays the builder's pinned claim. It was
  not repeated here.
- M2 is incomplete. The main incident cause is unknown. PA6 replay remains
  open. Native, installed, and dedicated paths are **unverified on real
  hardware**.

Not run: full suite, the other Windows tests as tests, CLI, fixtures,
`run_inner`, replay, owner, account, service VM, private state, install,
network, GitHub, and CI. No source file was edited.

## Close

Standards: 0 findings. Spec: 2 findings; worst is SP1, the `run_ok` scope
erases the `CreateProcessW` counter before the caller can read it. Security:
0 findings.
