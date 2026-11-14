# Windows main71 diagnostic module: bounded local implementation

Provider: OpenAI / gpt-6.1-sol / high. Date: 2026-11-14.
Disposition: **prepared partial source; elapsed-time contract blocker remains**.
No independent source review has occurred. Owner-directed Muse/Grok review is next;
this implementation sub-agent did not delegate, publish, push, create a PR or merge.

## Pins and scope

Initial clean HEAD: `1741419efbe8add6195b723bdeeb5d6ba0714142`.
Git top level equalled the supplied cwd. Git metadata identified an isolated linked
worktree (separate worktree Git directory and common repository directory).
`git merge-base --is-ancestor e3f70ef HEAD` succeeded; main base resolves to
`e3f70ef56007ce689d58e4c7fe024e456a80d997`.
Initial and reviewed Windows source blob:
`85f0c0eca19950fc65a02b621ff752eb6ba61551`.
Read AGENTS, full KICKOFF, accepted full proposal, Sol review `43bb26d` (retained
`d2bd9d3`), Opus `0da16cd` (retained `5f357fd`), and Grok `7acf278` (retained
`0ad3dea`). The later accepted proposal disposition governs local authority.

Source commit: `cf915a4518d1a94a2baa6175335fde88fd5fa2fd`.
Source blob: `0f61e58a92177d78d4aad4fc0e03292b90cfb554`.
Only `crates/arxd/src/development/windows.rs` changed in that commit.
This subsequent evidence commit changes only this document. No workflow, CI,
policy, Cargo, ADR, AGENTS, PROGRESS, proposal or review file was edited.

## Concrete remaining blocker

Entry-relative elapsed time cannot be observed with a new clock read while also
strictly forbidding new native calls on successful runs. `Instant::now()` on
Windows ultimately needs a platform clock. Moving the existing post-resume timeout
clock to entry would change timeout behavior, which is also forbidden.
The prepared source therefore keeps `ms=-` (missing), removes the proposed entry
clock, and leaves the existing timeout clock and comparisons untouched. It does
**not** satisfy the proposal's requested entry-elapsed observation. Review/owner
contract disposition must resolve that conflict; no permission question, scope
expansion or architectural deviation document was introduced here.

## Implemented structure and source inspection

Closed Stage (28 labels), Context (13 fixed actions/controls), Role and Op enums;
copyable fixed-size numeric/missing record; separate armed injection and captured
failure TLS. Scope and Invocation guards carry non-Send/non-Sync markers. Entry
rejects reentrancy and clears stale capture without disarming injection. Scope
exit/unwind clears all state; unscoped invocation exit clears everything. Scoped
invocation exit keeps only the bounded result/counter until its owning scope exits.
No RefCell borrow crosses a reader closure, snapshot, native call or formatting.
No new native replay test or real-run call was added to test discovery.

Injection is only reachable from private test code. It consumes a selected stage
once before that API, skips that API, and follows existing error returns and outer
cleanup. Peek injection with 109 retains broken-pipe success. There is no CLI,
config, MCP or release control. The test-only CreateProcessW counter increments
at the sole actual API call site after its injection check, not at a wrapper's
entry. Pure counter bookkeeping is **not** native zero-child-creation evidence.

Actual failures call the injected-reader capture helper with GetLastError as its
first operation, before TLS access or snapshots. The original `ok` helper handles
11 origins; seven explicit branches handle the rest. Drain saves GetLastError
once before the 109 decision and reuses it for diagnostics. Attr-size marks the
last-error observation invalid; wait retains its raw return, with error-valid
only for WAIT_FAILED. Assign captures before its original TerminateProcess,
then snapshots after that inner termination and before outer cleanup. Its native
snapshot therefore differs from the future injected assign snapshot, which skips
the inner termination. Cleanup equivalence is not proved.

Snapshots use existing borrowed process/Job handles and the current-process
pseudo-handle only. They record handle count, zero-time child wait, child code,
Job active/total/terminated counts and violation flags. Query failures remain
missing, including WAIT_FAILED from the diagnostic child-state query. No new
handle, OpenProcess, DuplicateHandle, token logging or wait beyond zero is added.
Snapshot functions are invoked only on failures. No diagnostic clock call remains
on success; all success-path diagnostic operations are in-memory bookkeeping.

