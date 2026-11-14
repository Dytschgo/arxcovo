# Windows main-after-PR71 failure: feedback-loop design proposal

Status: **PROPOSAL, NOT ACCEPTED.** Sol high review and two correction
reviews are retained; the latest narrow Spec/security corrections need recheck
before acceptance. The original requirement was independent Sol high review
before anyone implements it. Implementation and runs are not authorized here.
Debug phase 1 is not done: no red-capable loop exists for this failure, no
failing error stage is known, and this document gives no hypothesis or cause
ranking.

Author: Anthropic / claude-opus-5-5 / high, design sub-agent. Read-only
diagnosis. No source, PROGRESS, workflow, protection or test changes. No native
process launch, test run, rerun, retry, S4U, owner, account, service, VM,
private-state, GitHub, network or install operation was performed.

## Pins

| Item | Value |
| --- | --- |
| Failing main commit (PR71 squash) | `a9ec0acdefcd2dc60eed34f095869a045cf8e356` |
| Design worktree HEAD (clean, isolated worktree) | `4172fda230d6b69616adcd05d47afaf751b8a704` |
| `crates/` diff a9ec0ac..4172fda | none (only docs changed) |
| `crates/arxd/src/development/windows.rs` blob at both | `85f0c0eca19950fc65a02b621ff752eb6ba61551`, 1085 lines |
| Hosted run | `43545244266`, attempt 1, 232 s, 2026-11-14 02:15:16–02:19:08Z |
| nextest version / config | `cargo-nextest@0.9.151`; no `.config/nextest.toml`, so defaults apply (fail-fast on, no retries, failing test output shown) |
| CI command | `cargo nextest run --workspace --locked` (`.github/workflows/ci.yml:53` at a9ec0ac) |

Source was read with `git show a9ec0ac:<path>` only. I did not read the hosted
log. The failure facts below come from the coordinator's record in
`docs/PROGRESS.md` at `4172fda` (lines 5 and 9) and from the task brief.

## Failure facts (recorded, not inferred)

- Windows job: nextest reported 84 passed, 1 failed, 3 skipped, 307 not run
  because of fail-fast. Ubuntu and macOS passed in the same run.
- The failing test was
  `development::windows::tests::junction_cwd_and_tool_refused_and_handles_do_not_leak`.
- The panic was `.unwrap()` on `Err(ExecutionFailed)` at `windows.rs:1072`.
  That line is the `.unwrap()` inside `for _ in 0..8 { let (_dir, e, r) =
  fixture("overflow"); assert!(run_inner(..).unwrap().cleanup_confirmed); }`
  (lines 1068–1075).
- No rerun was authorized or performed. No cause is claimed.

Read straight from the source order (structure, not cause): line 1072 runs only
after every earlier statement in that test has completed. That includes the
ordinary-directory control run, the four junction cwd refusals, the
`pin`/`open_pinned` checks, the tool-junction refusal, the direct-tool run, the
two no-quarantine assertions, the junction removals and the first
`GetProcessHandleCount`. The failure was inside the handle-count loop, before
its `after` count and before the `after <= before + 4` leak assertion. Which of
the eight iterations failed is **not** recorded.

## What is missing

The hosted record stops at the Rust `Reason`. `ExecutionFailed` comes from
**18 failing source lines**, reached through **28 call-path labels**, in the call graph of `run_inner` at this blob, and
nothing tells them apart:

