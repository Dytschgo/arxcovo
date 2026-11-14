# Windows main71 native replay: Gate 2 preparation

Disposition: prepared source for independent Grok/Muse review, with controller
integration gaps explicitly OPEN. No native execution authority or runtime pass.
Builder: OpenAI / gpt-6.1-sol / high, 2026-11-14. No subagents or reviewers launched.

Initial HEAD `9f130532004ced02e49aff07ee9b525255aa3707` was clean. Cwd equaled Git
top level; distinct per-worktree/common Git directories confirmed isolation.
`git merge-base --is-ancestor aa49ce1 HEAD` succeeded. Source baseline Windows
blob was `4cb039f4742568a3eb3a124ae3626ac9511b8be2`. Read AGENTS, full KICKOFF,
all 519 proposal lines, and Grok reviews at b0b4581, b2676a4 and ab115ba.
The task-specific preparation boundary superseded kickoff scheduling,
delegation, PROGRESS, ADR and publishing steps. None were performed.

Source commit: `3eedb7736e8876dc1d8907a0d0bfdec3f06ca5ba`. This evidence commit changes documentation only.
All artifacts below were compiled from the source bytes at that commit, before
its commit operation; no source changes occurred between final checks and commit.
This is build provenance, not independently accepted execution provenance.

## Source inventory and limits

- Literal independent 28-row error-55 oracle, explicit origin/Stage/label,
  create/resume/pass/totals/valid/wait expectations. Eighteen pre-loop rows have
  None totals; wait/exit_code have Some(0)/Some(0); final pass is Some(0).
- Eight stream errors, four peek-109 successes and two controls alone select
  prefill. Only byte X is written, one per stream. Empty rows retain no writers.
- Non-inheritable parent writer duplicates follow sealed inheritance list update,
  precede original drops, and live through original cleanup. No duplicate enters
  child inheritance. Final prefill requires the existing stop result true.
- Consumption counts successful armed.take, including 109; resume counts after
  injection and before real ResumeThread. Observation copies counters after
  Invocation drops and before Scope clears. TLS and scope unwind stay pure.
- Every row constructs a fresh Execution with policy_fixture=false and calls
  run_inner directly inside observe; no nested run_ok or quarantine-entry claim.
  PATH equals workspace/bin only, whose sole entry must be cargo.exe; arguments
  are empty. The helper hash is checked every row with a bounded 16 MiB read;
  checked reparse-free ancestor/leaf pins remain held for the entire row.
  No constructor discovery, ambient Cargo, policy exemption or extra work call.
- Cleanup observer borrows only original process/Job handles, after original
  terminate/stop/quarantine, before drop. Zero-time process wait and actual Job
  query must both confirm exit/zero active. Failed outer terminate/stop is retained
  and refuses cleanup credit. There is no extra two-second observer wait.
  An assign child still running when observed fails closed; no timing retry.
- Release macros erase new fixture arguments. Existing actual failure capture,
  token/path checks, Job limits, suspended assignment/identity checks, request
  clock, cancellation/shutdown, 32 final passes, bitwise final operation and
  cleanup decisions are unchanged. Retaining writers changes fixture EOF:
  these cases cannot establish ordinary child-output EOF equivalence.
- Helper is a separate workspace, no external dependencies, publish=false,
  test=false/bench=false binary, only `fn main() {}`; compiled but never launched.
- Native bounded_matrix and kernel_peek_capture have no test/ignore attributes,
  callers, CLI entry or registration. Only four new pure Rust tests are registered.
  Inventory script asserts these source facts without test discovery. It is a
  structural guard, not a Rust call-graph/security proof.
- Diagnostic format remains unchanged and bounded by existing pure tests;
  ms remains missing. The separate numeric parser accepts eight bounded fields
  only. No owner data, raw outputs, argv, environment, SID or handles are reported.

## Controller preparation and concrete remaining gates

The PowerShell entry unconditionally throws GATE2_ONLY before any native work.
selectors.json has registration_enabled=false and exactly two future filters;
its totals are 35 cases / 20 fixture children / 16 resumes. Zero selected native
tests cannot produce evidence. No native tests were selected in this task.

Controller.cs contains compile-checked source for owned unnamed kill-on-close
Job containment, suspended harness creation, assign-before-resume, descendant
Job membership without breakaway, owned-handle termination, and post-exit Job
accounting. Its pure Budget checks fixed row order/counts, 10,000 ms row and
360,000 ms global expiry, known cleanup, exact consumption/create/resume totals
and stop-after-failure. Numeric input times are synthetic; no clocks or process
APIs ran in those pure checks. Native RunPinned and its P/Invoke imports are UNRUN.

Controller integration is **incomplete** and cannot be used for a native run:

1. No approved disposable standard-account environment is selected; nested outer
   and existing inner Job compatibility has not been exercised. No owner setup,
   account/service/VM or private-state discovery was performed.
2. The dormant launcher is not bound to immutable authorization/source/artifact
   pins or to the two exact Cargo commands. The PowerShell refusal is intentional;
   no parameter/manifest can activate it. Reviewed registration and exact command
   grammar remain a separate scope decision.
3. No native wrapper or bounded nonblocking row-event transport is implemented.
   RunPinned's future event callback could block; therefore its source loop alone
   does not prove hard external deadlines. It MUST NOT be activated as written.
   A reviewed owned-channel adapter must report begin/end per row, reject absent,
   duplicate or malformed events, and preserve the same global deadline across
   both filters. Actual counts must feed the numeric records, not expected values.
4. Unknown cleanup returns failure and Job close is a kill-on-close backstop;
   forced kill, failed termination/query, teardown latency, no-reuse enforcement
   and environment retirement remain native controller safety gates. No kill-by-
   PID/name, process enumeration or broad deletion is implemented or authorized.
5. Prepare-Provenance.py hashes only fixed bounded source/helper paths and rejects
   reparse ancestors. Its commit argument is explicitly a CLAIM, not proof of
   clean HEAD or immutable authorization. Future controller must verify those,
   pin exact test executable/selector/report hashes and environment class, enforce
   sanitized bounded reporting and reject all mismatches before native work.
6. The 300 s non-native + 360 s native + 60 s reserve allocation remains UNMEASURED.
   No ordinary-discovery/CI budget admission, publication, dispatch or retry.

These are source-review/run-admission gaps, not measured failures or implicit
permission to complete them by launching a process. No tool/build blocker was
encountered after the two compile corrections below. Review must assess the
prepared partial controller rather than treat its compilation as safety proof.

## Actual checks

Rust 1.95.0, compiler commit 59807616e1fa2540724bfbac14d7976d7e4a3860,
LLVM 22.1.2. Cargo 1.95.0 (f2d3ce0bd 2026-03-21). Target
x86_64-pc-windows-msvc, default features, dev/test unoptimized,
CARGO_PROFILE_DEV_DEBUG=0 for every Cargo compile/test/build invocation.
Both lockfiles stayed pinned. No install, network or dependency download.

Commands actually run (all successful on final restored source):

```powershell
$env:CARGO_PROFILE_DEV_DEBUG = '0'
rustc -Vv
cargo -V
cargo fmt --all -- --check
cargo fmt --manifest-path tools/windows-native-replay/child/Cargo.toml -- --check
cargo check -p arxd --tests --locked --offline --target x86_64-pc-windows-msvc
cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings
cargo check -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc
cargo test -p arxd --lib --no-run --locked --offline --target x86_64-pc-windows-msvc
cargo build --manifest-path tools/windows-native-replay/child/Cargo.toml --locked --offline --target x86_64-pc-windows-msvc --target-dir target/windows-native-replay-child
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure:: -- --test-threads=1
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::native_replay::pure:: -- --test-threads=1
python tools/windows-native-replay/Check-Source.py
python tools/windows-native-replay/Prepare-Provenance.py 3eedb7736e8876dc1d8907a0d0bfdec3f06ca5ba
git diff --check
```

The first exact pure filter passed 6 tests; the second passed 4. No full suite,
existing native tests, CLI or helper was executed. The libtest artifact ran only
these named pure filters. Each final run had 0 failed/ignored tests. Inventory
reported 28 oracle rows, 4 pure tests, zero native registrations/callers.

Controller compilation: Add-Type -Path tools/windows-native-replay/Controller.cs
-OutputAssembly target/windows-native-replay-controller-v3.dll -OutputType Library
-ErrorAction Stop. The same source was loaded with Add-Type -Path, then
[PureControllerChecks]::Run() passed. Only managed in-memory Budget/Event code
ran; no RunPinned, Stopwatch, process API or P/Invoke call. PowerShell AST
ParseFile of Invoke-BoundedReplay.ps1 returned zero parse errors; the script
itself was NOT invoked. No managed assembly is committed.

## Failure and mutation ledger

Provider/effort for every entry: OpenAI / gpt-6.1-sol / high.

| Check | Initial result | Change | Final result |
| --- | --- | --- | --- |
| Rust test compile | 9 E0603 visibility errors | pub(super) for parent hook types/functions | check, Clippy, no-run and pure filters pass |
| Controller compile | CS0649 unassigned Event fields, treated as compile error | explicit Event constructor | final C# source compiles; pure Budget checks pass |

One fix for each distinct compile issue; no repeated three-fix threshold.
No historical/native retry or cleanup evidence exists for these compile errors.

Four temporary source mutants were compiled and ran ONLY the new pure filter:

| Mutant | Result | Duration (s) |
| --- | --- | ---: |
| Ignore consumption in matcher | assertion-killed, exit 101, zero compiler errors | 4.062 |
| Invocation missing stdout becomes Some(0) | assertion-killed, exit 101, zero compiler errors | 4.219 |
| Clear consumption during Invocation drop | assertion-killed, exit 101, zero compiler errors | 4.734 |
| Ignore resume count in matcher | assertion-killed, exit 101, zero compiler errors | 5.188 |

Exact original LF bytes restored in finally and SHA256 checked; no mutant remains.
The first/fourth failed mocked_failure_and_contract_mutants; second/third failed
counters_survive_invocation_and_scope_clears_even_on_unwind. Full final pure
filters and compile/Clippy checks passed after restoration. These expected reds
are pure negative controls, not failed fixes, native marker kills or reproduction.
Native marker deletion, prefill suppression, real create reset and real suspended
resume mutants are all **UNRUN**; no additional native budget was spent.

## Exact source/artifact pins