The fixed line fields are: c=context, r=actual Reason token, s=stage, i=iteration,
e=error observation, v=error validity, w=primary raw wait return, ms=entry elapsed
(currently missing), o/d=stdout/stderr totals, p=final-drain pass, h=process handle
count, cw/cc=child wait/code, ja/jt/jk=Job active/total/terminated, jv=violation flags.
Non-ExecutionFailed Reasons discard any stale diagnostic record. The formatter
accepts no paths, arguments, SID, environment, output, handle values or names.
It uses closed vocabulary and numeric options, never truncation or raw dumps.
Max-width tests cover every Stage/Context with u32/u64 maxima and full lines <=384
bytes. Missing-field lines assert distinct nonzero indices 3 and 7 exactly.

### Independent static 18-origin / 28-label inventory

Line numbers below identify the accepted baseline, not the enlarged source file.
The test manifest is literal and independent of Stage::ALL; it is a static source
inventory and module-mapping oracle, not real-site coverage.

| Baseline origin | Labels |
| --- | --- |
| 64 GetTokenInformation | daemon_token_info, child_token_info |
| 89 OpenProcessToken | daemon_token_open, child_token_open |
| 221 CreateJobObjectW | job_create |
| 231 SetInformationJobObject | job_limits |
| 240 SetInformationJobObject | job_ui |
| 286 CreatePipe | pipe_out_create, pipe_err_create |
| 289 SetHandleInformation | pipe_out_noinherit, pipe_err_noinherit |
| 324 PeekNamedPipe | loop_out_peek, loop_err_peek, final_out_peek, final_err_peek |
| 329 ReadFile | loop_out_read, loop_err_read, final_out_read, final_err_read |
| 414 CreateFileW | nul_open |
| 420 attribute-size predicate | attr_size |
| 423 InitializeProcThreadAttributeList | attr_init |
| 430 UpdateProcThreadAttribute | attr_update |
| 448 CreateProcessW | create_process |
| 471 AssignProcessToJobObject | assign_job |
| 478 ResumeThread | resume |
| 508 WaitForSingleObject | wait |
| 512 GetExitCodeProcess | exit_code |

Token-info labels deliberately merge User/Elevation/Integrity/Groups.
Shared helper callers explicitly set daemon/child, out/err and loop/final roles.
Final-drain pass is forwarded; the original 0..8 loop now forwards its real index
to run_ok. Only the ten reviewed expected-success unwrap sites were replaced.
Refusal matches, setup unwraps, eight iterations, `after <= before + 4`, result
assertions, limits, exits, timeout clock, cancel/shutdown order, quarantine,
path/token/Job fences and original cleanup remain intact by source inspection.

## Commands and observed outcomes

All Cargo commands used the pinned toolchain, offline dependency cache and locked
lockfile. No download/install/network command was used.

- `rustc -Vv`: rustc 1.95.0, commit
  `59807616e1fa2540724bfbac14d7976d7e4a3860`, 2026-04-14,
  host `x86_64-pc-windows-msvc`, LLVM 22.1.2.
- `cargo -V`: cargo 1.95.0 (`f2d3ce0bd`, 2026-03-21).
- Initial `cargo fmt --all -- --check`: failed on formatting of newly written code.
  `cargo fmt --all` applied formatting. Final check passed.
- `cargo check -p arxd --tests --locked --offline --target x86_64-pc-windows-msvc`:
  first failed on a malformed invocation-guard macro hook. Replaced it with a
  cfg(test) local guard with the correct function lifetime; next check passed
  (4.80 s). No test was executed by cargo check.
- First `cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure:: -- --nocapture`
  was cancelled during compilation (exit 1, no test results), to remove clock
  reads from the pure module. It did not establish test evidence.
- After inspecting the complete diag/pure module and its transitive helper paths
  for native APIs, snapshots, run_inner/run_ok, unsafe blocks and clock reads,
  that same exact pure filter was executed: **4 passed, 0 failed, 356 filtered
  out; harness 0.00 s; compile+run 16.19 s**. The printed reentrancy and synthetic
  unwind panics were deliberately caught by lifecycle assertions. No ordinary
  native test, fixture, child process or replay was selected.
- The pure test execution was on an uncommitted precursor. Afterwards unused
  timestamp storage was removed to settle the missing-elapsed partial outcome,
  and diagnostic WAIT_FAILED snapshots were changed to missing. Test bodies and
  assertions were unchanged. The final source was compile/lint checked, **not
  re-executed**; there is no claim that the final commit's tests were run.
- `cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings`:
  passed after removing entry timing (6.29 s), and after the final WAIT_FAILED
  correction (2.84 s). These checks execute no application/tests.
- Final `cargo fmt --all -- --check` and `git diff --check`: passed.
- `git add -- crates/arxd/src/development/windows.rs` and conventional source
  commit succeeded. Then the non-test artifact command below succeeded.

