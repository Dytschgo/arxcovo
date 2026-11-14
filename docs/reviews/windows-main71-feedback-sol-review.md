# Independent Sol review: main71 Windows feedback proposal

Disposition: **REQUEST CHANGES to the design; no implementation or run authority.**
OpenAI / gpt-6.1-sol / high, 2026-11-14. Single review sub-agent,
independent of the Anthropic / claude-opus-5-5 / high proposal author.
Standards, Spec and security are assessed separately below. This reviews a
proposal, not an implemented instrumentation patch or a fix.

## Scope and exact pins

| Item | Inspected identity |
| --- | --- |
| Review checkout HEAD, clean before review | `f1c8057a67caca48fd0f44aeb200c63c219b6c57` |
| Proposal author commit | `cc446251ce03bb1e4ab88cb911ef7e3ed61b012f` |
| Retained proposal commit | `1ddd1387bb2f9140972ab8486d8d8902cdb3012c` |
| Proposal path | `docs/reviews/windows-main71-feedback-proposal.md` |
| Proposal blob at both commits and review HEAD | `8fe32e9acfc37b3e62a349e5d6b691a7ccae236e` |
| Failure source commit | `a9ec0acdefcd2dc60eed34f095869a045cf8e356` |
| Prepared design base | `4172fda230d6b69616adcd05d47afaf751b8a704` |
| Source path | `crates/arxd/src/development/windows.rs` |
| Source blob, failure commit and review HEAD | `85f0c0eca19950fc65a02b621ff752eb6ba61551` (1085 lines) |
| Recorded hosted incident | run `43545244266`, attempt 1, failed, 232 seconds |

Verified the supplied cwd equals the Git top level. Git metadata identifies a
linked worktree with its own worktree Git directory and a separate common Git
directory; initial porcelain status was empty. Absolute local paths are omitted.
The current worktree record names branch `t3/review-windows-diagnostics-proposal`
at the review HEAD above; other worktrees were not operated on.
The proposal commits have identical trees in the comparison inspected. The
`crates/` comparison from failure source to review HEAD is empty. Source was
inspected through local Git objects, not by executing it.

Read AGENTS.md, KICKOFF.md, CONTRIBUTING.md, the Review and Debug skills and
their selected profiles, and relevant Windows contracts: ADRs 0006, 0009,
0010, 0012, 0013 and the provisioning decisions in 0016. No glossary exists at
the repository root. Explicit task scope overrides the broader kickoff workflow:
no agents, babysitter changes, PROGRESS edits, implementation or experiment.

Failure counts are retained task/coordinator evidence, not a fresh hosted-log
inspection: 84 passed, 1 failed, 3 skipped, 307 unrun under fail-fast. At source
lines 1068–1075, the panic at 1072 is an expected-success unwrap in the eight-run
overflow handle-count loop, before the final leak assertion at 1080–1083.
Iteration and failing native stage are unknown. Earlier ExecutionFailed records
remain separate incidents. No cause, reproduction, fix or pass is established.

## Standards

**REQUEST CHANGES — two blocking evidence requirements.**

1. **P4.4 promises mutation evidence its tests do not establish.** Moving the
   actual failure capture past cleanup need not affect an injection that writes
   the record itself and returns before the native failure branch. The synthetic
   error is already recorded. Likewise tests 1/2 do not assert the reported
   context index, so making it constant need not turn either red. The repository
   requires meaningful behavior tests and truthful evidence. Specify separate
   assertions for distinct nonzero indices and a controlled error-clobbering
   cleanup seam through the same failure-capture helper. Do not claim that this
   proves native last-error ordering: that ordering additionally needs source
   inspection at every real branch. No mutations were performed in this review.
2. **P4.5 is too weak for the stated non-test exclusion.** Absence of `arx-diag`
   alone does not prove absence of injection, stage labels, TLS, calls or altered
   runtime behavior. There is no implementation diff yet to certify. Require an
   exact implementation SHA, baseline/source blob, Windows target, toolchain,
   profile, features and lockfile pin for any later non-test artifact evidence.
   Inspect all reachable non-test expansions/imports and all label/control
   strings; an empty non-test macro must not evaluate arguments. Changed panic
   source locations can change binary bytes; the enforceable claim is no added
   diagnostic executable logic or diagnostic strings, not byte equality.

No source smell is asserted against this prose-only proposal. The Stage enum is
justified by the closed-vocabulary requirement; avoid a general injectable API
framework beyond this bounded seam.

## Spec

**REQUEST CHANGES — three blocking design gaps.**

