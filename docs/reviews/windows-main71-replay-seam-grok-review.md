# Windows main71 replay seam correction: Grok review

Disposition: **no source blocker.** Security, standards, and spec are separate
below. The cfg(test) SP1 and SP2 seams match the accepted contract and the
source-review defects they were written to close. This review grants no
publication, merge, CI, root, native replay, or run authority.

Reviewer: xAI / grok-4.7 / high, one independent pass. The assignment forbids
subagents, so both review axes and the security check are this single reviewer.
Independence is from the Sol implementation, not from a second sub-reviewer.
Debug phases 2–6 are skipped: there is still no authorized red-capable native
loop, and this assignment forbids a native repro, a historical retry, and
product edits. No cause and no fix of the main71 incident are claimed.

## Pins

| Item | Identity |
| --- | --- |
| Review cwd | Isolated linked worktree. Git top level equals the review cwd. Status was empty before this commit. `<isolated-worktree>` |
| Governing HEAD before this review | `203d6674c9be57282d173616f1ce834d29bd03c0` on `t3/review-windows-replay-seam` |
| Merged PR73 base | `589febbdf29cf8e2177b42e77d854cdd642e035f`. `git merge-base --is-ancestor` succeeded |
| Prepared parent | `928afb2ac6a3aa37bac591daaff22f9739321f00`, ancestor of HEAD |
| Accepted proposal at HEAD | `docs/reviews/windows-main71-feedback-proposal.md` blob `eaf4e7afa87eebb12a2e6fe3a9c14923adb95e0e` |
| Source Grok review read | `docs/reviews/windows-main71-diagnostic-grok-source-review.md` blob `3a8ec17558bd8070cb83d55652adf34def82c26d` |
| Baseline Windows blob | `0f61e58a92177d78d4aad4fc0e03292b90cfb554` at `589febb`, `928afb2`, and `af2deeaa90708fffd36139f7a5f50b6db1b1c674` |
| Sol source | `a65e784b2a0c6a3401382d2f87087dde974fd5ea`. Not an ancestor of HEAD. Tree `a0138f3c09820ce38c46149359da5de6d93baf7c` |
| Retained source | `d84a39c938d22ff1950c2d3e4f3339cda48e7b31`. Same tree as `a65e784` |
| Corrected Windows blob | `4cb039f4742568a3eb3a124ae3626ac9511b8be2` at `d84a39c` and at HEAD. 75184 LF bytes, 2050 lines |
| Corrected LF SHA256 | `e2c91088e83ac15984219ea4581f5effe77c0e2089a9222fa1bb4eb764e1433a` |
| Code diff | `crates/arxd/src/development/windows.rs` only: 142 insertions, 9 deletions |
| Sol evidence | `77b1a3bf44f2dbff52d89ca9638f2c9e96b1f3b3`. Not an ancestor of HEAD. Tree `65581733c1a250e3d4306184828d5af7f72edcd4` |
| Retained evidence | `e0180b18977b579a73dd2c8297bc26a02eccaae3`. Same tree as `77b1a3b`. Evidence blob `c703a953f767f20e6dc8c4a680dd0460942ab6dc` |
| Coordinator tip | `203d667` redacts that evidence path and adds the progress bullet. Tip evidence blob `d448ea6ca383ae63e49d35314cbdeac8ab4ce991` |
| Lockfile | `Cargo.lock` blob `1c5c0fcf2b577f89ed998826ddf6608338b4b523`, unchanged |
| Toolchain this review | rustc 1.95.0 `59807616e1fa2540724bfbac14d7976d7e4a3860`, 2026-04-14, host `x86_64-pc-windows-msvc`, LLVM 22.1.2. cargo 1.95.0 `f2d3ce0bd7f24a49f8f72d9000448f8838c4e850` |

`a65e784` and `77b1a3b` are retained by equal trees after the rewrite onto
`928afb2`. They are not ancestors. The code diff against blob `0f61e58` is
the cfg(test) `stage!` body, `Observation` / `observe`, `capture_injected`,
two pure tests, and the cfg(test) peek block in `drain`.

## Standards

**PASS. No finding.**

Docs read: `AGENTS.md`, `CONTRIBUTING.md`, and the Review skill. The source
commit is one conventional test concern in the file the contract already uses.
The two new tests exercise the seam they add. The spec forbids a native site
test here, so the absent native test is not a standards breach. No credential
and no owner path were added to the Rust diff. The tip evidence file and
`docs/PROGRESS.md` use `<isolated-worktree>` rather than an absolute checkout
path.