An initial in-memory Python editing script had a syntax error and made no write.
Two early rg searches used unsupported Windows filename globs and were corrected
to directory searches. No three-failed-same-fix sequence occurred. No test failed,
no mutation source edit was executed and no native test was retried.

### Pure test credit and limits

The four tests cover independent mapping and per-entry deletion/mislabel *vector*
negative controls; lifecycle, entry reset, one-shot consumption, reentrancy,
unwind and counter bookkeeping; mock reader before mock cleanup clobber; complete
maximal formatting, nonzero indices, Reason/validity and synthetic privacy strings.
The ordering test uses the same failure helper as native capture, with an injected
Cell reader and cleanup that changes 123 to 999. It makes no GetLastError or
SetLastError call. Swapping helper order would violate its expected primary 123.
No source-mutant run was performed: mutation credit is limited to these explicit
module assertions/vector controls, not measured killed source mutants.
Synthetic sentinel checks supplement the typed input restriction; they are not
private-state inspection and do not alone prove privacy.

**UNVERIFIED:** actual Win32 site reachability, marker deletion/mislabel mutations,
loop-index call-site mutations, native error ordering in execution, quarantine
behavior under injection, zero native CreateProcessW calls for future cases,
post-create never-resume behavior, deterministic all-stream replay coverage,
cleanup equivalence, historical causality and CI/native time budgets.

## Non-test compile/artifact inspection

Exact source commit/blob and baseline are pinned above. Target:
`x86_64-pc-windows-msvc`. Toolchain: pinned 1.95.0 above. Profile: default `dev`,
unoptimized + debuginfo. Package/library: `arxd --lib`. Features: defaults, no
feature flag supplied; arxd declares no package feature table. Lockfile Git blob:
`1c5c0fcf2b577f89ed998826ddf6608338b4b523` (unchanged).

Command after the source commit:
`cargo rustc -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc -- --emit=llvm-ir`
passed (30.12 s); the artifact was not executed.

Artifact: `target/x86_64-pc-windows-msvc/debug/deps/arxd-efd3c4593c7d28a6.ll`,
224924517 bytes. SHA256:
`38d1d3a64246eeca24751baae28d6e10fddacf44894b93bb88776d3c1a486c1c`.
Inspection streamed that artifact with a 256 MiB size fence. All 28 Stage and all
13 Context strings were inventoried, plus control strings, helpers and module
symbol patterns. The 25 distinctive stage strings were absent. Generic stage
substrings resume/wait/exit_code had 1282/2579/6 matching IR lines. Context
slot_reuse/unduplicable/echo were absent; generic context substrings tree,
overflow, handles, cancel, shutdown, fresh, fail, cwd, direct, leak had respectively
3940/1718/13/6510/1652/22/1115/23/2147/47 matching lines. Such generic substrings
also occur in ordinary dependency/code/debug names; substring presence is not
attributed to diagnostics and is not an absence proof by itself.

Zero matches for nested diagnostic scope, reentrant diagnostic invocation,
invalid diagnostic role, diagnostic role, the c/r/s formatter prefix,
diagnostic_snapshot, diagnostic_failure, windows4diag and windows6run_ok.
Source cfg/expansion inspection is the controlling exclusion evidence:
`diagnostic!` and `stage!` expand to empty blocks without evaluating arguments;
`drain_at!` emits only its original call once, discarding all diagnostic arguments.
The diag module/imports/enums/TLS/injection/formatters, run_ok, snapshot helpers,
Invocation local and peek injection branch are all cfg(test). No diagnostic
imports, TLS, control routing, strings or executable diagnostic logic are added
to the non-test build. No expanded-source tool was installed or invoked.
No baseline artifact was built and **no binary equality claim** is made; source
locations and panic metadata may change artifact bytes.

## Remaining project state and authority

M2 remains incomplete. The main incident's cause is unknown; no focused red
native feedback loop exists. **PA6 replay remains OPEN.** Native, installed and
dedicated paths are **unverified on real hardware**. This is instrumentation
preparation, not an incident fix or hardware acceptance.

No Rust full suite, ordinary native existing test, CLI, service, account, VM,
root/S4U/owner/private-state operation, process/system inspection, GitHub,
network/install, CI dispatch, historical retry or native injection was run.
Build tools and the single authorized pure-test harness are the only execution
in the compilation/test categories. Old Lab Manager and agent-windows-test were
not accessed. No babysitter operation was performed. Required independent source
review and green merge gates remain outstanding; this partial source must not be
represented as complete contract acceptance.