| Source | Git blob | LF SHA256 |
| --- | --- | --- |
| `crates/arxd/src/development/windows.rs` | `6ee36f7b9948429d76d81a8e853477b9581beddd` | `581d03e2f360a217b27218c2115e56085d80bc42ae03cf69bce1de58d504e648` |
| `Cargo.lock` | `1c5c0fcf2b577f89ed998826ddf6608338b4b523` | `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc` |
| `rust-toolchain.toml` | `3417ee590341edc47a046d94e77ee9bee2b33bf4` | `97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe` |
| `tools/windows-native-replay/child/Cargo.toml` | `6d36690a7b1d4406339de7b63d753de4db44a091` | `e6d25788c0611c605743e738d3d185222bfacbd503bdbcc2f22a9db255eb49c8` |
| `tools/windows-native-replay/child/Cargo.lock` | `dbf05ddef24026e55c6e87b9a38a7c7899412b9d` | `f4e0e27cc75747f2ad09ac4bccda3e0d1cbf34df174e9584074e56957377cd22` |
| `tools/windows-native-replay/child/src/main.rs` | `cd5b7c59da062c25118665b2f164769265545e7c` | `f2cb904fca9f0e8bab8a6ca13b7b88dbea5b769b871c014621e51f01f7a5fb32` |
| `tools/windows-native-replay/Controller.cs` | `13308d3dfaf2b3c7ea1f9650f5c5508faf5a74bf` | `c4bb10c147b10a5c0ef3ee16888fe64cda550d5ef1209c06ebb03b7ac59f0ead` |
| `tools/windows-native-replay/Invoke-BoundedReplay.ps1` | `3eb27afad8fa72acf161cb83b0de85daf9863d10` | `954f3090c7ea3edb7fcbd7685461a3b3500848addc0ae53813bc62efaef8e9e6` |
| `tools/windows-native-replay/Prepare-Provenance.py` | `ea544b83bca9c310198b31d36976bd00dcc64b20` | `33196cc23a1f8c4e4f00756dad2e424cf9ffc8d6c9ab74ae6b34e0b27524f472` |
| `tools/windows-native-replay/Check-Source.py` | `750328dda77a771ef6ff46e9784abea63f227cd1` | `372b5ee874352c9211269c1440c0f5ec1656b241b228bc12d228df6fc1c44687` |
| `tools/windows-native-replay/selectors.json` | `c3ba2efeb5b1de6806b727114c1ae35dba7e25a3` | `69c6fd3a1465a81a0f4a89cbe5f3cbedfc962f998e7c5b06830a69c9dd6601a9` |

Compiled helper (never launched) SHA256:
`916409e0b83151cb067a24453b8d97463e7622fb6d98ed69641c2de5064f4f96`.
Compiled libtest executable, exact logical path
`target/x86_64-pc-windows-msvc/debug/deps/arxd-5547724799c07633.exe`, SHA256
`3d05dfb4189c3041450d8abb3a7d83eb4c717428cc05ce982a1d48e786737dfa`.
The helper's exact path is
`target/windows-native-replay-child/x86_64-pc-windows-msvc/debug/windows-native-replay-child.exe`.
No artifact hash grants execution authority. Source CR count was zero for every
pinned file. Root Cargo/workflow/CI/policy/ADR/proposal/review/PROGRESS untouched.

## Unrun and unresolved ledger

- All real Win32 fixture activity, 35 native cases, 20 children, 16 resumes,
  real kernel peek error 6, token/environment policy, pipe prefill, cleanup,
  nested Job/kill, hard native deadlines and native mutants: UNRUN.
- No ordinary test discovery/list publication, CI, GitHub, push, PR, dispatch,
  installation, owner account/service/VM/private-state inspection or administration.
  No babysitter change and no old Lab Manager or agent-windows-test access.
- Historical privacy flake, main71 ExecutionFailed and E_PROTECTION remain
  separate unresolved records. Run 43545244266 attempt 1 was not retried.
- M2 remains **incomplete**, cause **unknown**, Phase 1 has **no focused real red
  loop**, real-Runner PA6 record/replay remains **OPEN**. No cause fix or acceptance.
  Native, installed and dedicated paths are **unverified on real hardware**.


## Gate 2 retained-review correction — prepared partial, 2026-11-14

Builder: OpenAI / gpt-6.1-sol / high. Initial clean HEAD
`33f5f4e9be3fde99a7badeee45b3b98a92dc6649`; cwd equalled top level, Git directory
was per-worktree and distinct from common Git directory. Read AGENTS, KICKOFF,
full accepted proposal, retained Grok review and prior evidence. Retained review
`4518385` corresponds to supplied `b2dec92`; source `beb590c`/`3eedb77` and evidence
`a04aaf6`/`afcf804` identities are recorded in that review. No assumption of native
authorization follows from this Windows host or its current account.

Source correction commit: `c1f1255aea3f8b6857688d75e61acbc69868a89a`.
This appended evidence is committed separately after that source commit.

### Changes and limits

ST-INDEX: selectors.json row_map is the canonical 35-row schedule; C# carries a
source-checker-verified projection. Rust matrix_index now maps local 0..33 to
1..34: kernel 0, failures 1..28, peek109 29..32, controls 33..34. The checker
verifies filter row sets, map projections and 35/20/16 budgets. Existing native
assertions and independent 28-label oracle remain intact. A pure Rust boundary
test rejects index 34. Native begin/end emission is STILL ABSENT.

The unsafe dormant RunPinned and its P/Invoke imports were removed rather than
left available behind an informal instruction. No native launcher exists now.
SE-DEADLINE/SE-COMPLETE are prepared as managed contracts: OwnedSession starts
and owns Stopwatch internally; MonitorState polls one event at a time from a
sealed bounded nonblocking RowChannel, with no caller callback or stopwatch.
Tests use only synthetic numeric times, never OwnedSession. Complete plus exit 0,
zero active processes, an empty channel and no failure are required for modeled
success. Missing/duplicate/out-of-order/malformed events, overflow and unknown
cleanup fail closed in this model. The native kill-before-return and no-reuse
adapter DOES NOT EXIST, so these are NOT closed native integration findings.

SE-INHERIT: LaunchSpec checks drive-absolute paths, the exact 12-element Cargo
argument vector and exactly PATH/TEMP/TMP/SystemRoot; PATH is one helper directory.
It is a pure grammar validator, not an immutable validated launch object. No
Unicode native environment block, quoting encoder or immutable held-handle pin
implementation exists. PINNING.md specifies fixed manifest identity, exact
source/tool/helper/controller/libtest/selector/report pins, no reparse/hardlinks,
held ancestor/leaf handles, file-ID/hash TOCTOU fences, Unicode environment,
telemetry ownership and termination integration. These remain concrete blockers.

**PREPARED PARTIAL — NOT RUN-READY.** The PowerShell entry still unconditionally
throws. No source registration, launch/API test or native execution occurred.
Fresh Grok/Muse source/security recheck is required before any publication/run;
this implementation sub-agent was forbidden to delegate it and did not do so.

### Checks actually run

Rust 1.95.0, commit 59807616e1fa2540724bfbac14d7976d7e4a3860, LLVM 22.1.2;
Cargo 1.95.0 (f2d3ce0bd 2026-03-21). Default features, dev/test unoptimized,
x86_64-pc-windows-msvc, CARGO_PROFILE_DEV_DEBUG=0, locked/offline. No install.

```powershell
$env:CARGO_PROFILE_DEV_DEBUG='0'
rustc -Vv
cargo -V
cargo fmt --all -- --check
cargo fmt --manifest-path tools/windows-native-replay/child/Cargo.toml -- --check
cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings
cargo build --manifest-path tools/windows-native-replay/child/Cargo.toml --locked --offline --target x86_64-pc-windows-msvc --target-dir target/windows-native-replay-child
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure:: -- --test-threads=1
cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::native_replay::pure:: -- --test-threads=1
python tools/windows-native-replay/Check-Source.py
& tools/windows-native-replay/Test-PureController.ps1
python tools/windows-native-replay/Prepare-Provenance.py c1f1255aea3f8b6857688d75e61acbc69868a89a
git diff --check
```

All checks above passed except Prepare-Provenance.py, which correctly refused
`provenance unavailable`: the compiler-produced helper has link count 2. No
hardlink rejection was relaxed, no helper was copied or launched. All source
inputs individually passed bounded_hash; this is still only snapshot evidence.
The current compiled helper is NOT eligible as a pinned launch file. A reviewed
copy-and-hold implementation is required. Its hash below records build identity
only. Parser.ParseFile of Invoke-BoundedReplay.ps1 returned zero parse errors;
that script was never invoked. Add-Type compiled C# during managed pure checks.

After inspecting the pure code and transitive managed/TLS/parser paths, final
exact Rust filters passed 6 and 4 tests (360 and 362 filtered out, 0 failed,
0 ignored). No Win32 call, process or real clock is reachable in those selected
pure tests. Managed checks cover independent index boundaries, all rows,
partial/open/empty completion, exit accounting, duplicate/order/overflow,
unknown cleanup, row and independent global expiry, empty channel deadline,
and argument/environment/path rejection. They never construct OwnedSession.

Five in-memory C# counterfactuals compiled and were assertion-killed: row 15
create count changed; completion requirement removed; timeout >= changed to >;
arbitrary filter allowed; PATH equality removed. Compilation errors are never
counted as kills. Variants were compiled with unique type names; original files
were never overwritten. The final LF Controller SHA256 was unchanged and CR=0.
These are pure negative controls, NOT native timeout/marker/cleanup evidence.

Failure history (same builder/effort): one C# CS1009 test-string escaping error
was corrected once; one mutation runner stopped on the expected assertion
because PowerShell wrapped TargetInvocationException, corrected once to inspect
the underlying exception and fixed allowed assertion labels. Then all five
variants passed the kill check. No same issue reached three failed fixes. The
provenance hardlink refusal remains deliberately unresolved, without retries.

### Source pins after correction

| Source | Git blob | LF SHA256 |
| --- | --- | --- |
| `crates/arxd/src/development/windows.rs` | `c949e47dae5ac6998d6f406b250c761a43ab9d89` | `48499b5014da7106513f5bd190c0de6300f0e71ce7086c9a70620c7db5980895` |
| `tools/windows-native-replay/Check-Source.py` | `064ff0d945e59859e4139f41e5f30a031ba1d5dc` | `21ec56c1a44874eb9f0ec5ebf432d6db4478609b970ff46a0af03e04f43151f7` |
| `tools/windows-native-replay/Controller.cs` | `73569217b56b6b8ca7bbdd85bc981ccc7f713e31` | `76ad429af198533a1155170a150680b06cf82c24b1fc4a2524d8dab7906806c5` |
| `tools/windows-native-replay/PINNING.md` | `c60ef5fc4e2c4a7a478d89154c57497ca0b14709` | `b42faab8236b893c9bcbd6e21118d0e142e1ecfa5466173dfede11c6243879f3` |
| `tools/windows-native-replay/Prepare-Provenance.py` | `d3a6183316a530df64231bf49a446cd6a54978d1` | `2355e11e15e79791c64d9a8cdcd7917144e47dc57effbb713c4f5db7b0566370` |
| `tools/windows-native-replay/Test-PureController.ps1` | `b4944109e3c1ea9c08799491af6f41704a926b40` | `430922256f5054be171b1e7343617616f1499bce7df061fc8d6dc98d5c93a5c5` |
| `tools/windows-native-replay/selectors.json` | `41a9499660e8b0b40b21bd021f56ba45d760474a` | `e9aa9e6f405c2142d392096926830f0ba5b59be1a8ef46409d75d1513eab9b23` |

Build artifact snapshots (never native-run):