Fowler baseline: no smell is filed. The 28-row oracle repeats `Stage::ALL`
order and then asserts that equality, so it is the check rather than a second
untested copy. `windows.rs` remains the required home of this module.

## Spec

**PASS. SP1 and SP2 are closed as dormant source seams. No new finding. Native
replay stays unauthorized.**

Checked against proposal blob `eaf4e7af` and the source review's SP1 and SP2.
Proposal P1: "Attr-size may lack a valid failure error: record last-error
observation with validity, not a purported cause. Wait records raw wait_ret
and error-valid only for WAIT_FAILED." P1 also preserves 109 as success.
P4: "Pure module tests can validate counter bookkeeping, but do not earn
native zero-call evidence." The source review required a scope that is still
alive when `create_calls` is read, and injection metadata that matches the
actual attr-size and wait captures.

SP1, inspected and executed through the pure counter test. `observe` enters
one `Scope`, runs the closure, then copies `Observation { record, create_calls }`
before that scope drops. `Scope::drop` still replaces all TLS with
`State::default()`, including when the closure unwinds. `Invocation::drop`
while scoped clears the running flag and the armed stage, and leaves `calls`
and `captured` in place. The pure test increments twice inside the closure,
observes 2 after that invocation drops, checks the copied stage and error 55,
then requires TLS counter 0 and a missing record. A second `observe` is empty.
A caught synthetic panic also leaves TLS clear. `run_ok` is unchanged: it
still owns a separate scope and drops it before its caller resumes.
`Scope::enter` still asserts `!scoped`. A future replay arms and calls
`run_inner` directly inside the `observe` closure. Nested `run_ok` hits that
assert. This test calls `create_call` directly. It is counter bookkeeping.

SP2, inspected, with the helper executed by the pure oracle. `capture_injected`
returns before `capture` for error 109 on `LoopOutPeek`, `LoopErrPeek`,
`FinalOutPeek`, and `FinalErrPeek`. AttrSize stores `valid=false` and no wait.
Wait stores `valid=true` and raw `u32::MAX`. Every other stage stores
`valid=true` and no wait. The 28-row oracle, locked to `Stage::ALL`, checks
errors 55 and 109, one-shot consumption, peek-109 success with no record, and
failure for 109 on every non-peek stage. Its `create_calls == 0` only shows
that this closure never increments the counter.

windows-sys 0.61.2, read as source and not called, defines
`ERROR_BROKEN_PIPE = 109`, `WAIT_FAILED = 4294967295`, and
`WAIT_TIMEOUT = 258`. The synthetic literals match those definitions. The
cfg(test) `stage!` body and the cfg(test) peek block both call the helper
before `diagnostic_snapshot` and before `Err`. Peek 109 in `drain` returns
`Ok` before `PeekNamedPipe`. The native `PeekNamedPipe` failure branch,
attr-size `diagnostic_failure(false, None)`, wait
`diagnostic_failure(wait == WAIT_FAILED, Some(wait))`, assign capture,
`TerminateProcess`, cleanup, `timeout_s`, cancel, shutdown, and quarantine
are outside the diff.

The pure oracle calls the helper directly. It does not expand `stage!` or
enter `drain`. Call-site wiring is source inspection. The seven mutant edits
below are unique and change an asserted value. None is an equivalent. This
review recomputed their LF SHA256 from blob `4cb039f4` in memory and did not
execute them:

| Mutant | Recomputed LF SHA256 |
| --- | --- |
| `drop(_scope)` before the copy | `f063fe26b752bd52118a569fa7954974ee3bd7e8e0ec50f7ee3f36b2f8e1c038` |
| `s.calls = 0` on scoped invocation drop | `9b4866f45f835a5fb50a59687167bce5d55e944b3944f4e794fb44568f8a08dd` |
| validity forced true | `f0dd5462882149c40fe5fa12abf47d66a2d6189f1cbff9c66cdd2475883af539` |
| wait raw `None` | `16ac29462dab7ec8542232511bcb7788b5be2425e76b68977744795ae0628caf` |
| wait raw 258 | `bff0016cd55589c2acd8197dc3a652db8470f7ab6979ef5d0cb5d5da01f747dd` |
| Wait validity forced false | `b27066e23f9d8fd2e94e61fd345aa2c93231b55d53dfb6c4db97f80f25d0503f` |
| helper compares 110 | `6945f0fe1ccbe5936f3ee07a1213a7b7236b2f4006969f29635945e044a339f9` |

