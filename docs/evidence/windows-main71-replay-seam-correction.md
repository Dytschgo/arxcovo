# Windows main71 dormant replay seam correction

Provider: OpenAI / gpt-6.1-sol / high. Date: 2026-11-14.
Prepared for fresh independent Grok or Muse source review and normal PR gates.
No independent review, publication, hosted checks or merge was performed here.

## Pins and authority

Initial clean HEAD: `928afb2ac6a3aa37bac591daaff22f9739321f00`.
Top level matched the supplied isolated cwd:
`<isolated-worktree>`.
`git status --short` was empty. `git rev-parse --git-dir --git-common-dir`
identified a separate linked-worktree Git directory and common repository.
`git merge-base --is-ancestor 589febb HEAD` returned 0. Merged PR73 base:
`589febbdf29cf8e2177b42e77d854cdd642e035f`.
Initial source blob: `0f61e58a92177d78d4aad4fc0e03292b90cfb554`.

Read AGENTS, KICKOFF, accepted Windows proposal (blob
`eaf4e7afa87eebb12a2e6fe3a9c14923adb95e0e`), diagnostic Grok source
review including SP1/SP2, prior pure-boundary recheck and implementation
evidence, and current PROGRESS checkpoint. Local exact-head comment evidence
is PROGRESS's record of Grok comment `6871360901` on reviewed publication
`af2deeaa90708fffd36139f7a5f50b6db1b1c674`. Reading that commit's Windows
source and hashing it produced the same `0f61e58a` blob. The external comment
body was not fetched; this is local recorded evidence, not fresh GitHub
verification or an independent review of this correction.

Source commit: `a65e784b2a0c6a3401382d2f87087dde974fd5ea`.
Source blob: `4cb039f4742568a3eb3a124ae3626ac9511b8be2`.
Source LF-byte SHA256:
`e2c91088e83ac15984219ea4581f5effe77c0e2089a9222fa1bb4eb764e1433a`.
Only `crates/arxd/src/development/windows.rs` changed in that commit.
The following evidence commit changes only this document. No PROGRESS, ADR,
proposal, review, workflow, CI, Cargo or policy file was edited.

## Correction and pure credit

SP1: `diag::observe` owns a Scope, calls a synchronous closure, then copies
fixed typed `Observation { record, create_calls }` before the Scope drops.
Scope drop still clears all TLS, including on unwind. Invocation drop still
preserves scoped results until the owning Scope exits. Future separately
authorized replay must arm and call directly inside this closure, or retain an
outer Scope directly; it must not call `run_ok`, whose non-nestable scope and
capture behavior are unchanged. No stale TLS is retained outside its scope.
The new pure test increments twice inside the closure, observes 2 after
Invocation drops, checks the copied record, then requires TLS counter 0 and
record missing. It also checks a fresh invocation and unwind cleanup. This is
counter bookkeeping evidence only, not native zero-CreateProcessW evidence.

SP2: `capture_injected` centralizes synthetic metadata from closed Stage.
AttrSize records an invalid last-error observation. Wait exclusively models
synthetic WAIT_FAILED (`0xffffffff`), with valid last-error and matching raw
wait; it does not model other native wait returns. All other injected failures
have valid error and no wait. Four Peek stages with error 109 return success
without capture; 109 at other stages remains a failure. Both test-only stage
macro and Peek branch use the helper before their existing failure snapshots.
No actual-failure branch, cleanup or API was added or changed.

An explicit 28-row oracle asserts exact stage/error/validity/raw-wait metadata
for errors 55 and 109, including one-shot consumption and Peek success. The
original four pure tests remain, covering label mapping, lifecycle, mock
error-reader ordering, formatting and bounds. These tests validate dormant
module behavior, not real-site reachability or actual API error ordering.

Before execution, inspected all six pure bodies and their transitive helpers:
Stage/Context labels, resolve, Scope/Invocation entry/drop, observe, arm,
stage, capture_injected, create_call/create_calls, capture/failure, record,
line/n and Reason::as_str. Their supplied closures use in-memory values,
Cell, TLS, formatting and caught synthetic panics. None reaches Win32,
diagnostic_snapshot, run_ok, run_inner, a process or clock. The exact filter
below is the only Rust test selection executed, including mutant runs.

## Commands and actual outcomes

Pinned toolchain: rustc 1.95.0, commit
`59807616e1fa2540724bfbac14d7976d7e4a3860`, LLVM 22.1.2, host and target
`x86_64-pc-windows-msvc`; cargo 1.95.0 (`f2d3ce0bd`). Default features,
default test/dev profiles. Unchanged Cargo.lock blob:
`1c5c0fcf2b577f89ed998826ddf6608338b4b523`.

- `cargo fmt --all`: applied formatting once.
- `cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure::`:
  passed 6/6, 356 filtered out, harness 0.00s; cold build 57.60s.
- Same exact command on each of seven source mutants below: compiled, exit 101,
  intended assertion failure observed. No compiler error counted as a kill.