- `target/windows-native-replay-child/x86_64-pc-windows-msvc/debug/windows-native-replay-child.exe` SHA256 `78b1e0c96e4668ef674142822c7487fcec3697ec83182789d66940b4d80bd91b`.
- `target/x86_64-pc-windows-msvc/debug/deps/arxd-5547724799c07633.exe` SHA256 `8b23624184adc6ffd2c297b8c79eede324891d6e0bc364544c7a1447a727780b`.

Budgets **UNMEASURED** (300/360/60 allocation); native runtime **UNRUN**. No
nested-Job result, selected disposable standard-account environment, immutable
runtime manifest/held identity adapter or native telemetry. Unknown cleanup must
stop reuse; no PID/name kill, enumeration or broad cleanup was added/performed.
No workflow/CI/root Cargo/policy/proposal/review/PROGRESS edit, GitHub/network,
install, CI dispatch, push, PR, subagent, owner/account/service/VM/private-state
operation, old Lab Manager access or babysitter operation occurred.

M2 remains **incomplete**, cause **unknown**, PA6 replay **OPEN**; no historical
retry, cause fix or acceptance. Native/dedicated/installed paths remain
**unverified on real hardware**.

Final local identity verification: b2dec92 and retained 4518385 review blobs both
`ad3aee74e06fb1ab62217426eb01797c80f3d608`; 3eedb77/beb590c trees both
`8ee82d5639eed05f4cc39f4756702530aed53f12`; afcf804/a04aaf6 trees both
`9f65e6643b6485eff648cbe72918bed87fe35921`. One read-only rev-parse initially
failed because unquoted `^{tree}` was parsed by PowerShell; quoted Git arguments
then verified all four identities. No source/build/test retry followed that shell
syntax correction. Final porcelain was empty before this evidence append.

## Controller-source partial, 2026-11-14

Builder OpenAI / gpt-6.1-sol / high, implementation sub-agent, no delegation.
Initial isolated worktree cwd equalled Git top level; porcelain was empty;
per-worktree Git directory differed from the common directory. Initial HEAD
`065123cffab4683f26b334084157ab172ec854e7`. Read AGENTS, KICKOFF, PINNING,
accepted native proposal, retained controller recheck through
`git show 2e6259a:docs/reviews/windows-main71-native-controller-contract-grok-recheck.md`,
prior Gate 2 review and scope/oracle/boundary reviews. The named retained recheck
is in local history, not a file at this checkout's HEAD. No review file changed.

Source commit: `b4ef99bea9d328c0e1129da1e2eae63ffb578dce`.
**PARTIAL, not a functional controller adapter and not run-ready.** This is a
local unpublished source checkpoint. No new SHA grants native authority.

Implemented source: HeldFiles opens/holds root-down non-following ancestor and
leaf handles, restricts paths to a supplied root/volume, checks type, identity,
length and link count, reads/hashes through the held stream, and copies a verified
source using CREATE_NEW and one retained read/write handle with FILE_SHARE_READ.
Copy verification never reopens its destination, and the original nlink=2
artifact is refused before copying. No native file operation was exercised.
The write-capable copy handle is private to the primitive and retained through
disposal. These are source properties awaiting independent security review.

Managed additions: fixed 40-byte fragmented-frame decoder with a bounded feed,
numeric bounds and failure latching into RowChannel; argument escaping with an
independent pure reader; exact sorted four-value UTF-16 double-NUL environment;
monotonic last-poll bookkeeping; owned-clock Poll immediately before Success.
The wire decoder has no native transport. Existing row/count oracle stays literal.

Concrete unfinished integration:

- No fixed externally hash-pinned manifest admission, full source/build/artifact
  binding, immutable admission object or runtime dirty-checkout validation.
  Individual externally supplied file pins are not that manifest. No manifest
  bootstrap or generated run-manifest.json was added.
- Held directories do not prove exclusive helper-directory membership. There is
  no closed bounded list of admitted files held through a full session.
- The retained Cargo grammar can rebuild an unpinned harness under the four-value
  environment; no reviewed direct-libtest replacement or rebuild exclusion exists.
- Rust begin/end emission and native pipe transport remain absent. Kernel 0 and
  matrix 1..34 stay undiscovered; no Rust file or product fence changed.
- OwnedSession still does not own a Job. Assign-before-resume, independent kill
  control, bounded TerminateJob/zero-active confirmation and unknown-cleanup reuse
  prohibition have no native adapter. No pure Job-failure mutants were added.

These are missing implementation, not evidence that native work is safe. The
selected environment remains **UNSELECTED**, nested-Job proof **OPEN**, native
runtime **UNRUN**, and 300/360/60 allocation **UNMEASURED**. M2 **incomplete**;
Phase 1 has no focused native failure loop; cause **unknown**; PA6 replay **OPEN**;
native/installed/dedicated paths **unverified on real hardware**.

Actual checks:

```powershell
./tools/windows-native-replay/Test-PureController.ps1
python tools/windows-native-replay/Check-Source.py
git diff --check
```

Final managed run: baseline passed and all eleven compiled mutants assertion-killed:
index, complete, timeout, arguments, environment, held-pin, link2, argument-escape,
frame-eof, frame-output, clock-order. The pure Run call graph was inspected first:
arrays, dictionaries, strings, SHA256, synthetic times, mock byte frames, metadata
scalars and in-memory queues only. It constructs neither OwnedSession nor HeldFiles;
no P/Invoke, process or real-clock call occurs. Add-Type compiles the native imports
but never constructs or invokes the native adapter. The extra environment key,
begin-without-end, wrong counters, duplicate/order, malformed/overflow, EOF and
deadline cases are pure evidence only. Controller and HeldFiles exact LF hashes
were unchanged after in-memory mutations. No compile failure received kill credit.

Failure ledger: managed invocation 1 passed baseline/index then rejected the newly
earlier `begin alone incomplete` assertion for the complete mutant. A Python edit
command failed with SyntaxError before changing files; invocation 2 repeated the
same assertion. An applied allow-list correction fixed it. Invocation 3 reached
argument-escape and rejected its `quoted end` assertion; that expected assertion
was then allowed. Invocation 4 exposed a surviving EOF mutant: a partial first
frame also failed the independent zero-frames check. Added complete-frame then
partial-trailing-frame coverage. Invocation 5 killed EOF/output mutants but the
clock mutation did not compile (CS0649); no credit. Replaced that mutation with
an assignment to zero, preserving compilability. Invocation 6 passed all eleven.
No native fix attempts or native failure loop occurred. A PINNING patch initially
failed context matching without changing files; the corrected patch applied.

Source inventory passed: 28 oracle rows, four existing pure Rust tests, zero
native registrations/callers, selectors disabled. This text inventory is not a
call-graph proof. git diff --check passed. No Rust tests, Rust compilation, fmt,
Clippy or helper build were run in this pass because Rust/helper source did not
change. No provenance main/artifact hash run was performed. Earlier artifact
hashes are not refreshed evidence for this source commit.

Source hash inventory below was computed with a bounded Python read of the fixed
SOURCES tuple (AST literal extraction); all files <=2 MiB, CR count zero. Git blob
IDs use SHA1 of `blob <length>\0<bytes>`; the two changed C# blobs were also checked
with git rev-parse. This is a source inventory, **not** an authorization manifest.

| Source | Git blob | LF SHA256 |
| --- | --- | --- |
| `crates/arxd/src/development/windows.rs` | `c949e47dae5ac6998d6f406b250c761a43ab9d89` | `48499b5014da7106513f5bd190c0de6300f0e71ce7086c9a70620c7db5980895` |
| `Cargo.lock` | `1c5c0fcf2b577f89ed998826ddf6608338b4b523` | `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc` |
| `rust-toolchain.toml` | `3417ee590341edc47a046d94e77ee9bee2b33bf4` | `97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe` |
| `tools/windows-native-replay/child/Cargo.toml` | `6d36690a7b1d4406339de7b63d753de4db44a091` | `e6d25788c0611c605743e738d3d185222bfacbd503bdbcc2f22a9db255eb49c8` |
| `tools/windows-native-replay/child/Cargo.lock` | `dbf05ddef24026e55c6e87b9a38a7c7899412b9d` | `f4e0e27cc75747f2ad09ac4bccda3e0d1cbf34df174e9584074e56957377cd22` |
| `tools/windows-native-replay/child/src/main.rs` | `cd5b7c59da062c25118665b2f164769265545e7c` | `f2cb904fca9f0e8bab8a6ca13b7b88dbea5b769b871c014621e51f01f7a5fb32` |
| `tools/windows-native-replay/Invoke-BoundedReplay.ps1` | `3eb27afad8fa72acf161cb83b0de85daf9863d10` | `954f3090c7ea3edb7fcbd7685461a3b3500848addc0ae53813bc62efaef8e9e6` |
| `tools/windows-native-replay/Controller.cs` | `57b165d30bad70cce6e603d9c676657665679a83` | `6f9ee1c136f59c95ef0c2d5ea34a23dec3ab9bd3e629f01bbe629cba21f5d6de` |
| `tools/windows-native-replay/HeldFiles.cs` | `e9d888627b253ec273b49b965f4e814e7430a43e` | `c23c4046033bbe566a02f7a32ddd1fc253be3722cf541387aa83e6315a8c8bde` |
| `tools/windows-native-replay/Prepare-Provenance.py` | `10b16f1bc958d247bc595273eca2008f715836e3` | `bce03583e848cb3c1d62018b94a1eeb8ed84a1b371720683cf562ff600b92b6c` |
| `tools/windows-native-replay/Check-Source.py` | `d744e3fcf818f2569445cad47ba6a0f08f5da7d3` | `cd1fa4ed0ee8ec412dd011e4a3d382e462e138fa85e453a733cae406e0631d5f` |
| `tools/windows-native-replay/selectors.json` | `41a9499660e8b0b40b21bd021f56ba45d760474a` | `e9aa9e6f405c2142d392096926830f0ba5b59be1a8ef46409d75d1513eab9b23` |
| `tools/windows-native-replay/Test-PureController.ps1` | `b131931064ad1acf679640e71adc8cb1d417ccd2` | `707d002382db3dd231a13e1915df6a49571b4639f7463393287174087167e522` |
| `tools/windows-native-replay/PINNING.md` | `694b1ea7d7f04a8ee717c5705ff4eadea9f98f18` | `01fdb31e107fb3fff27184067783ce8f7f5ec7345c23290bde9f6a47c8d748aa` |

No controller/helper/native launch, native file-identity API test, normal Rust
test/full suite, CLI, account/service/VM/private-state operation, install, owner
admin action, old Lab Manager access, GitHub/network, push/PR/CI/dispatch or
subagents. PowerShell remains GATE2_ONLY. No workflow, root Cargo, policy, ADR,
proposal, review or PROGRESS edit. Independent review remains pending.


## Three-integration assignment: prepared technical-blocker handoff, 2026-11-14

Provider: OpenAI / gpt-6.1-sol / high, implementation sub-agent, no delegation.
**INCOMPLETE. This is not the requested completed three-integration source.**
Fresh independent source review is still required. No run readiness, native
permission, publication, PR or CI admission is claimed.