The scope-drop edit also makes the metadata test unwrap a missing record.
That panic is not separate metadata credit. The counter assertion is the kill.
Builder execution of these blobs remains the evidence in
`docs/evidence/windows-main71-replay-seam-correction.md`. Restored source
matches `e2c91088…`.

Non-test `diagnostic!` and `stage!` still expand to an empty block, so their
arguments are not evaluated. Non-test `drain_at!` still emits only its call.
Those three bodies are outside the diff. The diag module remains `cfg(test)`.
No Cargo, workflow, native test module, or discovery filter changed. The two
new tests sit in the existing pure module.

## Security

**PASS. No source finding.** One publication note, not a code blocker.

Release and non-test library code gain no diagnostic module, label, control
string, TLS, formatter, or executable diagnostic logic. The controlling
evidence is the unchanged non-test macro bodies and the `cfg(test)` module.
This review did not rebuild LLVM IR and makes no binary-equality claim.

The Rust diff adds no success-path native call, handle, clock, token read, or
path. Failure snapshots and `GetLastError` remain on the pre-existing
cfg(test) failure path. Injection that returns `Err` still does so before the
API, and peek 109 returns success before `PeekNamedPipe`. Job limits, cancel
and shutdown order, quarantine, and the timeout comparisons are the baseline
statements.

The tip evidence file is redacted to `<isolated-worktree>`. Evidence blob
`c703a95` in commit `e0180b1` still stores one absolute owner worktree path.
This review does not quote it. A squash of HEAD publishes the redacted tip.
Pushing that parent commit publishes the blob. `docs/PROGRESS.md` at HEAD
does not contain the path.

## Tests this review ran

Transitive inspection came first. The six pure tests reach `Stage`, `Context`,
`Role`, `Op`, `resolve`, `Scope`, `Invocation`, `observe`, `arm`, `stage`,
`capture_injected`, `capture`, `failure`, `create_call`, `create_calls`,
`record`, `line`, and `Reason::as_str`. Closures pass integers, a `Cell`,
formatting, or `panic`. `failure` in the ordering test sets that `Cell`. No
pure callee reaches `diagnostic_snapshot`, `GetLastError`, `Instant`,
`run_ok`, `run_inner`, `unsafe`, or a process launch.

Then only:

`cargo test -p arxd --lib --locked --offline --target x86_64-pc-windows-msvc development::windows::diag::pure::`

Result: **6 passed, 0 failed, 356 filtered out, harness 0.00 s, compile
54.04 s.** Names: `independent_mapping_and_module_mutants`,
`scoped_observation_survives_invocation_but_clears_tls`,
`injected_metadata_all_stages_and_broken_pipe`,
`lifecycle_one_shot_unwind_and_counter`,
`error_reader_precedes_mock_cleanup_and_preserves_first`,
`complete_format_bounds_indices_validity_and_privacy`.

Fmt and package all-target Clippy were not re-executed. They remain the
builder's recorded evidence for this blob. Mutant processes were not
re-executed.

## Limits

- The pure filter does not reach Win32, prove marker reachability, real-site
  deletion or mislabel mutations, native last-error order, a zero
  `CreateProcessW` count, never-resume behavior, cleanup equivalence, timing,
  or the historical failure.
- `create_calls == 0` in the metadata oracle is an idle counter. It is not
  the SP1 proof and not native zero-call evidence. The SP1 proof is the
  copied counter 2.
- Removing the helper call from `stage!` or from the peek block is not one of
  the seven mutants and was not executed.
- Debug phase 1 for the main `ExecutionFailed` incident remains open. No
  focused native loop was built. M2 is incomplete. Real-Runner PA6 replay
  remains open. Native, installed, and dedicated paths are **unverified on
  real hardware**.
- Replay authorization is a separate decision. This correction is reversible:
  one cfg(test) file, no migration. Revert does not by itself authorize or
  cancel a native run.

Not run: full suite, the other Windows tests as tests, fmt, Clippy, CLI,
fixtures, `run_inner`, replay, owner, account, service, VM, private state,
install, network, GitHub, and CI. No product source was edited.

## Close

Standards: 0 findings. Spec: 0 findings. Security: 0 source findings; the
publication note is the unrebased evidence blob `c703a95`. Ready for
coordinator normal PR gates only. Not native authority.