- Restored exact source bytes, then repeated that exact command: passed 6/6,
  356 filtered out, harness 0.00s; build 4.58s. This final tested blob equals
  the committed source blob above; no source edit followed.
- `cargo clippy -p arxd --all-targets --locked --offline --target x86_64-pc-windows-msvc -- -D warnings`:
  passed, 27.90s. Compiles package targets including non-test library and test
  targets; executes no application or test. No separate cargo check claimed.
- Final `cargo fmt --all -- --check` and `git diff --check`: passed.
- Static byte/string comparisons against initial source confirmed unchanged
  run_ok, native failure/snapshot helpers, all non-test macro bodies and the
  entire ordinary native tests module. Reviewed remaining diff: cfg(test)
  helper/module/pure tests and test-only injection wiring only.

Release `diagnostic!` and `stage!` still discard all arguments; `drain_at!`
emits only its original call. The entire diag module remains cfg(test).
No new success-path native API/handle or diagnostic clock, release diagnostic
logic or strings, native test discovery, timeout, cancel, quarantine, token,
path or Job behavior is introduced. This is source exclusion evidence; no
new IR artifact or binary-equivalence claim is made.

## Assertion-killed source mutants

A transient Python driver kept the LF source bytes in memory, applied one
unique replacement at a time, and ran the exact Cargo pure filter above with
a 120s subprocess timeout. It required exit 101, `test result: FAILED`, an
assertion failure, and absence of `could not compile`; it restored the bytes
after each case and in finally. Each mutation is independently applied to the
pinned corrected source; none changes a native path or a test assertion.

| Mutant | Replacement / observed assertion | Seconds |
| --- | --- | --- |
| scope_drop_before_copy | Insert `drop(_scope);` before Observation creation; counter 0 != 2 | 7.25 |
| invocation_zeroes_counter | Insert `s.calls = 0;` before scoped `s.running = false`; counter 0 != 2 | 7.25 |
| attr_size_valid_true | Replace `stage != Stage::AttrSize` with `true`; AttrSize valid true != false | 7.50 |
| wait_raw_missing | Replace `(stage == Stage::Wait).then_some(u32::MAX)` with `None`; Wait None != Some(4294967295) | 5.17 |
| wait_raw_timeout | Replace same expression with `.then_some(258)`; Wait 258 != 4294967295 | 5.52 |
| wait_valid_false | Add `&& stage != Stage::Wait` to validity; Wait false != true | 5.67 |
| peek_109_failure | Replace helper's `error == 109` with `error == 110`; failure true != false for Peek 109 | 5.72 |

Mutant source SHA256, in table order:

1. `f063fe26b752bd52118a569fa7954974ee3bd7e8e0ec50f7ee3f36b2f8e1c038`
2. `9b4866f45f835a5fb50a59687167bce5d55e944b3944f4e794fb44568f8a08dd`
3. `f0dd5462882149c40fe5fa12abf47d66a2d6189f1cbff9c66cdd2475883af539`
4. `16ac29462dab7ec8542232511bcb7788b5be2425e76b68977744795ae0628caf`
5. `bff0016cd55589c2acd8197dc3a652db8470f7ab6979ef5d0cb5d5da01f747dd`
6. `b27066e23f9d8fd2e94e61fd345aa2c93231b55d53dfb6c4db97f80f25d0503f`
7. `6945f0fe1ccbe5936f3ee07a1213a7b7236b2f4006969f29635945e044a339f9`

First mutant also made the metadata test unwrap a missing record; its kill
credit is specifically the counter equality assertion, not that unwrap.
Other mutants each failed their intended equality assertion (5 passed/1
failed). Scope-drop mutant: 4 passed/2 failed. All kept 356 filtered out and
0.00s harness time. Restored exact LF SHA256 equals the source pin above.

No failed implementation fixes occurred. Expected mutant assertion failures
are negative-control evidence, not failed fix attempts. Two read-only rg
commands initially used unsupported Windows globs and were corrected with
directory searches; no source write or test selection resulted from those
search errors. No compile, lint or baseline-test failure occurred.

## Remaining limits and handoff

Native replay still needs separate bounded authorization. Fresh independent
Grok or Muse review and normal required PR gates remain outstanding; no new
SHA, comment, green historical CI or this document grants run authority.
Main71 ExecutionFailed has unknown cause and no historical reproduction or
retry. Debug Phase 1 has no focused native feedback loop. M2 is incomplete;
real-Runner PA6 replay remains OPEN. Native, installed and dedicated paths
are **unverified on real hardware**.

No native capture, native counter observation, zero native create calls,
never-resume behavior, real marker reachability, actual error ordering,
cleanup equivalence, timing/budget acceptance or historical cause is proved.
No Rust full suite, ordinary native existing test, native replay, fixture,
CLI, run_inner, account, service, VM, S4U, owner/private-state access,
installation, network, GitHub, push, PR, dispatch, subagent or babysitter
operation occurred. Old Lab Manager and agent-windows-test were not accessed.
Build tools and the explicitly allowed pure harness were the only execution
in build/test categories. No application/native API execution was requested
or performed by this task.