| Stage (proposed label) | Origin at a9ec0ac | Process state at that point |
| --- | --- | --- |
| `daemon_token_open` | `facts` → `ok(OpenProcessToken)` :89 via :366 | before spawn |
| `daemon_token_info` | `info` → `ok(GetTokenInformation)` :64 (user, elevation, integrity, groups) | before spawn |
| `pipe_out_create` / `pipe_err_create` | `ok(CreatePipe)` :286 via :395 / :396 | before spawn |
| `pipe_out_noinherit` / `pipe_err_noinherit` | `ok(SetHandleInformation)` :289 | before spawn |
| `job_create` | `CreateJobObjectW` null :221 | before spawn |
| `job_limits` / `job_ui` | `ok(SetInformationJobObject)` :231 / :240 | before spawn |
| `nul_open` | `CreateFileW("NUL")` invalid :414 | before spawn |
| `attr_size` | size check :420 | before spawn |
| `attr_init` / `attr_update` | `ok(..)` :423 / :430 | before spawn |
| `create_process` | `ok(CreateProcessW)` :448 | before spawn |
| `assign_job` | `AssignProcessToJobObject` :471 | suspended child, not in job |
| `child_token_open` / `child_token_info` | `facts(process)` :473 → :89 / :64 | suspended child in job |
| `resume` | `ResumeThread` :478 | suspended child in job |
| `loop_{out,err}_peek` / `loop_{out,err}_read` | `drain` :324 / :329 via :485–486 | running child |
| `wait` | `WaitForSingleObject` not `WAIT_TIMEOUT` :508 | running child |
| `exit_code` | `ok(GetExitCodeProcess)` :512 | after exit or stop decision |
| `final_{out,err}_peek` / `final_{out,err}_read` | `drain` :324 / :329 via :519–520 (≤32 passes) | after `job.stop()` confirmed |

`CleanupUnconfirmed`, `IdentityMismatch`, `PreflightFailed`,
`InvalidArguments` and `Unavailable` are separate variants. The recorded
`ExecutionFailed` rules those out as the returned value. It does not rule out
earlier work in the same call. This table lists observation points, not
candidate causes.

These observations are missing:

1. **The stage**, meaning which of the origins above returned.
2. **The Win32 error number**, captured at the failing call before any `Drop`
   or `CloseHandle` can overwrite it. For `wait`, also the raw return value.
3. **The iteration index** (0–7) within the loop.
4. **The child and job state after the failure**: child wait state and exit
   code, job active/total/terminated process counts, job limit-violation flags.
   This only applies after `create_process`.
5. **Elapsed milliseconds** from `run_inner` entry, and drain totals and pass
   index for drain stages.
6. **The process handle count at failure.** It is directly relevant to what
   this test measures. It is recorded as a number only, with no inference
   attached.
7. **Concurrency context**: which other tests nextest was running at that
   moment, and runner load. The proposed artifact cannot capture these.
   Whatever the default nextest log keeps is all there is. I have not read it.

## Relation to earlier `ExecutionFailed` records (kept separate)

`docs/reviews/development-execution-failure-observations.md` and
`0016-e2-current-parent-assessment.md` record earlier `ExecutionFailed` reds in
other tests and runs, on other source. This incident is recorded on its own.
Sharing a `Reason` variant does not make it the same failure, and I do not use
those records as evidence about this one.

The structural point that carries over is this. The retained
`development-execution-failure-instrumentation.patch` (SHA256
`60e9ab39…d71e`, based on the same blob `85f0c0e`) is a *temporary local*
stderr patch with `[DEBUG-ef7c]` tags. It was never committed to crates, so
ordinary hosted CI could not have emitted a stage line on this failure. The
proposal below differs in three ways: it is committed, it is compiled only
under `cfg(test)`, and it travels in the test panic message, so the next
natural failure in already-running ordinary CI carries its own stage. It also
proposes a separately bounded replay seam and pure mapping controls.
Native replay is not authorized by this proposal.

## Proposal: test-only stage/error artifact and replay seam

### P1. Test-only state and capture

Use a private cfg(test) module with closed Stage and Context enums, and a
copyable fixed-size record. A same-thread scoped guard owns separate armed
one-shot injection and captured-failure state. Entry clears stale capture
without disarming the requested injection. Reject nested/reentrant invocation;
restore/clear state on unwind and test exit. Shared facts/pipe/drain helpers
preserve explicit daemon/child, out/err and loop/final context. Success clears
pending stages and leaves captured failure empty. Never hold a RefCell borrow
across native calls, formatting or nested stage operations. Nextest process
isolation is not a thread-ownership proof: arming, calling and reading happen
on the same synchronous worker. Cross-thread use requires a new design.

Use cfg(test) macros with non-test expansions that neither execute code nor
evaluate arguments. Stage/control strings and imports stay test-only. Capture
actual GetLastError first at every real failure branch, before cleanup or
snapshot calls; inspect all 18 origins individually. Drain reuses the saved
error used for its broken-pipe decision, preserving 109 as success. Attr-size
may lack a valid failure error: record last-error observation with validity,
not a purported cause. Wait records raw wait_ret and error-valid only for
WAIT_FAILED. No production Reason, path/token/Job fence or quarantine change.