1. **P4.1 does not ensure reachability of every label.** A `touch` fixture
   produces stdout but no guaranteed stderr bytes. `ReadFile` at 329 is reached
   only when the corresponding peek reports available bytes. Final reads need
   bytes still available after the loop and job stop; ordinary timing does not
   ensure that. The loop also short-circuits its stderr drain on stdout overflow.
   Specify deterministic state/stream inputs for loop/final out/err read sites
   and explicit injection-consumed assertions. A missing marker must fail through
   an independently specified expected mapping, not solely Stage::ALL and its
   own recorded labels. All 28 labels need coverage; reducing to process-state
   classes for budget loses the claimed mapping guarantee.
2. **P1/P3 leave record ownership ambiguous.** Reset-on-entry must clear stale
   output without disarming the requested one-shot injection. Context nesting
   must preserve daemon/child and pipe/phase ownership through shared helpers.
   Successful runs must clear the pending stages so P4.3 can observe an empty
   failure record. Specify separate armed-injection and captured-failure state,
   a same-thread scoped guard, non-reentrancy, and reset on unwind/test exit.
   Use a copyable fixed-size record; do not hold a RefCell borrow across native
   calls, failure formatting or nested stage operations. These are conditions,
   not assertions that an implementation already violates them.
3. **P2's context/reason contract is not closed or truthful as written.**
   `&'static str` permits runtime-leaked strings and arbitrary static text; it
   does not enforce source literals. Use a closed Context enum, including fixed
   action variants. `run_ok` receives any Reason, but the sample line always
   says `reason=ExecutionFailed`. Preserve the actual fixed Reason token and
   emit missing diagnostic fields for other errors, without stale stage data.
   Do not turn a new InvalidArguments/IdentityMismatch failure into purported
   ExecutionFailed evidence. Require a worst-case field-width proof and tests
   of the complete line at maximal integers/longest enum names within 384 bytes;
   a cap by truncation must not erase the stage, error or iteration.

The expected-success replacement inventory is correct: 808, 826, 879, 947,
956, 962, 966, 1009, 1054 and 1071–1072. Preserve the result assertions, all
eight loop iterations, `after <= before + 4`, timeouts, limits, cancel/shutdown
ordering and cleanup criteria. Leave refusal matches at 933, 941, 1026 and
1048 and fixture/setup unwraps alone. No change to Reason, MCP, activity,
quarantine, owner paths, path/reparse checks or policy-fixture authority.

## Security and execution authority

**HOLD — two blockers to treating replay as a pure or authorized run.**

1. **Test-only compilation is not native-execution authorization.** The real
   run_inner path opens tokens, files, pipes and Jobs before many injection
   points. Post-create points require a real suspended child; post-resume points
   execute it. Even a pre-create injection may already have made native calls.
   The silent uninjected control also launches a child. Divide the proposed
   validation into pure record/format/mapping tests, non-test compile/artifact
   inspection, and native fixture/replay tests with explicit side effects. None
   is authorized to run by this report. Native replay needs a separately bounded
   authorization identifying exact SHA, commands, environment and time/cleanup
   limits. Historical ordinary CI grants no extra, privileged or historical
   retry. A new SHA or changed test name must not launder that retry. Adding
   native tests to ordinary discovery also needs explicit scope disposition;
   calling them test-only does not resolve it.
2. **P3/P4 do not prove the child-safety and cleanup claims.** For injection at
   or before create_process, assert zero child-creation API calls; absence of a
   touch marker proves only no observed action, not no suspended child. For
   assign/token/resume injections, a child already exists: assert it never
   resumes and cleanup remains on the original error/quarantine path. No new
   bypass of token equality, Job assignment, path checks, bounds or quarantine
   is acceptable. The native assign failure terminates inside the closure at
   470; injection returns before that and depends on 555–558 cleanup. The
   proposal discloses this difference but overclaims "Cleanup is equivalent."
   Remove that claim pending evidence. Native post-failure observations at 555
   occur after the inner termination on assign failure; replay observations
   occur before outer termination. These snapshots are not equivalent, and
   neither is the hosted runner's missing snapshot. Keep that distinction in
   every artifact interpretation; never use it as a reproduction claim.

Numeric fields are acceptable in principle: handle *count*, child exit code,
job counts/violation flags, elapsed time and byte totals disclose no handle
values or object names. Use only existing borrowed handles and GetCurrentProcess's
pseudo-handle; no OpenProcess, DuplicateHandle, token reads for logging, new
events, waits beyond zero, or new kernel handles for diagnostics. Query failure
must produce missing data, never replace the primary error or control cleanup.
No raw structures/padding, output, environment, argv, SID, path or event names
may enter formatting. Privacy checks should use synthetic sentinels without
enumerating private owner environment/state. The proposed broad character regex
alone permits private alphanumeric content and is not a privacy proof.

## Decisions on questions 1–6