Initial HEAD `ded6dbc716b2991b294f07384655b29c396edbe5` was clean; cwd equalled
Git top level in an isolated linked worktree (separate worktree Git directory
under the common Git directory). Retained accepted partial source `6af00cb`
and Grok held-pin/decoder review were the baseline. Read AGENTS, KICKOFF,
accepted native scope proposal, PINNING, feedback proposal and recent main71
reviews. User restrictions override general progress/delegation instructions.
No PROGRESS, proposal, review, ADR, policy, workflow or root Cargo edits.

Source commit: `eac5a6d2006d5b3242d3ea5460a049755cc071aa`.

Prepared changes:

- Admission.cs compiles a closed bounded JSON parser: external manifest SHA256,
  fixed keys/roles, full commit/source blob inputs, LF SHA256, 128-bit ids,
  bounds, fixed toolchain/profile/target and artifact roles. Tests reject extra
  keys, mismatched external hash/commit and a root self-hash field. There is no
  usable run-manifest.json and no self-generated admission pin.
- HeldFiles uses parent-relative NtCreateFile child opens, mandatory FileIdInfo,
  link-count checks before/after hash, transient shared volume-root bootstrap
  closed after the first child, and no hardlink-two/copy fallback.
- Actual dormant Rust row functions emit begin/end frames: kernel 0, matrix
  1..34. Serializer has independent literal byte oracles. No native registration,
  ignored registration or caller was added. End frames follow row assertions;
  a panic withholds completion. C# decoder checks frame order and exact segment
  counts 2/70, in addition to MonitorState/Budget accounting.
- NativeJob contains the dormant suspended direct-libtest launcher, explicit
  inherited handle list, four-value UTF-16 environment, Job assignment before
  resume, zero-time process queries and bounded pipe polling without callbacks
  or task-pool readers. OwnedSession owns the started clock, channel, decoder
  and Job across both filters. Abort attempts owned-Job termination before
  confirmation; unconfirmed Job owners are retained, never reused.
- Direct libtest grammar replaces Cargo runtime rebuild grammar in the dormant
  adapter. Toolchain Cargo remains a manifest artifact role. This source choice
  requires independent review and grants no discovery authority.

### Concrete source blockers, not deferred native evidence

1. Admission.Revalidate deliberately throws `helper directory exclusion unproved`
   after retaining/verifying input files. Current directory handles do not prove
   exclusive helper-directory membership against concurrent insertion. A
   handle-based exclusion mechanism and corresponding pure contract checks are
   missing. Do not remove this throw on the strength of a directory listing.
2. Manifest bytes are parsed in memory; a fixed held-manifest loader with
   externally supplied manifest identity/hash is not wired. cleanCheckout and
   reviewed blob map remain caller inputs. Artifact role aliases and provenance
   contents are not cross-bound to the exact source roles. Temp/SystemRoot
   directory pins are incomplete; the parser currently requires SystemRoot to
   be under the disposable root, which also needs reconciliation with a usable
   selected-environment layout. No build manifest confers permission.
3. Telemetry shares inherited stderr with possible harness diagnostics. Invalid
   bytes fail closed, but a dedicated telemetry-only transport plus separate
   raw-stderr discard is not complete. Frames currently carry duration 0;
   that is not an observed runtime duration. No producer call-site omission
   mutant or actual row execution has been verified.
4. Job ownership/race source needs further review and correction: no independent
   proof of cleanup paths after launch/assignment failures, bounded confirmation
   and final-success race. The 358-second cutoff reserves two seconds globally,
   but per-row ten-second teardown accounting is incomplete. In-memory cleanup
   predicate tests do not prove native termination ordering or handle cleanup.
   This is missing implementation, not a reason to run a forbidden experiment.

These source gaps require a subsequent implementation handoff. They are not
owner-only administrative blockers. The new source is retained behind both the
unconditional PowerShell GATE2_ONLY refusal and registration_enabled=false.
Admission itself also refuses. No further source claim is inferred from compile.

### Actual checks and call-graph boundary

Before running managed checks, inspected PureControllerChecks.Run: string/JSON
parsing, SHA256 on synthetic supplied bytes, in-memory queues, numeric synthetic
clock values, metadata predicates and synthetic frame decoding. It never
constructs Admission, HeldFiles, OwnedSession or NativeJob; no P/Invoke,
Stopwatch, process operation, native sink, file or environment inspection is
reachable from Run. The PowerShell runner reads source/compiler reference
assemblies and compiles all imports, without invoking them.

Rust exact pure filters were inspected: frame serialization, diag bookkeeping,
literal oracle checks and synthetic parser/observation checks only. They never
call emit_frame, native row functions, run_inner, prepared_execution, clocks or
native APIs. No ordinary Rust test filter or full suite was used.

Commands run (all Cargo work offline/locked; CARGO_PROFILE_DEV_DEBUG=0):

```powershell
python tools/windows-native-replay/Check-Source.py
pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1
cargo fmt --all
cargo fmt --all -- --check
cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings
```

Final outcomes: source inventory PASS (28 literal rows, five pure tests,
zero native registrations/callers, selectors disabled); managed baseline PASS
and **21 compiled mutants assertion-killed**; fmt check PASS; Clippy PASS
(final run 3.13s, package targets compiled, none executed).

Five separate commands used this exact prefix:
`cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc`
and suffix `-- --exact --test-threads=1`, with these exact filters:

- `development::windows::native_replay::pure::producer_bytes_have_independent_literal_oracle`
- `development::windows::native_replay::pure::counters_survive_invocation_and_scope_clears_even_on_unwind`
- `development::windows::native_replay::pure::oracle_counts_missing_zero_and_literal_bindings`
- `development::windows::native_replay::pure::mocked_failure_and_contract_mutants`
- `development::windows::native_replay::pure::controller_rejects_unknown_cleanup_extra_counts_and_private_payloads`

Each selected exactly one test: PASS, 366 filtered out, harness 0.00s.
Initial build 37.91s; subsequent commands 0.24�0.29s. Restored producer test
was rerun after the mutation below: PASS, compile 3.92s. This is pure credit,
not native row count, child safety or cost evidence.

Toolchain: rustc 1.95.0, compiler commit
`59807616e1fa2540724bfbac14d7976d7e4a3860`, host/target
x86_64-pc-windows-msvc, LLVM 22.1.2; cargo 1.95.0
`f2d3ce0bd` (2026-03-21). Default features; dev debug=0.
Managed compiler: installed PowerShell 7 in-memory Add-Type using its .NET
reference assemblies. No helper build or executable was run.

### Failure ledger and mutation limits

- Initial Add-Type attempt failed compilation because specifying only the JSON
  reference excluded ConcurrentQueue's assembly. Corrected to installed compiler
  reference assemblies; imports compiled successfully. Compile error gets no
  mutation credit.
- Initial Rust check (`cargo check -p arxd --tests --locked --offline --target
  x86_64-pc-windows-msvc`) failed with missing GetStdHandle/STD_ERROR_HANDLE
  imports in the new cfg(test) module. Added imports; subsequent Clippy compiled
  all targets successfully. The cargo-check command itself was not rerun and is
  not claimed green.
- Two fmt checks initially reported new-block/import ordering differences;
  cargo fmt corrected them. Final fmt check passed.
- First frame-eof mutant survived because its old fixture did not isolate a
  partial second segment. Added a two-frame/partial-next-byte test; final mutant
  compiled and failed the partial-EOF assertion.
- Adding full mock-stream checks made the index mutant fail at `stream end`;
  added that precise expected assertion to the runner's allowed kill labels.
- Initial self-field mutant accidentally changed pin-object key parsing and
  failed `manifest valid`; no intended self-hash credit for that attempt.
  Restricted mutation to the root schema; final compiled mutant failed
  `manifest self hash` as intended.
- Rust producer begin-flag inversion: LF SHA256
  `2f7856b932f10d1883cff5bdee52e24eede4d4774ec0b3abae0aa3a1c6c85252`.
  Actual pure serializer compiled; literal byte oracle assertion-failed (exit
  101), then original bytes restored and exact pure test passed. No omission
  or kernel-row proof is claimed. This does not call WriteFile.

No failure reached three same-issue failed fixes. No fourth attempt, delegation
or historical retry was made. Managed mutant hashes below refer to the actual
compiled LF bundle including deterministic type-name isolation; compiler errors
are fatal and never accepted as assertion kills.

| Managed mutant | Compiled LF bundle SHA256 |
| --- | --- |
| index | `22611b2ca42bdb02333f8c1d72dd633d1a46aa2ca8ec51722a45915a27c3594d` |
| complete | `023f5cfcc5b066e5e3de127814ca467212a9feb1ab0e79c5dcedded11fa6c7c4` |
| timeout | `c17fcf4e069ab65b2432235261bc1237c1286cb0abb40be18a343689a0855314` |
| arguments | `8cef1a27b6103eb6fdc1487024c2b4ec9effa37c08360eae1aa93df004addc65` |
| environment | `15e1357115d1c094dfae2e7142d6d05856b093212131fec2526ada3e89344e77` |
| held-pin | `d59a5190675b7a53e5b2d92c8fe906e9e8097c15c794bc3a1b90a3b7a67eeb72` |
| link2 | `5d5162d6469b8f131932d1e14227baaf2d419b09d608b7cb3e66a57ccaab2fab` |
| argument-escape | `bc05463610924438c8dd896af4140706fa1707503a4e58a19009b79dbd79058f` |
| frame-eof | `8842233eab4962bfb3760e139ee7546edb41e08b9698deebdf9892eb711e031e` |
| frame-output | `78fef5f3bb4ecb83d79490ec2e837edce5b1606aeab16b59bc7113e5b3885e28` |
| manifest-extra | `681d937dd1a24d8301487ae03ad580a2ddccbeafc43ebb2c5524666dd078cd37` |
| manifest-external | `f73c2963a1c9220558501af8acf4eb96651f7e5a970f35e825381f3864900e84` |
| manifest-commit | `3b9ffaded072a448a3ea83ec920335eedffc7c8d8d53d9d70ee08ab71401dcf6` |
| cleanup-kill-first | `5d4e2f0a25556ff9b383df1a8c02e4c914c1031ec009a576334e517b203f6cdb` |
| cleanup-active | `249796a3b9a0a0cf46463d482357f5b8ef3f41ea412846359df85e542ed0c375` |
| cleanup-deadline | `c4680e11beb51d5eca51fb5fedcdff1797e8ea9c1537603eaafc74349814f2ba` |
| post-hash-links | `26195ca439cbfd1e82c9cda162b2aabcaeeef6e24b1e9f5c4ca0d5f4ee05da15` |
| frame-complete | `830fc6534c77a5848925fceb3ef9db6d636db48d8a90cb7cce53a198a441f8e9` |
| frame-order | `cd2117124911a027e43b42fc50f7b89313249dfe55a426fcaf8c6d1725a753ba` |
| manifest-self-field | `42c6ddd2bdae46c7f82e3a3e9c6cc4cdefc7bbb756f65b2cc8472d795c63cb2e` |
| clock-order | `ff915d8da68e4bc352d217eddb8a217927f145166440c38bada6f56e1b717c34` |