Numeric snapshots use existing borrowed handles and GetCurrentProcess's
pseudo-handle only. No OpenProcess, DuplicateHandle, new events/handles, token
reads for logging or waits beyond zero. Query errors produce missing fields,
never replace the primary error or alter cleanup. Observe before the existing
outer cleanup; native assign failure already terminated inside its closure,
so its snapshot differs from replay's pre-outer-termination snapshot.

### P2. Truthful bounded output

run_ok accepts only closed Context variants (including fixed action variants)
and the actual iteration u32. It preserves the actual fixed Reason token.
For other Reasons, missing diagnostic fields replace stale stage data; never
label InvalidArguments or IdentityMismatch as ExecutionFailed. Use a single
fixed field layout, closed enums and numeric/missing values only, at most
384 bytes. Prove worst-case widths with maximal integers and longest enum
names and test the complete line; truncation may not erase error/stage/index.
Synthetic privacy sentinels cover paths, argv, SID, environment, raw output,
handle values and names without enumerating owner data. A character regex
alone is not a privacy proof.

Replace only the expected-success run_inner unwraps identified in the Sol
review (808, 826, 879, 947, 956, 962, 966, 1009, 1054, 1071-1072). Keep refusal
matches, setup unwraps, eight iterations, leak threshold after<=before+4,
expected results, limits, timeouts and cancel/shutdown/cleanup order intact.
No workflow, nextest, retries, fail-fast or protection change.

### P3. Validation and execution authority are separate

A. Pure tests cover only the diagnostic module: record lifecycle, closed
label mapping, formatter, error validity, index preservation and a failure-
capture helper with an injected pure error-reader closure. A mock cleanup
clobbers that mock error value; there is no GetLastError/SetLastError native
call in this test. This proves helper ordering only. Each real failure site
still requires source inspection, including actual loop-index forwarding.

The current direct Win32 call graph has no mock API layer. Do not add one or
claim pure tests reach its real failure sites. The independent 18-origin /
28-label manifest supports static inventory and module mapping tests only.
Real-site marker reachability, deletion/mislabel mutation coverage and
quarantine effects stay UNVERIFIED until separately authorized native replay.
Pure mapping tests are not a deterministic native failure/reachability loop.

B. Non-test compile/artifact inspection is a separate validation category.
Record exact implementation SHA, baseline/source blob, Windows target,
toolchain, profile, features and lockfile. Inspect all reachable macro
expansions/imports, injection controls, TLS and label/control strings. Assert
no added diagnostic executable logic or strings, not binary equality or just
absence of arx-diag; changed panic source locations can alter binary bytes.
No build has been performed or authorized by this proposal.

C. Native fixture replay is NOT pure and is not authorized here. Real
run_inner opens tokens/files/pipes/Jobs before many injection points;
post-create cases require a real suspended child and post-resume cases run
it. Even an uninjected silent control launches a child. Any native replay or
adding such tests to discovery needs a separately bounded scope disposition
naming SHA, commands, environment, case/child count, time and cleanup limits.
A new SHA/test name or past ordinary CI does not authorize a historical,
privileged or extra retry. Do not add or execute native replay in the initial
instrumentation implementation merely by calling it cfg(test).

### P4. Deterministic coverage and later native acceptance contract

All 28 mappings remain required. Specify deterministic stream/state inputs
for loop/final stdout/stderr read/peek sites: touch output alone cannot
reach stderr or guarantee bytes survive until final drain. Each case asserts
injection consumed exactly once against the independent mapping; a missing
marker fails, and stdout overflow cannot silently skip stderr coverage.
Use a fresh Execution for every native case because post-spawn failure
quarantines it. Injection code 109 at peek cannot force a failure which the
product treats as success.

Every separately authorized native replay case at or before create_process
must assert zero CreateProcessW calls using a cfg(test)-only counter located
at the actual API call site. Marker absence is insufficient. Pure module tests
can validate counter bookkeeping, but do not earn native zero-call evidence.
For any separately authorized assign/token/resume native case, assert the
already-created child never resumes and follows original error/quarantine
cleanup. A missing touch marker proves no observed action, not absence of a
suspended child. No token-equality/Job/path/bounds/quarantine bypass.
Capture ordering mutations, context-index mutations and mapping mutations
need their own intended assertion, not merely an injected record that bypasses
the real failure-capture branch. Replay cannot prove native last-error order,
cleanup equivalence, timing or the historical cause.

