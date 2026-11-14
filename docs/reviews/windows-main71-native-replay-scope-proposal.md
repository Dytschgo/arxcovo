# NEXT M2: bounded Windows native capture and replay scope

Status: **PROPOSED; independent Grok/Muse review and coordinator disposition
required. No implementation, build, native execution, discovery addition,
publication or dispatch authority.**

Author: OpenAI / gpt-6.1-sol / high, design sub-agent, 2026-11-14.
The requested automatic Sol implementation routing is a future routing choice;
it does not turn this design assignment into implementation authority.

## Basis and inspected pins

Initial cwd equalled Git top level, initial porcelain status was empty, and
separate worktree/common Git directories established a linked isolated checkout.
Paths in this record are repository-relative or sanitized placeholders.

| Item | Exact pin |
| --- | --- |
| Prepared HEAD, including progress | `c9e1e772da7a4d248ebdc856d52e8081bbc471b9` |
| Merged PR74 | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74` |
| Actual `crates/arxd/src/development/windows.rs` blob | `4cb039f4742568a3eb3a124ae3626ac9511b8be2` |
| Corrected source LF SHA256, as recorded by Grok | `e2c91088e83ac15984219ea4581f5effe77c0e2089a9222fa1bb4eb764e1433a` |
| Accepted full feedback proposal blob | `eaf4e7afa87eebb12a2e6fe3a9c14923adb95e0e` |
| Grok capture-source review blob | `3a8ec17558bd8070cb83d55652adf34def82c26d` |
| Grok SP1/SP2 correction review blob | `52ae5461b93a7ceb699ad8285c862acf2eb87783` |
| Progress blob | `b560554b4ebe0798fde89c4806a9dc9a61d98ac7` |
| Workspace lockfile blob | `1c5c0fcf2b577f89ed998826ddf6608338b4b523` |
| Toolchain file blob | `3417ee590341edc47a046d94e77ee9bee2b33bf4` |
| CI workflow blob | `fc065af9786e75c52cfb7668b726d0a89931cd5b` |
| Historical failure source | `a9ec0acdefcd2dc60eed34f095869a045cf8e356`, Windows blob `85f0c0eca19950fc65a02b621ff752eb6ba61551` |

Read AGENTS, full KICKOFF, full accepted Windows proposal including the missing-ms
disposition, Sol design review, Opus correction review, Grok pure-boundary recheck,
Grok capture-source and SP1/SP2 correction reviews, progress, relevant source,
CI, CONTRIBUTING and ADRs 0010/0013. The debug skill was read; no selected
active profile or root glossary was present. The task-specific restriction
overrides kickoff's general delegation, babysitter and progress-writing workflow.
No babysitter operation is proposed here.

SP1 and SP2 are closed as dormant source seams, not as native evidence.
`diag::observe` owns `Scope`, calls a synchronous closure, and copies record and
actual create counter before Scope drops. Arm and call `run_inner` directly
inside that closure; never nest `run_ok`, whose private scope would assert and
whose return would erase the counter. `capture_injected` closes metadata:
AttrSize has invalid last-error observation, Wait models WAIT_FAILED, peek 109
is successful broken-pipe handling with no failure record. Non-test macro
arguments remain unevaluated; dormant diagnostic success paths add no native
calls or handles.

Debug Phase 1 still has **no natural red-capable native failure loop** for
main71. Phases 2–6 are not entered. Chosen injection errors exercise diagnostic
mechanisms; they are neither observed causes nor a cause fix. Historical run
43545244266 attempt 1 is unresolved and must not be retried. The recorded
232-second failed job with 307 unrun tests gives no budget headroom. PR74's
recorded 468-second PR CI is historical, not a fresh check in this assignment.

## Proposed evidence boundary

Three evidence categories must remain separate in the future report:

1. Existing six pure tests: TLS ownership, mapping, formatting, mock-reader
   ordering and synthetic metadata only. No native reachability credit.
2. Native kernel capture control: an actual invalid-handle PeekNamedPipe through
   `drain` must take its real error branch. This can test that branch and its
   captured error; it does not test all real failure origins or main71.
3. Native injected replay: all 28 labels through actual `run_inner` call sites,
   real token/path/pipe/Job setup and, where required, a child. The selected API
   is skipped. This proves selected marker routing, one-shot consumption and
   the asserted synthetic cleanup state only. It does not prove the selected
   kernel API would fail, native last-error ordering at that skipped API,
   cleanup equivalence to actual failure, or historical reproduction.

No general Win32 mock layer is proposed. Real-branch capture ordering still
requires inspection of every origin. Native assign failure captures before its
inner TerminateProcess and snapshots afterwards; assign injection snapshots
before outer termination. Keep those observations distinct in every report.

## Independent 18-origin / 28-label contract

Origin numbers below belong to historical blob `85f0c0e`, not current line
numbers. Eleven fallible ok-call origins plus seven explicit branches = 18;
the common `ok` helper is not an additional origin. Token-info labels each
merge User, Elevation, Integrity and Groups; no per-class coverage is claimed.

Input S is a proposed silent child which exits 0, writes neither stream and
creates no descendants. Input L/F is the separately reviewed pipe fixture
described below. All requests use timeout_s=1, output_bytes=8192, pids=16,
memory_bytes=512 MiB, fresh cancellation and fresh Execution. Error 55 is a
chosen synthetic error, never a reported kernel result.

| Origin / API | Required labels (one error-55 case each) | Deterministic route / input | Actual create calls per case |
| --- | --- | --- | --- |
| 89 OpenProcessToken | daemon_token_open | first daemon facts, before policy/spawn | 0 |
| 64 GetTokenInformation | daemon_token_info | daemon facts, first info call | 0 |
| 286 CreatePipe | pipe_out_create, pipe_err_create | valid owned cwd/tool; out then err pipe | 0 |
| 289 SetHandleInformation | pipe_out_noinherit, pipe_err_noinherit | respective pipe created, noinherit marker | 0 |
| 221 CreateJobObjectW | job_create | both pipes ready | 0 |
| 231 SetInformationJobObject | job_limits | unnamed Job created | 0 |
| 240 SetInformationJobObject | job_ui | memory/process/kill limits installed | 0 |
| 414 CreateFileW | nul_open | Job ready, before NUL open | 0 |
| 420 attribute-size predicate | attr_size | before size probe; e=55, v=0, w=- | 0 |
| 423 InitializeProcThreadAttributeList | attr_init | bounded valid size obtained | 0 |
| 430 UpdateProcThreadAttribute | attr_update | list initialized | 0 |
| 448 CreateProcessW | create_process | handle list ready, injection before actual counter/API | 0 |
| 471 AssignProcessToJobObject | assign_job | S created suspended, not assigned; outer termination required | 1 |
| 89 OpenProcessToken | child_token_open | S suspended and assigned, before token open | 1 |
| 64 GetTokenInformation | child_token_info | S suspended and assigned, first child info | 1 |
| 478 ResumeThread | resume | S suspended, assigned, identity equality/policy passed | 1 |
| 324 PeekNamedPipe | loop_out_peek, loop_err_peek | S resumed; L prefill at first loop pair; out below limit so err reachable | 1 |
| 329 ReadFile | loop_out_read, loop_err_read | L prefill; actual corresponding peek must report >=1 byte | 1 |
| 508 WaitForSingleObject | wait | S resumed, empty successful loop drains; no cancel/shutdown/overflow; injection before first wait, w=4294967295 v=1 | 1 |
| 512 GetExitCodeProcess | exit_code | S resumed; normal wait-exit or bounded request timeout reaches marker | 1 |
| 324 PeekNamedPipe | final_out_peek, final_err_peek | successful loop, exit-code and confirmed Job stop; F prefill at pass 0 | 1 |
| 329 ReadFile | final_out_read, final_err_read | F prefill after confirmed stop; actual corresponding peek >=1 byte | 1 |

There are 14 zero-create cases, four suspended-child cases and ten resumed-child
cases. That totals **28 cases / 14 children**, not 28 children. Every earlier
unexpected failure blocks the row; it cannot count as coverage of the selected
label. No skipped label, fallback input or timing retry earns a pass.

## Missing deterministic stream seam: separate fixture implementation review

Existing `touch` writes no stderr. `echo` writes both but scheduling cannot
guarantee final bytes; overflow can short-circuit the loop stderr drain. Existing
`drain` reaches ReadFile only if PeekNamedPipe reports bytes. No safe deterministic
existing seam guarantees all eight stream labels at the `run_inner` callers.
This is an implementation prerequisite, not permission to improvise one in a run.

Propose a narrow cfg(test) stream fixture, reviewed separately before use:

- Build a separate test helper at the proposed manifest
  `tools/windows-native-replay/child/Cargo.toml`. It has its own pinned lockfile,
  no external dependencies, no libtest harness output, no child spawning, no
  reads of identity/private data and only silent exit 0. Copy its reviewed
  executable as cargo.exe in one owned fixture directory whose only executable
  is that hashed helper. PATH is exactly that one directory, arguments are
  empty, and the resolved bytes must match the reviewed hash before every row.
  Keep this helper outside workspace members and preserve all configured tool
  and path checks. Real Cargo must never be selectable. Do not use cmd/PowerShell as
  the fixture child. Exact helper bytes and lockfile pins are mandatory later.
- Only while the explicit fixture guard is active, retain one non-inheritable
  duplicate of each existing stdout/stderr writer in the parent, AFTER the
  inheritance list is sealed and BEFORE the original writer drops at baseline
  1371-1372. Taking duplicates after those drops is forbidden. The child's
  inheritance list remains exactly NUL/stdout/stderr; retained duplicates are
  never inherited. Originals still drop at the original point. This is **new
  fixture-only handle/native activity**, not diagnostic instrumentation and
  not allowed by the accepted dormant capture contract alone. Separate review
  must accept it or reject this route. Non-fixture tests and non-test builds
  must acquire no new handles/calls or evaluate fixture arguments.
- Prefill is enabled ONLY for eight stream failure rows, four peek-109 rows
  and the two controls. All other rows have empty streams and no prefill.
- L cases: immediately before the first loop drain pair, synchronously write
  exactly one fixed synthetic byte X into each original stream through those
  retained writers. Write completion and byte count are asserted before either
  peek. No other bytes are produced by S. The first out drain can consume at
  most one byte, far below 8192, so it cannot short-circuit err. Both peek and
  read cases use the same prefill, with injection at their selected site.
- F cases: do not seed during the loop. Only after the existing `job.stop()`
  has returned true, immediately before final pass 0, write one fixed byte X to
  each stream. These bytes cannot have been consumed during the loop. Assert
  write counts, real peek availability and pass=0. Keep the original 32 passes
  and bitwise final drain operation. Do not bypass stop, skip drains or change
  phase/order to make the test pass.
- Keep retained writers until all drains and error cleanup return, then close
  them before removing the temp fixture. S never writes, so pipes are empty
  at prefill; each synchronous write is one byte, not a pipe-capacity stress.
  A watchdog still bounds hung native calls; scheduler timing is never the
  mechanism for stream reachability. No sleeps/poll-until-lucky output checks.
- Two uninjected controls exercise L and F separately and assert stdout/stderr
  contain exactly their respective one-byte payload, totals 1/1, no drops,
  successful cleanup and no diagnostic record. Retaining writers changes EOF
  behavior in these fixture cases; this limitation must be reported. It is not
  proof of ordinary child-output EOF behavior or cleanup equivalence.

The fixture may add only fixed TLS selectors and call-boundary hooks; no new
CLI/config/MCP or release control. It must not fabricate PeekNamedPipe results,
ReadFile results, token facts, Job assignment, child identity or cleanup success.
Use normal policy checks (`run_inner(..., false)`) in the approved standard
environment; do not expand or depend on `policy_fixture`'s existing exemption.
Construct the Execution without extra tool discovery/child launches. If the
existing constructors cannot do this safely, propose a test-only constructor
under implementation review with no policy/path/quarantine exemption.

## Oracle, counters and assertions

Use a literal table of the 28 expected external labels, origins, stream phase,
role, metadata and child counts above. It must not be generated by Stage::ALL,
resolve, the captured record or the fixture selector. Review explicit table-to-
Stage binding. Count/set self-consistency supplements actual row assertions.

Inside a single same-thread `diag::observe` closure: arm the independently
selected Stage with 55, invoke `run_inner` directly once, then copy the returned
Reason and counters before Scope drops. Observation.create_calls is the real
counter at the sole CreateProcessW site, after injection and immediately before
that API. Assert 0 for all 14 at/before-create rows and exactly 1 for every other
run_inner row. No helper may call create_call to manufacture expected counts.

The current Observation alone does **not** prove consumed exactly once: invocation
drop clears armed state even if it was never consumed. Separately reviewed
cfg(test) instrumentation must count only successful `armed.take()` consumption
and attempted native ResumeThread calls at the actual site after injection.
Consumption and resume counters must survive Invocation::drop just as
create_calls does; Invocation must not clear them with armed state. Copy them
before Scope drop, then Scope clears all state, including on unwind. Peek109
counts as consumed although no failure record is stored. Resume increment is
AFTER its injection return and immediately BEFORE actual ResumeThread. Preserve
non-reentrancy. Do not add native counter operations. Require consumed=1 for
every injected row, 0 for controls; a second consume in the same invocation
cannot occur. Pending/armed empty after drop is not consumption evidence.

For error-55 rows require Err(ExecutionFailed), exact independent stage, error=55,
valid=true except AttrSize, wait missing except Wait, ms missing, and correct
phase/pass/totals where applicable. Snapshot query failure remains missing;
do not assert unstable handle totals or child exit timing as constants. Preserve
the primary error before snapshots/cleanup. Assert all state is clear after
scope drop and each fresh row begins empty. Distinct fixed case indices and
Context values must survive formatting; an index unit test does not prove
historical loop-index forwarding, which remains source inspection.

Add four peek-109 cases, one per loop/final out/err role, with identical L/F
inputs. Require consumed=1, no failure record, successful result/cleanup,
one actual create and one actual resume. The skipped peek may defer a byte to
a later pass; it must never be called ExecutionFailed. ReadFile error 109 is
not peek success; pure metadata already distinguishes it, with no native credit.

Kernel capture control (one case, zero children): enter observe/Invocation,
set a literal LoopOut role, invoke real `drain` with a null invalid handle and
an empty Captured. Assert its actual peek failure yields ExecutionFailed and
captured ERROR_INVALID_HANDLE=6, valid=true, no primary wait, no injection
consumption and create/resume counters 0. Never close a fabricated handle.
Snapshot query failures cannot replace error 6. A test-only post-capture mock
clobber may test ordering separately; it cannot claim ordering for other APIs.

Intended future negative controls: deleting/mislabeling any real marker must
fail its row's consumed/stage assertion, suppressing L/F prefill must fail its
read row, resetting create counter before observation must fail a one-child
control, and allowing ResumeThread in a suspended row must fail its actual
resume counter. These are test-design obligations, **not executed mutation
evidence**. Native mutation runs would add cases/children and are outside the
35-case envelope below; submit an explicit additional reviewed budget rather
than quietly replaying the matrix per mutant. Pure mutants retain pure credit.

## Child lifecycle and cleanup contract

At/before-create failures may open tokens, pipes, files and a Job, but actual
create counter must be 0. For assign/child-token/resume rows it is 1 and actual
resume counter must be 0. Create remains CREATE_SUSPENDED; missing output or a
missing touch file never proves never-resume. No child can run before successful
Job assignment and identity/policy checks. Assign injection is the special
not-yet-in-Job suspended case: original outer TerminateProcess must run.

For the ten resumed rows, four 109 rows and two controls, exactly one resume is
allowed after the existing assignment/token checks, and S makes no descendants.
Never use tree/handles/overflow fixtures to inflate process counts. Existing
kill-on-close, process/memory/UI limits, terminate/Job.stop/quarantine order and
all refusal paths remain unchanged. Each post-create error quarantines its
Execution; construct a fresh Execution for EVERY row, never clear/reuse one.
Direct run_inner does not read quarantine on entry, so a second direct call is
not a quarantine refusal test. Any such assertion must separately use work;
no additional work call is included in this matrix or its child budget.

The existing error cleanup ignores its outer Job.stop result; return of
ExecutionFailed does not itself establish confirmed cleanup. Proposed fixture
observers must retain only already-owned process/Job handles long enough to
observe termination and Job ActiveProcesses=0 after the original cleanup, using
bounded waits/queries. Do not OpenProcess by PID/name, duplicate diagnostic
handles, or replace original cleanup. Implementation review must settle safe
borrowing/lifetimes and recording the existing stop result without changing
product decisions. For assign, separately confirm the unassigned process exited;
an empty Job alone is insufficient. Never count pre-cleanup snapshot as proof.

Unknown cleanup, failed queries, timeout, failed termination, unexpected extra
child/create/resume or containment failure stops the remaining matrix, marks
evidence incomplete and prohibits environment reuse. Preserve a sanitized
failure record, keep technical containment, and retire the disposable environment
through its authorized controller. No kill-by-name, PID guessing, broad process
enumeration or broad filesystem glob deletion. Only exact fixture-owned handles
and validated exact temp paths may be cleaned. If disposal needs irreversible
administration, that action remains owner-only; do not perform it.

## Environment, commands and provenance for a later authorization

Choose one fresh local disposable Windows environment under a dedicated standard,
unelevated test account, or a hosted disposable Windows worker with an approved
standard token. Neither is selected or provisioned by this document. Windows
has no root UID: "nonroot" here means not SYSTEM/service/elevated, medium
integrity, no enabled administrator group. Internally verify only the fixed
policy verdict; never record account names/SIDs, profile paths or token dumps.
If a hosted token fails this policy, stop; no elevated policy-fixture workaround.

Use owned local temp storage with checked reparse-free ancestors and leaf pins,
separate from owner state. Set TEMP/TMP and Cargo target to exact dedicated
fixture-owned locations in that environment; capture only sanitized logical
paths. Keep source immutable while running. Do not stat or enumerate owner
profiles, accounts, services, VMs, installed applications, system/process state
or private state as environment discovery. Targeted internal queries of the
current approved token and fixture-owned handles require the later run authority.
Owner private account, old Lab Manager installation/state and Hyper-V VM
`agent-windows-test` are forbidden. No install, account switch, S4U, rights,
SCM, VM change, privileged retry or E_PROTECTION experiment.

The following are **exact proposed commands, not run commands or existing-test
claims**. The helper manifest and two native tests do not exist at this pin.
Commands execute sequentially, once each only in their separately authorized
stage. No broad cargo test/nextest invocation is a local replay command.

```powershell
$env:CARGO_PROFILE_DEV_DEBUG = '0'
git rev-parse HEAD
git rev-parse HEAD:crates/arxd/src/development/windows.rs
git rev-parse HEAD:Cargo.lock
git status --porcelain=v1
rustc -Vv
cargo -V
cargo fmt --all -- --check
cargo check -p arxd --tests --locked --offline --target x86_64-pc-windows-msvc
cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings
cargo test -p arxd --lib --no-run --locked --offline --target x86_64-pc-windows-msvc
cargo build --manifest-path tools/windows-native-replay/child/Cargo.toml --locked --offline --target x86_64-pc-windows-msvc --target-dir target/windows-native-replay-child
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure:: -- --test-threads=1
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::native_replay::kernel_peek_capture -- --exact --test-threads=1
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::native_replay::bounded_matrix -- --exact --test-threads=1
```

The final two Cargo lines are controller-owned child commands, never standalone
operator invocations. The proposed operator entry point for that native stage is:

```powershell
./tools/windows-native-replay/Invoke-BoundedReplay.ps1 -Target x86_64-pc-windows-msvc -CaseTimeoutSeconds 10 -GlobalTimeoutSeconds 360 -ExpectedCases 35 -ExpectedChildren 20
```

This is an experiment harness authorization record, not a product approval
flow or a new user-facing product fence. This proposed driver must require a
reviewed immutable authorization manifest
at a fixed repository-relative path, pin its hash in the run disposition, and
reject absent/mismatched source/environment/command pins before native work.
It executes only the two literal filters above, once each, under one global
deadline. Implementation review must pin its full actual command grammar and
hash/report operations; no driver exists or is approved now. The controller
must contain the harness before it runs, with inherited outer kill-on-close Job
containment and no breakaway, including the suspended child before its inner
Job assignment. Nested Job compatibility with the existing inner limits is a
required safety check in the selected environment, never a reason to disable
either Job fence. If safe containment is unavailable, native execution is blocked.

The matrix filter runs exactly 34 rows (28 failures, four 109 cases, two
uninjected controls). Kernel filter runs exactly one. Zero tests selected is a
failure. Do not invoke fixture helper directly as an extra positive control.
Helper path must be derived from the pinned target-dir artifact, checked and
hashed before copying, not discovered with wildcards. Review must add exact
bounded hash/report driver commands at the implementation pin, before execution;
raw Cargo JSON/path output must not be published. No credentials or owner paths
in report/commands uploaded externally. No dependency download is implied by
offline failures. Rust 1.95.0 and target x86_64-pc-windows-msvc are fixed; record
full compiler commit, Cargo version, dev profile, CARGO_PROFILE_DEV_DEBUG=0 to
match CI, default features and both lockfiles. Set that profile environment in
the approved driver; do not silently compare default local debug artifacts.

Before any future build/report: replace the prospective-source placeholder in
the coordinator disposition with the **full independently reviewed implementation
commit**, Windows/helper/driver/test/lockfile blobs, target/toolchain/profile and
features; pin the exact source checkout used for every artifact. Include bounded
SHA256s of helper and test executable, selector manifest and report, approved
environment class, case outcomes and numeric durations. A current/main alias is
insufficient. Stop on dirty source or any pin mismatch; re-review changed code.
No new SHA/test name confers execution authority.

## Proposed counts and hard bounds; budget admission still open

| Category | Cases | Children | Resumed children | Per-case hard deadline |
| --- | --- | --- | --- | --- |
| At/before-create error 55 | 14 | 0 | 0 | 10 s |
| Suspended post-create error 55 | 4 | 4 | 0 | 10 s |
| Resumed error 55, including eight streams | 10 | 10 | 10 | 10 s |
| Peek 109 success | 4 | 4 | 4 | 10 s |
| L/F uninjected controls | 2 | 2 | 2 | 10 s |
| Kernel peek capture | 1 | 0 | 0 | 10 s |
| Total | **35** | **20** | **16** | **350 s sum; 360 s global native envelope** |

Numbers count fixture children, not Cargo/compiler/test-harness processes.
Use one matrix worker and no fixture grandchildren. Actual per-row counters
are authoritative; expected counts are not proof. Harness/build processes
must also remain under the disposable controller's containment and deadline.

Worst post-create request path includes 1 s request timeout, a first Job.stop
up to 2 s before final drain, and another outer error Job.stop up to 2 s if
final drain fails: **5 s**, not 3 s. Reserve the other 5 s for setup, bounded
queries/writes, post-cleanup confirmation and teardown. Early injected failures
usually spend less, but budget them at the full 10 s. No extra two-second wait
may be added outside the row deadline. The controller's 360 s envelope includes
case setup, all 35 cases, teardown and its final cleanup margin. Per-case/global
deadline expiry is failure with unknown-cleanup disposition, never retry.
Synchronous Win32 calls are not assumed mathematically interruptible: an external
fixture-specific controller must enforce the hard deadline and terminate its
contained harness tree by owned handles. No timeout thread may abandon work
while calling the case green. Controller safety is an implementation review gate.

The existing verify job stays 12 minutes (720 s); CI wall clock must stay under
15 minutes (900 s). Proposed admission allocation: at most 300 s for setup,
compile, existing verification/UI and teardown outside native replay, at most
360 s native, with 60 s reserve = 720 s. This allocation is **not measured or
accepted**. Existing CI duration alone does not establish the non-replay 300 s
allocation. Current information therefore blocks ordinary-discovery admission.
First obtain an authorized isolated bounded measurement at exact pins; review
full-job budget evidence before adding native tests to routine CI. If it cannot
fit, redesign and re-review the fixture or validation split with all labels
still required. Never skip labels/tests, disable tests, raise timeouts, add
retries, change fail-fast or bypass ci-time-budget/agent-review to fit.

The matrix is not first run locally and then silently rerun in ordinary CI.
Each distinct execution context/count needs its own disposition. Do not publish
discoverable native tests before that disposition: required CI would execute
them. No workflow_dispatch or historical CI rerun is proposed here.

## Privacy, failure ledger and completion gates

Keep the accepted diagnostic line <=384 bytes, without truncation:
`c r s i e v w ms o d p h cw cc ja jt jk jv`. These mean closed context,
actual Reason, stage, index, error observation/validity, primary raw wait,
missing entry milliseconds, stdout/stderr numeric totals, final pass, handle
count, child wait/code, Job active/total/terminated counts and violation flags.
Emit **ms=-**; no new entry clock or moved timeout clock. Missing query results
stay `-`. No raw path, argv, environment, SID, username, handle value, object
name, raw output, structure/padding or owner data. New consumption/resume/cleanup
counters go in a separate fixed numeric case report, not an expanded diagnostic
line. Synthetic payload and privacy sentinels only; do not collect owner data
to test redaction. Build logs must be bounded and sanitized before publication.

Maintain a failure ledger in future authorized evidence: exact source pin,
case, expected/actual fixed fields, attempted fix, provider/effort, duration,
cleanup disposition and outcome. Never erase an earlier red with a quiet green
reset. After three failed fixes of the same issue, hand off to the other
provider with all three attempts and failure evidence; do not perform a fourth
same-provider fix. Expected assertion-killed negative controls are distinguished
from failed fixes. Historical privacy flake, main71 ExecutionFailed and
E_PROTECTION remain separate unresolved records, with no S4U/E_PROTECTION retry.

Gates, in order:

1. Independent Grok/Muse scope review checks source pins, oracle/reachability,
   fixture-only extra handles, cleanup/controller containment, counts and budget.
   This sub-agent does not delegate it. Coordinator records acceptance or gaps.
2. Coordinator separately authorizes bounded Sol implementation preparation and
   named compile/pure checks, with no native execution or ordinary-discovery
   publication. Gate 2 permits functions the harness does not discover only:
   no #[test] native registration, ignored-test registration or new discovery
   entry. Registration needs a separate scope decision. The unpublished branch
   must not trigger unapproved CI.
   Obtain a different-provider source/security review of helper, pipe hooks,
   counters, constructor, controller and provenance driver at their final SHA.
   No release hook, policy bypass or changed cleanup decision may slip through.
3. Coordinator makes an explicit, separately bounded native-run disposition:
   exact reviewed SHA/blobs, commands/filters, one environment, 35 cases/20
   children, deadlines, controller/cleanup and sanitized artifacts. This is
   required even if implementation or past ordinary CI was accepted.
4. Independently review the resulting evidence and remaining unverified claims.
   Separately authorize any new native ordinary discovery and its measured
   full-job budget before publication. Normal PR publication, exact-head review,
   all required CI/ci-time-budget/agent-review checks and coordinator-only merge
   remain gates. No outsider auto-merge, bypass, test disable or public publishing.

Decisions still needed, without asking the owner now: whether Grok/Muse accepts
the fixture architecture; whether coordinator authorizes its bounded preparation;
which already-approved disposable standard-account environment/controller can
meet these limits; the exact reviewed implementation/artifact/driver pins; whether
measured isolated and full-job costs admit routine discovery; and separate native
run and publication dispositions. If no such environment exists, identify its
precise missing owner-only administrative prerequisite for later resolution;
do not create accounts, enable features, install or administer anything here.

Outcome of this assignment: this proposal alone was written and committed.
Only read-only repository/Git inspection and document validation were performed;
no build, test, network/GitHub, Win32 experiment, fixture/application launch,
owner/account/service/VM/private-state inspection, installation, CI dispatch,
subagent, source edit or PROGRESS edit. M2 is **incomplete**, real-Runner PA6
record/replay remains **OPEN**, and native/installed/dedicated paths are
**unverified on real hardware**. No incident fix or acceptance is claimed.

## Correction following Grok review b0b4581

Status remains proposed, unaccepted. Coordinator Sol medium corrects
SP-ORACLE, SP-COUNTER, SE-IMAGE, SE-DUP and SE-REUSE above. No code or run
is authorized. Independent error-55 stream oracle (values copied into the
literal 28-row table, never derived from captured records or selectors):

| Labels | Pass | Pre-read stdout total | Pre-read stderr total |
| --- | --- | ---: | ---: |
| loop_out_peek, loop_out_read | missing | 0 | 0 |
| loop_err_peek, loop_err_read | missing | 1 | 0 |
| final_out_peek, final_out_read | 0 | 0 | 0 |
| final_err_peek, final_err_read | 0 | 1 | 0 |

The other twenty error-55 rows have missing pass/totals because no drain
snapshot is supplied there. Each peek109/control row has no failure record
and final successful stream totals 1/1, no drops. A later authorized build
must reject any mismatch rather than regenerating this oracle from output.
All prefill bytes are X; expected totals are defined by deterministic single
byte writes and out-before-err order, not observations made by the fixture.

No approved disposable standard-account controller is named yet. Choosing it,
proving nested-Job containment, final source/helper hashes and measuring the
non-replay CI share remain OPEN decisions before native authority. The extra
parent writer duplicates are proposed fixture-only exceptions pending review;
accepting their design never authorizes compile or execution. All historical
failure and no-retry rules remain unchanged.