### Source pins at the source commit

All listed source bytes are LF, CR count zero.

| Path | Git blob | LF SHA256 |
| --- | --- | --- |
| `crates/arxd/src/development/windows.rs` | `81cf50b7c2a3e0da39c825dac9afc61fc0b568d0` | `ef82d058b73dc272b297fdddea9e31d7189cef8a5fded9dbf36af2789ceba81c` |
| `tools/windows-native-replay/Admission.cs` | `df8de5fcf9934d817c3f9366cb62b29b3b30499d` | `33d01b5ebe97a4bd8e189d4a4479ef43432c17ff0c14d77cc56b3a37beb2cef6` |
| `tools/windows-native-replay/Check-Source.py` | `58cdffa88484ec37edbd30bedf1414a654c4d71e` | `940e844dbb6af2eb98cc66fd551dcaaeb80807bfa51f8bc60ddbe549b0af53ff` |
| `tools/windows-native-replay/Controller.cs` | `ac13134483facb791d921d7eaaa2ab2015a37f42` | `74bf6b57e85c7feda4525f8e175f0a97a027373e7b469d61b3094e5548a52c00` |
| `tools/windows-native-replay/HeldFiles.cs` | `47177e878d309a9f6d22e77f830556a94cc1384d` | `8c83e825a03c4bbd182ca4fbc19697b454b91fbb0eba9645d8ed054b6212da0d` |
| `tools/windows-native-replay/Invoke-BoundedReplay.ps1` | `a4c88570d6012ae356c9b4f1e71b2f0d0d76acba` | `29dc8f9f6468d9c03d9defe65381f6f13064ee354aa7637309c0a50b98b6fb33` |
| `tools/windows-native-replay/NativeJob.cs` | `5fa8efba3012d51815f49fb9a38d23a1804f2059` | `f934f165408a9a879090da129f95122a75d2bcd5831ac5bb60cb1063f39a36d4` |
| `tools/windows-native-replay/PINNING.md` | `8bba98fbe889474cdf4c167c9049d9168384809d` | `1675770695a0aeb0348b66ad38b87994a035badc0c142d728eb8af2cd110f56b` |
| `tools/windows-native-replay/Prepare-Provenance.py` | `cbc1ddc275b5543741b7f517699d7e7a8a3120a9` | `1b0f44ef0eb368cc4d47dd343df813fa3b52ef3e0d62ff23fa9b8711596785ee` |
| `tools/windows-native-replay/selectors.json` | `41a9499660e8b0b40b21bd021f56ba45d760474a` | `e9aa9e6f405c2142d392096926830f0ba5b59be1a8ef46409d75d1513eab9b23` |
| `tools/windows-native-replay/Test-PureController.ps1` | `d5514815c0a61899cf20ce42913a9def81e1ff2e` | `d9098c121fec71bfbfa9e1aefb791f2ddccdfd11fe168ec6e75f9f43226b7f32` |
| `Cargo.lock` | `1c5c0fcf2b577f89ed998826ddf6608338b4b523` | `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc` |
| `tools/windows-native-replay/child/Cargo.lock` | `dbf05ddef24026e55c6e87b9a38a7c7899412b9d` | `f4e0e27cc75747f2ad09ac4bccda3e0d1cbf34df174e9584074e56957377cd22` |

Native APIs/controller/helper/process launch/run_inner/native fixture/ordinary
Rust tests/full suite/CLI/account/service/VM/owner private state/process or system
inspection/install/network/GitHub/push/PR/CI/dispatch/subagents: NOT RUN.
No provenance artifact preparation command was executed. All native containment,
128-bit identity semantics, namespace exclusion, nested Job behavior and real
cleanup remain **UNRUN, unverified on real hardware**. Environment unselected;
nested Job proof UNRUN; 300/360/60 and actual native costs UNMEASURED.

M2 is **incomplete**; main71 cause **unknown**; no historical retry; Phase 1
has no focused real red loop; real-Runner PA6 replay **OPEN**. This is prepared
code with explicit technical blockers, not a completed adapter or incident fix.

## ExclusiveNames / SystemRootSeparate source slice — 2026-11-14

Provider/effort/outcome: OpenAI Sol high; bounded implementation completed in
source, with native proof UNRUN. Fresh Grok/Muse review is still required.
No delegation, publication, push, or PR. This is not native run clearance.

### Checkout and scope pins

Initial cwd equalled Git top level; the Git directory was an isolated worktree
directory under the common Git directory. Initial porcelain was empty.
Initial head: `9b52b042856c9e0876c45280c1d559170539c07d`;
initial tree: `cdac7f0d3c941b62deb4993c8f830c766cd35eb3`.
Retained review: `620b3be91ee90dc957b8e03b6f0b53177b6e77a0`
(user identifies original `984d345`; no historical replay).
Accepted proposal blob: `366876f711595c14bde42763e2a263cfe44ca9c2`.
Source commit: `37ff6946532fcef93e28de3aea39582d13c4a6f5`;
source tree: `1674e02f43c7f913d931cd585ea10898222d8946`.
Source commit changes only the five admission/held-file/pure-test/source-check
files below. No Rust, manifest/Git-blob binder, provenance writer, grammar,
NativeJob, workflow, CI, root Cargo, policy, design, review or PROGRESS edit.

Read before editing: AGENTS, KICKOFF, accepted native proposal, full adapter
integration review (including SP-EXCLUDE, SP-SYSROOT and Next implementation
slice), PINNING, Admission, HeldFiles, Controller, test runner and source
inventory. The engineering skill and its selected profile/behavior safeguards
were read; the task's narrower authorization governs.

### Implemented source and limits

ExclusiveNames accepts precisely dot, dotdot and one expected regular leaf,
with distinct names, nonzero 128-bit IDs, proper directory type on dot entries,
and no reparse attribute or tag. SameScan compares the complete name, attributes,
reparse tag and 128-bit ID sets independent of enumeration order.

Revalidate calls RevalidateBoundary, which calls RevalidatePins. The latter
opens the helper directory and launched-image directory through the injected
file/query interface; production uses HeldFiles, pure checks use only synthetic
metadata. Each directory must have exactly its own one pinned executable.
Sharing the two executable directories is refused under this one-leaf contract.
Each proof scans, validates names, opens/reads the leaf relative to that same
directory, scans again, validates names again, compares the sets, and checks the
enumerated leaf ID against the pin. All other file pins retain their existing
hash/length/identity/link checks and sources retain LF enforcement.

HeldFiles caches relative-opened directory handles. Child directory access
remains FILE_LIST_DIRECTORY|SYNCHRONIZE, share read only, FILE_DIRECTORY_FILE,
FILE_OPEN_REPARSE_POINT and synchronous nonalert I/O. The transient full-share
volume-root bootstrap closes after the first child; it is never retained in
the cache. Share read fences ADD_FILE but does not enumerate names, freeze an
unheld sibling, or fence DELETE_CHILD / WRITE_ATTRIBUTES. It is not treated as
exclusion proof by itself. No path listing or query fallback exists.

Production Scan uses NtQueryDirectoryFile class 60
(FILE_ID_EXTD_DIR_INFORMATION), single record, 1024-byte buffer, at most four
calls per scan: three expected entries plus STATUS_NO_MORE_FILES. Restart is
set on the first call of each scan. Unsupported query, partial/overflow result,
unsafe size, extra entry or failure to exhaust refuses. Names, type/reparse
flags and both ID qwords from the actual records feed the predicates. The
relative leaf handle uses share read only and remains retained alongside the
directories; mandatory FileIdInfo and strict links==1 before AND after hashing
are unchanged. No hardlink/copy/reparse/share weakening or fallback was added.

SystemRootSeparate now permits disposable C:\fixture, helper C:\fixture\bin,
temp C:\fixture\tmp and external C:\Windows. It refuses a volume-root
workspace, equal/nested SystemRoot, a workspace under SystemRoot, and unsafe
or external helper/temp/source/artifact paths. Parse uses this predicate and
still confines every source/artifact pin to the disposable root. The old
C:\fixture\Windows acceptance fixture is replaced. HeldFiles separately
requires an externally approved exact SystemRoot path and nonzero 128-bit
directory ID; it opens that directory without following reparses and verifies
volume/type/identity, holding it separately. Temp is also held inside the root.

Handle lifetime is retention until owner disposal, never a scan-local using
block. Current admission always throws before a process exists. Future launch
ownership must retain Admission/HeldFiles until process exit AND zero Job active
count; this slice does not establish that supervisor/lifetime behavior against
a real process. Native directory share, enumeration completeness/record layout,
SystemRoot opening, file identity and lifetime through confirmed death are all
**UNRUN, unverified on real hardware**. No API experiment was authorized.
The final overall refusal remains `remaining admission gates unproved`.
No usable Admission/OwnedSession run is enabled.

### Executed checks and call graph

Commands actually run (repository-relative):

- `Get-Location`, `git rev-parse --show-toplevel`,
  `git rev-parse --git-dir`, `git status --short`, `git log -4 --oneline`;
  fixed source/document reads and bounded rg searches.
- `git rev-parse HEAD`, `git rev-parse 'HEAD^{tree}'`;
  Python fixed-file SHA256/CR counts and `git rev-parse HEAD:<path>` pins.
- `pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1`
  five invocations: three interrupted negative-control runs, then two complete
  passing runs, with the final run against the committed source bytes.
- `python tools/windows-native-replay/Check-Source.py` twice, both exit 0:
  28 oracle rows, five pure Rust registrations, zero native registration/callers,
  selectors disabled; this is structural inventory, not native proof.
- `git diff --check` repeatedly, all exit 0. Diff/stat inspection, explicit
  five-file staging and the conventional source commit above.

Before managed execution, inspected Run and its helpers: synthetic JSON and
hashing, literal frames, synthetic elapsed values and metadata only. The new
AdmissionPureChecks invokes the production predicates, RevalidatePins and
RevalidateBoundary through mock IAdmissionFiles/IAdmissionDirectory. Mock
methods only return arrays/bytes or count calls. They never instantiate
HeldFiles, Admission, OwnedSession or NativeJob, read a clock, open/query a
file, run provenance, or invoke any native API. Defining DllImport during
Add-Type is compilation only. Path operations in the decision path are lexical.
No native effects are reachable from either pure Run method.

Final managed run: baseline pass, all **45** separately renamed/compiled
mutants assertion-killed, exit 0, tool wall time 8.71s. Add-Type used the
PowerShell 7 reference assemblies; no SDK install/network or version probing.
Compiler errors are fatal, never assertion kills. Source files are never
modified by mutation: variants live in memory, use distinct type names, and
all five compiled/runner source files are checked for exact byte SHA256 and
zero CR before and after. This is an exact LF unchanged check, stronger than a
rewrite-based restore. Check-Source is separately pinned below.