1. **Test-only seam: conditionally acceptable architecture, not yet verified.**
   Private cfg(test) data/helpers and non-test no-op macros can work. All macro
   arguments must be inert when excluded. No release hooks or strings. Apply
   the exact non-test build pins/evidence in Standards 2 later; no build here.
2. **Inventory: accept 18 sites / 28 labels at the proposal's granularity.**
   Eleven fallible ok-call sites: 64, 89, 231, 240, 286, 289, 329, 423, 430,
   448, 512, and seven explicit return sites (221, 324, 414,
   420, 471, 478, 508), hence 18 origins. The helper itself must not be counted
   again. The 28 labels expand reuse as listed below. Token info labels collapse
   four classes each; they do not distinguish User/Elevation/Integrity/Groups.
   That is acceptable as declared granularity, not full invocation coverage.
   Capture actual GetLastError first at each real failing branch, notably before
   TerminateProcess at assign; snapshot diagnostics must follow capture. In
   drain reuse the error saved for the broken-pipe decision, preserving 109 as
   success. At attr_size the size predicate can reject a call without a valid
   failure error; call it a captured last-error observation with validity
   semantics, not necessarily the cause. For wait distinguish WAIT_FAILED
   error validity from other unexpected wait returns and retain raw wait_ret.
3. **Privacy: conditional accept of numeric fields, closed enums required.**
   Apply Spec 3 and the security restrictions above; no dump/debug formatting.
4. **TLS: acceptable only within a synchronous, same-thread invocation.**
   Current direct test callers qualify structurally. Nextest process isolation
   is not a thread-ownership proof. The surrounding executor already uses
   spawn_blocking; an injection armed or record read outside that worker will
   miss it. Refuse to extend this TLS contract across threads or concurrent/
   nested invocations without an explicit context passed on the worker and a
   new review. Clear state on all exits; a fresh Execution per replay case is
   needed because post-spawn errors quarantine, even if one directory is shared.
5. **Drop P4.6's count-equality guard as written.** 18 source origins cannot
   equal 28 path labels. Prefer a reviewed independent site-to-label manifest
   with 18 site entries and 28 required labels, checked separately. Keep
   label-deletion/mislabel mutations. A raw substring count is not coverage.
6. **Budget: no measured acceptance yet.** Preserve the inspected 12-minute
   verify job timeout and the repository's under-15-minute CI wall-clock gate.
   Record pure/native test durations, replay case/child counts and aggregate
   delta on the same pinned target/config in a later authorized first run.
   Include cleanup's existing two-second bound per post-spawn case and every
   request timeout in the pre-run worst-case accounting. The failed 232-second
   main job stopped with 307 unrun tests and cannot establish available budget.
   If full label coverage cannot fit, redesign the pure/native split and
   resubmit; do not silently remove labels, extend timeouts, retry, skip tests
   or relax ci-time-budget/agent-review. No CI dispatch is requested here.

### Source inventory cross-check

| Shared/explicit origin | Required label expansion | Count |
| --- | --- | --- |
| facts OpenProcessToken :89 | daemon / child open | 2 |
| info GetTokenInformation :64 | daemon / child info | 2 |
| pipe CreatePipe :286 | out / err create | 2 |
| pipe SetHandleInformation :289 | out / err noinherit | 2 |
| Job :221, :231, :240 | create / limits / ui | 3 |
| :414, :420, :423, :430, :448 | nul / attr size / init / update / create process | 5 |
| :471, :478 | assign / resume | 2 |
| drain peek :324 | loop out / err; final out / err | 4 |
| drain ReadFile :329 | loop out / err; final out / err | 4 |
| :508, :512 | wait / exit code | 2 |
| Total | 18 origins, without double-counting ok's shared return | 28 |

## Smallest correction and disposition

Revise only the design first: separate pure validation from native replay;
specify deterministic reachability for all 28 labels, scoped TLS/injection
ownership, closed context and truthful reasons; narrow mutation/non-test claims;
remove cleanup-equivalence language; replace the invalid count guard; state
pinned budget evidence and a separate execution-authority boundary. Keep the
production behavior and every existing safety/acceptance threshold unchanged.
Then obtain a bounded design recheck before any implementation decision. This
report grants neither implementation, build, test, native call, launch, retry,
publishing nor merge authority.

Only read-only Git/PowerShell inspection and this review document's write/commit
were performed. No implementation, build, test, native API experiment, child
fixture, GitHub/network/install, owner/account/service/VM/private-state operation
or subagent was used. No source, proposal or PROGRESS edit. Debug Phase 1 remains
incomplete: no focused red-capable reproducer has been run. M2 remains incomplete;
native/installed/dedicated paths are **unverified on real hardware**.

Standards: 2 blockers (mutation evidence and non-test exclusion). Spec: 3
blockers (label reachability, state ownership, context/reason contract). Security:
2 blockers (execution authority and child/cleanup proof). No run authority.
