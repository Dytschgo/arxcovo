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