Coverage includes extra DLL, missing/repeated/wrong/reparse leaf, reparse tag,
dot/leaf types, zero ID, changed low/high IDs and attributes, set reordering;
both executable directory scan wiring, changed scans, relative-leaf result
size/ID, source LF, unsafe roots and helper/temp/source/artifact pins; parser
external Windows acceptance, nested Windows/outside artifact refusal; and the
remaining unconditional admission refusal. It gives no native safety credit.

### Failure ledger (retained)

1. Read-only Git pin command initially used unquoted HEAD^{tree}; PowerShell
   mangled that argument and Git exited 1 with ambiguous argument. Corrected
   once to quoted `'HEAD^{tree}'`, which returned the initial tree above.
2. Managed run 1: baseline passed; names-leaf mutant
   `42f14ded2b712cdfc7adc35fbcceb92e2e86e5b4d7fd7f65ccd19246b7c6600a`
   survived because final membership redundantly enforces the expected leaf.
   Corrected the semantic mutant to disable both leaf checks; no production
   guard was weakened. Tool wall time 5.68s, exit 1.
3. Managed run 2: baseline passed; dot-type mutant was a literal false branch,
   causing Add-Type CS0162 unreachable-code failure. Not an assertion kill.
   Changed only the mutant to a nonconstant impossible attribute comparison.
   Tool wall time 5.68s, exit 1.
4. Managed run 3: baseline passed; leaf-ID mutant deleted the entire body of
   an if under foreach, producing Add-Type CS1525. Not an assertion kill.
   Replaced the deleted statement with an empty block. Tool wall time 6.90s,
   exit 1.
5. Managed run 4: baseline and all 45 mutants passed/killed, exit 0, 8.39s.
   Then folded the new pure helper into already-pinned Controller.cs, added
   parser/scan assertions and clarified directory sharing comments; final run
   5 baseline/all 45 killed and exact LF checks passed, exit 0, 8.71s.

These are distinct negative-control definition issues, not three failed fixes
of one production issue; no ledger reset or fourth same-issue attempt.
Extra/missing/repeated and volume-root mutants deliberately inject acceptance
of forbidden snapshots because deleting one redundant local check alone does
not remove the complete semantic guard. Wrong-leaf disables both redundant
checks. Every mutant still compiles and fails a production assertion in the
final run. The new changed-scan wiring fixture changes the dot entry ID so the
leaf-pin equality cannot mask loss of the scan-comparison guard.
All failures used pure data; cleanup disposition: no native handles/processes.

### Final source pins

Every row has CR count 0; SHA256 is over exact LF bytes at the source commit.

| Path | Git blob | LF SHA256 |
| --- | --- | --- |
| `tools/windows-native-replay/Admission.cs` | `dd8d58903469ac438c51c98f42597985058e6990` | `9ee0b8a9df333e03a84caf2793ea983305202a89c44e662980d83ec216002ded` |
| `tools/windows-native-replay/HeldFiles.cs` | `9526c04b368664ee23e6a6efbb27e554cf5b10b9` | `9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643` |
| `tools/windows-native-replay/Controller.cs` | `9039215000a5ac9fbcae35bd1bb8300a6c840d27` | `9b902a15898cb4e0155821e17b181fdbe36191870a8590000aa4ef92423808d5` |
| `tools/windows-native-replay/Test-PureController.ps1` | `e83263cc2f1095f272a166259bd74815b1a48df6` | `9e20bcc726d31205be55c6b7b7e467f867a1e61fc1d40d303099da3bf6e7e729` |
| `tools/windows-native-replay/Check-Source.py` | `aca568d8c501b1fa340b74f68770c8637fea370e` | `4bef03b97b6a0b470bd09a2056c5ff3bd65915639c9037f08c294dc8cf34e75c` |
| `tools/windows-native-replay/NativeJob.cs` | `5fa8efba3012d51815f49fb9a38d23a1804f2059` | `f934f165408a9a879090da129f95122a75d2bcd5831ac5bb60cb1063f39a36d4` |
| `tools/windows-native-replay/Invoke-BoundedReplay.ps1` | `a4c88570d6012ae356c9b4f1e71b2f0d0d76acba` | `29dc8f9f6468d9c03d9defe65381f6f13064ee354aa7637309c0a50b98b6fb33` |
| `tools/windows-native-replay/selectors.json` | `41a9499660e8b0b40b21bd021f56ba45d760474a` | `e9aa9e6f405c2142d392096926830f0ba5b59be1a8ef46409d75d1513eab9b23` |
| `tools/windows-native-replay/PINNING.md` | `8bba98fbe889474cdf4c167c9049d9168384809d` | `1675770695a0aeb0348b66ad38b87994a035badc0c142d728eb8af2cd110f56b` |
| `docs/reviews/windows-main71-native-replay-scope-proposal.md` | `366876f711595c14bde42763e2a263cfe44ca9c2` | `38301eec15029b58db8cbf42d2df22fdba6f337dc1fac7404585700d9aa3c5e2` |
| `docs/reviews/windows-main71-adapter-integration-grok-review.md` | `8ae1393db2f6a9b167ca397a8aff0585cf43f2ec` | `73d73c8df19eed5d92c5cc2170e971d2857ead0f38475259c0dd1c2030f239db` |
| `Cargo.lock` | `1c5c0fcf2b577f89ed998826ddf6608338b4b523` | `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc` |
| `tools/windows-native-replay/child/Cargo.lock` | `dbf05ddef24026e55c6e87b9a38a7c7899412b9d` | `f4e0e27cc75747f2ad09ac4bccda3e0d1cbf34df174e9584074e56957377cd22` |

### Final compiled mutation pins

These hashes include deterministic type-name isolation in the actual compiled
LF bundle. Every row below was assertion-killed in final run 5.

| Mutant | Compiled LF SHA256 |
| --- | --- |
| index | `0d07d75be68a7f315acd25b3a7e1ee3f92b3654247da424a346f3c7760829a11` |
| complete | `c50d0b0845b1eb87845cead5097c14e20d947b55e929b0a3c223d911ed005622` |
| timeout | `b8ff1b87a80190b0e0a7a6f14290b40d03b9c4b8f0e40cd15fa0017116a10838` |
| arguments | `b582c81f527be0d1d41c1d0d037b787cc878a9732f61569648bf8b4b19d31116` |
| environment | `94c8ed41517511a0b0d5b1885346937f870baeeac42776249ae823b1705b8d43` |
| held-pin | `4fb80d703f895b27d99a23851191ffa359f6480e7194bf0b5eb4971fc433b032` |
| link2 | `5b2de72a63689f46f66cfb711c82628e8d5b72cd808fba0d8c41c24857ba2481` |
| argument-escape | `7ade46717839b1025b39abd0d2ee090eba4f1d05112ef0b4d47f32a4718499c0` |
| frame-eof | `b2bf01fd6c9f5ae68c6f81e4fb1becad3132d420aaabfde8c85f568fc3899f9e` |
| frame-output | `446962c37761bed8716e2647bcd89df4a0f6c22475463b9d523f2903ac84569a` |
| manifest-extra | `4771f249cd40b156beaf297e2a81516f6a52e7afb7081d52c2c4f0625e185d20` |
| manifest-external | `2a38499de3ac0f5b4d3e56288fc716683a94f3443b87ad8495532bb4ce94f75a` |
| manifest-commit | `ae6a4ea4a76250037baa335ec4bc7200094f722681510af7fabeba759a545d35` |
| cleanup-kill-first | `ead375c99849cc872b7966da1bacf17ba530c5c624d65b931faef754aa46219f` |
| cleanup-active | `7eb240897d74696ce8576d1f0d81743b788aca0e359b7167a6e7280d024bb8c0` |
| cleanup-deadline | `e3d11c3abf8cb4e8165a0d1a778c150fd80b75a93759ed1633c69af9b11006bb` |
| post-hash-links | `4b0be2b2724369a2a2087b2d40292993c467d8ba99ed41a83af1d199c9627391` |
| frame-complete | `fb53d103092855fe4327b97f11b48ecb5f8c7ba14dfc2733bc6ceb711b12c750` |
| frame-order | `b17f5be6fe840148c278c82b670030d98bd0972c3c4c376711fd9f0ee92e0977` |
| manifest-self-field | `ae0208a0a8d7c4d96c99d5efbfc8a49d5172804bd5982292465e6cc6bf89b432` |
| clock-order | `08d8ec1361481058a4c34c1747a0c50e8d6a526846875b76d20f4d7c9c4d26ea` |
| names-extra | `5afd918b392a5ba0413efbaf5a8b4ec1cc15df27d145a43566dc6293453540d1` |
| names-repeat | `8abd214b2440d15545275e3ac2c45c0edb6ea92aea6fb8048754e292ea5e1ddb` |
| names-missing | `684906ab4b6e470315b82fe9f560b8cc1357180dc032e712e9848f5492dc5eea` |
| names-reparse | `1576c8921a69ac617c9aa6ccd1f35d82c79dcd4e9272ba9df5c32252a2388a62` |
| names-tag | `5e00ba8032848f95a1fb46e88c854d3cd2c7c9686f124a87fde01b229c2e342b` |
| names-leaf | `af947a7d80e7d780e470aad3d198746c199aa9a46b000cc4bf7118c80967f05a` |
| names-type | `a55ffaf2a9630cdac4adc8734f3b01aee2740814c4cd9aaf63dc676c9263ceaf` |
| names-dot-type | `52ca2b211197f455060a91c4d169ee28ca0c3cde12a07d78a52fec6079d94327` |
| names-id | `e68f345c19446d751138afdbb3e49ad45f1121cae6d2f7a3133dd1536c0d22b0` |
| scan-id | `0c637376ef99a3bd55699d71671a6b49a8f233183fb1cb7920db3f50717dd20e` |
| scan-wire | `de8bb896e49c1e1b020cc4bd5cf49522bdc9926e9fdb9390833aa51dc7f2f908` |
| scan-first-wire | `c38d7c3593fcfed4730e28f7d43cab8b9bf47952a4e72eb7c5f99068d2f45bfb` |
| scan-second-wire | `166320f0b456a60f391e9afac980ce99596a17c845d30e2af4a1c9ab64143099` |
| leaf-id-wire | `b770a41e0b40a361f9521597d20631f58ae3c94b5614566f65464d7aa50938d6` |
| leaf-size-wire | `1c15f81dd817fad499ae88e880fb1696dc7fb8d91327efc5e130bc04cc848f78` |
| root-volume | `e7e6b9c3edaf5e7b710690e7495d5328fae70b6ec5ce94b4be701ccb247f0485` |
| root-nested | `efeb95018ec7022639cef6179e7ff6c50a559aad9707b6aa7ceaf5cb609734a1` |
| root-equal | `9f3933821f987b6d23b9e11142600b246910f7e3aaca5f2eff271312ca5eede0` |
| root-parent | `72be711ffe010d0f5446763235aa04246c5a0cc19de366e2371c61cbdbceeb1f` |
| root-pins | `be4f574f7f08b355ccba1db0cfab8edcb265ed697d86f33df97f53fadb8b964e` |
| root-helper | `5e2cf5db250bff927452e214445c6eb06c2f2888db02274493d77eca334d4c4e` |
| root-temp | `0f92a8e1f857198a8057e4f16fcd33003a6637db03f022257f0d79fff379c994` |
| root-wire | `ecf6ff39aaddc5187395db10286a2078f61a26a561477eb8aa5ccc370e49cad5` |
| final-refusal | `cc27faa6738ccc038ab8bfe237b8b3881e85b2401229e21007f8d9a7ca1edd75` |