Keep the 12-minute verify timeout and under-15-minute CI wall-clock gate.
Before proposing native execution, bound every case, request timeout and
existing two-second post-spawn cleanup, then measure authorized pure/native
costs at the same pinned configuration. The failed 232s job with 307 unrun
tests gives no available-budget evidence. If all-label coverage cannot fit,
redesign the pure/native split and re-review; do not remove labels, increase
timeouts, skip tests or bypass gates.

## What this does and does not buy

- Pure tests make the diagnostic module's formatting, record lifecycle and
  closed mapping red-capable. They do not reach real Win32 sites or prove
  marker reachability, native label/deletion mutations or failure causes.
  Real-site replay evidence is deferred to separately authorized native
  validation with deterministic inputs for all 28 labels. Debug phase 1
  remains open: no focused native failure reproducer exists.
- The next *natural* `ExecutionFailed` in ordinary CI on any PR or main, with no
  rerun, gets turned into one line: stage, Win32 error, iteration, numeric
  state. A focused loop at that one stage can only be designed after that, and
  it needs its own authorization.

**Limits of synthetic replay compared with the actual native failure:**

- Injection skips the real API, so it says nothing about *why* that API would
  fail. The codes it uses are chosen, not observed.
- Post-failure facts in replay describe synthetic state, not the state the
  hosted runner had.
- At `assign_job`, the native path calls `TerminateProcess` inside the closure.
  The injected path skips that and relies on the outer `is_err` cleanup.
  Cleanup equivalence is unproved. The observations occur at different
  points and cannot substitute for one another or the missing hosted trace.
- Replay cannot reproduce scheduler timing, kernel resource pressure, other
  security software, runner concurrency, or what nextest ran alongside the
  failing test.
- Adding code to the test binary changes code layout and timing, so the
  natural recurrence rate may change. A period without recurrence after the
  artifact lands is **not** evidence of a fix.
- Coverage is limited to `windows.rs` tests. A failure that surfaces through
  other callers, such as executor, MCP or SDK positive controls, would still
  show only the `Reason` unless those callers adopt `run_ok` in a separate
  change.

## Review needs (Sol high, independent)

1. Does every non-test hunk count as test-only? Labels sit inside production
   functions as `cfg(test)` macros that expand to nothing. Accept, or ask for a
   different seam.
2. Is the stage table complete for this blob, and is `GetLastError` captured
   before any intervening Win32 call at every site?
3. Is the field set privacy-safe? In particular, are `handles`, `child_code`
   and `job_violation` acceptable as raw numbers?
4. Is the thread-local design sound under nextest's process-per-test model,
   and should it be refused if any `run_inner` caller moves to another thread?
5. Use the independent 18-origin/28-label manifest; drop count equality.
6. CI budget acceptance criteria for the replay test.

## Explicit non-actions

No rerun, retry, `--retries`, flaky tolerance, test disable or skip, threshold
or timeout change, fail-fast change, gate bypass or protection change is
proposed or implied. Ordinary CI that already ran historically does not grant
privileged or extra retries. M2 is incomplete. Native, installed and dedicated
paths are **unverified on real hardware**. Providers: author Anthropic /
claude-opus-5-5 / high. Outcome: proposal corrected by Sol medium after review `43bb26d`
(retained `d2bd9d3`); unaccepted pending narrow different-provider recheck.

## Latest narrow contract correction

Sol medium addresses review `0da16cd` (retained `5f357fd`): pure validation is
limited to the diagnostic module; real-site reachability remains unverified.
Every pre-create native replay case needs the actual cfg(test) API counter.
Injection remains test-code-only, occurs before its selected API, and never
calls that selected API. It is unreachable from CLI/config/MCP. Success-path
instrumentation adds no native calls or handles. The two token-info labels
each merge four token classes; they do not identify the exact class.
The output field vocabulary described in the review is a design inventory,
not an already implemented field layout. Review questions 3/4/6 are resolved
as conditional requirements above, not fresh requests or run authorization.