Not run: native/runtime APIs, controller/helper/child launch, PowerShell entry,
run_inner, native fixture, native or ordinary/full/discovery Rust tests, CLI,
filesystem/process/system/private-state inspection, provenance preparation,
install/network/GitHub/CI/dispatch, owner/admin/VM/account/service actions.
No Rust edit or Cargo/fmt command was needed. Gate 2 entry remains GATE2_ONLY;
registration_enabled remains false, with no test/ignored/CLI native entry.

Remaining gates: fresh independent Grok/Muse source review; held fixed manifest
and Git-blob/provenance/lock binding; dedicated telemetry transport; accepted
launch grammar; deadline waiter/supervisor including synchronous-call limits;
separate selected standard-account environment/nested Job proof, bounded native
authorization, measured 300/360/60 and full-job budget; separate publication and
required-check/coordinator merge gates. None is cleared by this source slice.

M2 **incomplete**; main71 cause **unknown**, no historical retry; Phase 1 has no
focused native red loop; real-Runner PA6 replay **OPEN**; all hardware paths
**unverified on real hardware**. Native proof **UNRUN**.

Evidence-write ledger: apply_patch refused to decode the pre-existing evidence
file as UTF-8 (invalid byte at offset 39689). No write occurred. Appended this
note as UTF-8 bytes without rewriting the pre-existing byte prefix; exact prefix
equality was checked. This is an evidence encoding issue, not a source/test fix.

## Held-manifest binding slice — 2026-11-14 (source only)

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent. No subagents.
Source commit: 06a257ee57e0fc604ac5dc69ba4665565ec24dde.
Initial HEAD: 7dc9134 (clean). Cwd equalled Git top; worktree Git directory
and common directory differed (isolated linked checkout). Read AGENTS,
KICKOFF, engineering skill/profile/behavior safeguards, accepted native scope,
PINNING, and both requested Grok reviews. No PROGRESS/design/review/CI/workflow,
Rust/root Cargo/policy edits. Source delta only the five named replay files.

Baseline: admission 37ff6946532fcef93e28de3aea39582d13c4a6f5,
exclusive-name source retained 77e5dcb0f2be8c5bd076d6880f65e47b19b9d5e6.
Review objects 984d345c71ea91dfce069c2000da2caa1f8556d4 (retained620b3be)
and 852b9b133b70739f3c9c6bf75df431ea09413f91 (retained66018b2).
Initial git diff 77e5dcb -- tools/windows-native-replay was empty:
source unchanged since the review. No source baseline silently replaced.

Dataflow: coordinator external disposition SHA256 + id -> held fixed
tools/windows-native-replay/run-disposition.json (<=65536) -> strict pinned
manifest hash/id/size and reviewed commit/blob mapping -> held fixed
run-manifest.json -> held source/artifact bytes -> Git SHA1 blob framing,
LF, provenance and exact lock/selectors aliases -> private copied Facts.
Both documents require runtime_authority:false. Provenance additionally
requires native_executed:false. The trusted disposition attests the commit
and its mapping; this slice does not read/validate a Git commit object.
The root hash is FUTURE coordinator input external to both documents, never
invented permission. No real manifest, disposition, provenance or environment
was generated. Parser-only fixtures still accept bytes for isolated schema
tests; admission's constructor has no byte/blob-map/cleanCheckout overload.

Every required role remains closed and hash/size/id-bound. Provenance's
source_sha256 checks the SAME held source bytes used for Git SHA1.
Workspace/helper locks and selectors require exact corresponding source
paths, ids, sizes, SHA256 and bytes. Other path aliases refuse. Id format is
exactly 32 lowercase hex/nonzero; literal qword-order and independent
hello-LF Git blob oracles are present. Native HeldFiles implementation is
unchanged: bounded relative opens, type/reparse/volume/identity/link1 checks
before and after hashing, retained ancestors/leaves, and both complete
exclusive scans remain. These are SOURCE claims, not kernel proof.

Callgraph inspected before managed execution: PureControllerChecks.Run ->
existing synthetic parser/clock/frame/metadata oracles, AdmissionPureChecks
mock directory interface, BindingPureChecks.Fixture -> Admission.Bind ->
Fixture.ReadPinned/Dir.Scan/Dir.ReadLeaf/HoldEnvironment plus in-memory
JSON/hash/schema/blob/provenance/alias checks. No HeldFiles/Admission/
OwnedSession/NativeJob instance is constructed by Run. GitBlob uses SHA1
ASCII framing plus bytes, not a caller dictionary assertion. No native
imports, filesystem admission, wall-clock reader, process API, provenance
utility or Rust fixture are executed by these pure checks. The PowerShell
test harness reads fixed source and compiles in memory; Check-Source.py
reads fixed repository source/selectors only.

Tests: strict/malformed/duplicate/unknown document and role fixtures, missing
and unknown source maps, externally pinned hash mismatch, held-byte mismatch,
blob mismatch despite both documents agreeing, commit mismatch, provenance
source/helper/commit/authority/native claims, lock/selectors path/id/byte
aliases, forbidden role alias, limits, lowercase/length/zero ids, LF refusal,
immutable fact copies, actual binder complete read/scan counts. Existing
directory/path/link/type and final-refusal controls are retained.
62 compiled mutants (45 retained + 17 binding controls) assertion-killed.
Compiler errors NEVER count as kills. binding-held-manifest mutates the
shared held-read hash guard: an isolated same-size direct held-read assertion
kills it; Parse also independently hashes the manifest.

Actual validation commands:
- python tools/windows-native-replay/Check-Source.py — exit 0, inventory/map/
  binding output, 28 oracle rows, 5 pure Rust registrations, 0 native
  registrations/callers, selectors disabled. No Rust tests executed.
- pwsh -NoProfile -File tools/windows-native-replay/Test-PureController.ps1 —
  final exit 0; managed baseline green; all 62 mutants compiled and killed;
  exact LF unchanged checks passed for five inputs. Final full output below.
- git diff --check — exit 0 before source commit.
- Read-only git cwd/top/status/history/rev-parse/diff inspections; scoped
  git add and source commit. No time/version/budget measurement claim.

Failure ledger (not reset; distinct issues, no three same-issue failures):
1. First compilation: CS0128 duplicate local leaf. Renamed byte array to
   leafBytes; next compile succeeded. One correction.
2. First compiled baseline: binding manifest limit assertion failed because
   fixture Seal overwrote the bad size. Split SealDisposition/SealManifest;
   baseline passed. One correction.
3. Mutant compile: BindingPureChecks type already exists. Added that test
   class to per-mutant rename list; compilation succeeded. One correction.
4. New binding-held-manifest mutant survived: the remaining size/Parse hash
   guards already rejected the mock corruption. Added a direct same-size
   held-read hash assertion; next/full final run killed it. One correction.
Expected mutant assertion kills are negative controls, not failed fixes.

Runtime UNRUN. No entry, provenance, native API/file-admission, native fixture,
controller/helper/child launch, run_inner, ordinary discovery/Rust test,
clock/process effect, network/install/GitHub/CI/dispatch/account/service/VM/
owner/private-state operation. GATE2_ONLY, selectors=false, no OwnedSession
construction in Run, and final remaining-admission-gates refusal remain.
Transport/grammar/deadline gaps and all prior gates are open. Fresh Grok/Muse
source/security review required BEFORE publication; no publication/merge here.
M2 incomplete; main71 cause unknown/no retry; Phase 1 no native redloop;
PA6 OPEN; all hardware paths unverified on real hardware.

Prefix preservation: preexisting 64796 bytes SHA256
096747fd5fcccc0f2db43326f6985d4ea8d2188f9cde78774a3770a02e2fb767,
byte offset39689 = 0x96. Append is binary UTF-8/LF; prefix verified byte-for-byte
against that length/hash, including 0x96. No encoding rewrite.

### Exact source pins (LF file bytes and Git blob framing)

| File | LF SHA256 | Git blob |
| --- | --- | --- |
| Admission.cs | b6c1aa6b39d618a85e2183988cc95b44c965dfbb44670b2c2c4382d336754289 | f26127281e627d3cca76312846a18243206cd063 |
| Controller.cs | 0941fa6fb8add01470e87e7084d753acc8b967ffd76c2f0bc621bee24cc1075a | 77080352f9061522de94efca18cb9137371203e0 |
| HeldFiles.cs | 9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643 | 9526c04b368664ee23e6a6efbb27e554cf5b10b9 |
| NativeJob.cs | f934f165408a9a879090da129f95122a75d2bcd5831ac5bb60cb1063f39a36d4 | 5fa8efba3012d51815f49fb9a38d23a1804f2059 |
| Test-PureController.ps1 | 2e9b07ff121769509e65797c285f262cb255938d0a4f17b643b900c46d17cb30 | df6e4c8ce9cb8a846c4d37f72ab46be319e7bb1c |
| Check-Source.py | b3e9075a8dceed2c38ae4d6a032322f3d8dd1c77c7fbfa48200415593e3cef18 | f90717b6331a786fd108446d417359a50a0284ca |
| PINNING.md | cc0472cf5d1e82b2280a9915a04802a5b72876bcfa19340e179bac4354890bfa | 630581e2c7269608ff5c66d3b454a18586ac60ab |

### Final managed output (exact hashes; LF normalized output)

```text
managed pure baseline: pass
mutant LF SHA256 index: 36ee13e10410937963e5c71b1a731289b5c9a0b82a43913906f50de87728847a
assertion-killed: index
mutant LF SHA256 complete: 6ddebe6dd08c042797c878c29ca3515f20293da24d7567490b687c5f8d7315b9
assertion-killed: complete
mutant LF SHA256 timeout: faeec85535bda0742d321a26eaf4ed3c671540eb3c7233d94412e152ef563fef
assertion-killed: timeout
mutant LF SHA256 arguments: 83ff2ea34656d11fce177d9ab55e1602e2bf2fffa5de9c4ebc4f57c25df6eece
assertion-killed: arguments
mutant LF SHA256 environment: a1609c0e22a4ccfbc5b4cbc2d5cb9a3cc76f3e507899f82faffb243b3e391cf9
assertion-killed: environment
mutant LF SHA256 held-pin: 7ba95c9e0f99893bc24ebe2273dd9f21413f6a2195c2f1c74c08b2286ec5824b
assertion-killed: held-pin
mutant LF SHA256 link2: 88e92069aa7ee89178c7a67e66b18e679250fecb477dfdb09c9899aa5e6cabe5
assertion-killed: link2
mutant LF SHA256 argument-escape: dd0953851ec74378d5d6d8bab947dc6018ae557d98c3473bbf6199935a93efa6
assertion-killed: argument-escape
mutant LF SHA256 frame-eof: d405759f94f59dabeaaee7498ec74748c580f0e607ce5a1377147bad0987be26
assertion-killed: frame-eof
mutant LF SHA256 frame-output: b9baec6e02959137a111084a1f99f1214afa3d19d12551e8ba8dd060ddbff7b2
assertion-killed: frame-output
mutant LF SHA256 manifest-extra: 708f7122fe6e5ed890e5408f9ed428ba1a8f976682271b2abb9e105742219caf
assertion-killed: manifest-extra
mutant LF SHA256 manifest-external: 30055780c577bddf71d994f201e8efd0eba9ef38d8638905495009bb577cb41e
assertion-killed: manifest-external
mutant LF SHA256 manifest-commit: acefd3ec9867bb56a14dbb60a4e5fffb2e02a0d549d8840c42f46c0c3d8f7d11
assertion-killed: manifest-commit
mutant LF SHA256 cleanup-kill-first: e310a8e444489d95ac69ecfdb30bab3ce20090f04a49caba62262d7d2ca6a348
assertion-killed: cleanup-kill-first
mutant LF SHA256 cleanup-active: b60b5c8fe6e69afbf1a8b6edfeb73ce98520ce8d2f609c078298439a93f79968
assertion-killed: cleanup-active
mutant LF SHA256 cleanup-deadline: 5b90c7a84e82b88fc308ae30ab9c1565a4f3ebd5da4cc5e6a8bdf5eb0d8d7228
assertion-killed: cleanup-deadline
mutant LF SHA256 post-hash-links: a27b269ee425c6ccbe3bd49b7876d79c60f0abf9605a823a62d560d5c28ea7c3
assertion-killed: post-hash-links
mutant LF SHA256 frame-complete: 9b0650898795bddbaedc7e9ff0a587da20edd4d390edf89d1019671b1a98c324
assertion-killed: frame-complete
mutant LF SHA256 frame-order: febe49ac7ebe17aac5bb4a2d0f48262c5c96c5d9350281c5fdea03bcd78a5987
assertion-killed: frame-order
mutant LF SHA256 manifest-self-field: a7e197a217eefb3bd19602c7164fff8dd1d404382f70d1ac953c14dd7ec1c30f
assertion-killed: manifest-self-field
mutant LF SHA256 clock-order: d3de6976b2992ac55acf5c33576aee789a6ded922e07c5964345be9df892c4c2
assertion-killed: clock-order
mutant LF SHA256 names-extra: d9ea5da8a2eefa1d824a4663014811ed7d272b80265cb84d0aeed21bf5e92aa8
assertion-killed: names-extra
mutant LF SHA256 names-repeat: 3973858c5492f0a21039fc5b2e009554699e1ae7b13c53fccb6e0dfc91a3b8c5
assertion-killed: names-repeat
mutant LF SHA256 names-missing: 55abc5e956042c72e789e2985519fbd3b63a2cb29cee278274b84902e533bef6
assertion-killed: names-missing
mutant LF SHA256 names-reparse: 00634a707a9b32f2d2a0c5a627054c75e20c811a7a8c074c5b3c6c770fe913bc
assertion-killed: names-reparse
mutant LF SHA256 names-tag: d88b44326ff2fc32be9b41d14ece45dec57cdae91ebb20d26e68f79cafe5001b
assertion-killed: names-tag
mutant LF SHA256 names-leaf: 61f97d6eaadaf8098697d01b45af6a6169fbb35c5b4b89d9871c9e22360e0c81
assertion-killed: names-leaf
mutant LF SHA256 names-type: d81ccea644aa749d97c056832c4976bf6cd8c4e3d441c7f09ef486e3c4762a14
assertion-killed: names-type
mutant LF SHA256 names-dot-type: d7969304072add73f0214e9155baecd87b7656e75b19326cd16bd6869dd96181
assertion-killed: names-dot-type
mutant LF SHA256 names-id: 95aee1e9eeb4ce82c865e4e5124f62320173137fda1b259a39eaef4f453282c4
assertion-killed: names-id
mutant LF SHA256 scan-id: 46a3136934f6c7e0284d967fe8d14882c1ae3e2c7d9f0c309471912918bdba79
assertion-killed: scan-id
mutant LF SHA256 scan-wire: cdef654c2c34ca12ba8527c3d411a3708dd3b6a51fd6bbeb12bf4b250950dcc5
assertion-killed: scan-wire
mutant LF SHA256 scan-first-wire: 3d4821489f59f9abb7410e6e7639dfd789f6978fee38c8874f75663e45e83c69
assertion-killed: scan-first-wire
mutant LF SHA256 scan-second-wire: f61105f82075ed39dcdbe47124751194dff4f105e6495a0b4a24bd8c142d784b
assertion-killed: scan-second-wire
mutant LF SHA256 leaf-id-wire: 1df923ef92d1c788bf9af718453d9f91d400c1242d60bb1a3585daa992b00ac9
assertion-killed: leaf-id-wire
mutant LF SHA256 leaf-size-wire: e46414279de458e8ffe2525db521e2de8bf49ee222013e82a7e665f9976722a3
assertion-killed: leaf-size-wire
mutant LF SHA256 root-volume: 7d0715ce62868208f4bdbb7a6eea471d286e7c9dd8beab0519c80aa5ffbc5b33
assertion-killed: root-volume
mutant LF SHA256 root-nested: 618489fbae4e5b8155aa7790504a0057113216f2a41b36e0b64be052250320b9
assertion-killed: root-nested
mutant LF SHA256 root-equal: b0a8d8e7c7cc3a11172108d649b06c57e44fd011ac6d826a7ec3916d1fd55929
assertion-killed: root-equal
mutant LF SHA256 root-parent: e92ddba26191068c0539a6e0c8f0498f4a3f1831097d09143b1272279b51b327
assertion-killed: root-parent
mutant LF SHA256 root-pins: b4e0f5ce1ccb4eda6aa98518ef9deefb41935f68c1e0ced6532a2c392d1eb52c
assertion-killed: root-pins
mutant LF SHA256 root-helper: f40a12f44bdbd8435fd1d412b1c46dd00366d966f10cd55150928177887fcd0f
assertion-killed: root-helper
mutant LF SHA256 root-temp: a1b7f2b1099455f0194ae8445361d2883ee2f2c43425ec18d5564809f893c10d
assertion-killed: root-temp
mutant LF SHA256 root-wire: 087451c4c3ebec05eb979e8353e762c81f014cb7ffa359a3ad2022c26f5bc78d
assertion-killed: root-wire
mutant LF SHA256 binding-root-hash: 1f867eed8d467b68d4666f331e41c4e70059e784ef77f4cba080c51f247d288a
assertion-killed: binding-root-hash
mutant LF SHA256 binding-held-manifest: 84e61621c1d199c133248d2ce26af3747cc5a53d80d1b0ebeeb2b2142129e790
assertion-killed: binding-held-manifest
mutant LF SHA256 binding-git-bytes: d7ad9e843b838bec19a52e517ebfaa263f5d5fc7d0189459c8d02c3ce11b5779
assertion-killed: binding-git-bytes
mutant LF SHA256 binding-reviewed-blob: d2030f4b85c8758451a28ecd2bde4457419de861453d5a4e4b6f5c113e3d2c46
assertion-killed: binding-reviewed-blob
mutant LF SHA256 binding-git-frame: 0859388c43bae8748d19a2358f1d3f3b2823d155293f2732cea0e94b2b842e79
assertion-killed: binding-git-frame
mutant LF SHA256 binding-provenance-source: f256ca2096616bc7c17650d0a8c5e9071b53d79c57f5de442c5c4c129936c4b8
assertion-killed: binding-provenance-source
mutant LF SHA256 binding-provenance-commit: 4fd44c7cf46c242026cf1b6030d49e30ee1c0eb469f5b4a58f50d82dbe9d63b8
assertion-killed: binding-provenance-commit
mutant LF SHA256 binding-provenance-helper: abdab5363a549e0d8f295fbdabeed7c89b1a058f1a870a7831ff7a0d958e6c09
assertion-killed: binding-provenance-helper
mutant LF SHA256 binding-manifest-authority: 96ac5645691e46639aa6fea584c287793c14dfab6443ae2053206c47cead25d4
assertion-killed: binding-manifest-authority
mutant LF SHA256 binding-disposition-authority: 902d57acfd6fdc48b158e2989819caf6957671822bb83c7d651d86f19d9a42e5
assertion-killed: binding-disposition-authority
mutant LF SHA256 binding-provenance-authority: db9fb141795acb3388633d0b370b48090d07b54ffb8b57292363f0748924eb75
assertion-killed: binding-provenance-authority
mutant LF SHA256 binding-provenance-native: 35dbb1bc89519c10c70f0cc3b30fb45fb1dc346c890ee6689ba04306a4e5461f
assertion-killed: binding-provenance-native
mutant LF SHA256 binding-alias-path: 3d95aa698bca3a3193a6fbd41e032b218900dff72aeeabd8086b819154d179ad
assertion-killed: binding-alias-path
mutant LF SHA256 binding-lock-bytes: 13475f2bad2166a3eb456692e1abbafc32c505df0b0169e480708393b75e18fd
assertion-killed: binding-lock-bytes
mutant LF SHA256 binding-role-alias: e34b13ff8b5a4590852c6c48c964240b2d90c1ddc26ecdb650eb26d3c18bc7a7
assertion-killed: binding-role-alias
mutant LF SHA256 binding-qword-order: c3ee77218642f85969b74fd7feabdf85bfded8543efdc14174292bb56ac9e836
assertion-killed: binding-qword-order
mutant LF SHA256 binding-facts-copy: 77dc1ce6541fa24fcb0112bce16bb51ed83ea4f20ad3e800338da270469432a1
assertion-killed: binding-facts-copy
mutant LF SHA256 final-refusal: 43b7905b47aa8cf80b33ba5a9c08954677c23bfe98c1431e230cf27636736ae6
assertion-killed: final-refusal
unchanged LF source SHA256: 0941FA6FB8ADD01470E87E7084D753ACC8B967FFD76C2F0BC621BEE24CC1075A
unchanged LF held file SHA256: 9470B8F11A37AD22ECC7EB28926DE444D0C8D5CD6E25E64B9E0131459ADA1643
exact LF unchanged Controller.cs 0941fa6fb8add01470e87e7084d753acc8b967ffd76c2f0bc621bee24cc1075a
exact LF unchanged HeldFiles.cs 9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643
exact LF unchanged NativeJob.cs f934f165408a9a879090da129f95122a75d2bcd5831ac5bb60cb1063f39a36d4
exact LF unchanged Admission.cs b6c1aa6b39d618a85e2183988cc95b44c965dfbb44670b2c2c4382d336754289
exact LF unchanged Test-PureController.ps1 2e9b07ff121769509e65797c285f262cb255938d0a4f17b643b900c46d17cb30
```
