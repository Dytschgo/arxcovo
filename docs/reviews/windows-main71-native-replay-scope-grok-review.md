# Windows main71 native replay scope: Grok review

Disposition: **CHANGES REQUIRED.** Standards pass. Spec and security each
have blocking text gaps. The parent-writer fixture is the deterministic stream
seam, and it is not cleared for code. This review grants no implementation,
build, native execution, ordinary discovery, publication, CI, root, S4U, or
run authority.

Reviewer: xAI / grok-4.7 / high, one independent pass, 2026-11-14. The
assignment forbids subagents, so standards, spec, and security are this single
reviewer. Independence is from the Sol proposal, not from a second reviewer.

Debug phase 1 for historical main71 was not run. No red-capable command for
that `ExecutionFailed` was executed. Phases 2–6 are not entered. No cause and
no fix are claimed. The 28-label matrix, after the corrections below, would be
a reachability loop for marker consumption. Chosen error 55 leaves the
historical cause unknown.

## Pins

| Item | Identity |
| --- | --- |
| Review cwd | Linked worktree. Git top level equals the review cwd. Porcelain was empty. Per-worktree Git dir under the common Git dir. `<isolated-worktree>` |
| Branch | `t3/review-windows-native-replay` |
| Requested proposal | `d863c1019db87c666b285d3fda7006c1464f5f81`, parent `c9e1e772da7a4d248ebdc856d52e8081bbc471b9` |
| Checkout HEAD before this review | `88986fa5bd639bf686bde6de4aa3f37ae94abaeb`, same parent and same tree `8c74dac03eec0fd2c021de3880c2cdc67cb02aae` |
| Proposal blob | `docs/reviews/windows-main71-native-replay-scope-proposal.md` `4e5f8eeb3a91f711c64bd1b04312ee9f7be7993f` at both commits |
| Merged PR74 | `aa49ce1e99475d07d63c6b0d23fa94bdb30bab74`. `git merge-base --is-ancestor` succeeded |
| Windows blob | `4cb039f4742568a3eb3a124ae3626ac9511b8be2` at `aa49ce1`, `c9e1e772`, `d863c101`, and HEAD. 75184 LF bytes, 2050 lines, CR count 0 |
| Windows LF SHA256 | `e2c91088e83ac15984219ea4581f5effe77c0e2089a9222fa1bb4eb764e1433a` |
| Historical source | `a9ec0acdefcd2dc60eed34f095869a045cf8e356`, Windows blob `85f0c0eca19950fc65a02b621ff752eb6ba61551` |
| Accepted proposal | `eaf4e7afa87eebb12a2e6fe3a9c14923adb95e0e` |
| Grok source review | `3a8ec17558bd8070cb83d55652adf34def82c26d` |
| Grok SP1/SP2 review | `52ae5461b93a7ceb699ad8285c862acf2eb87783` |
| Pure-boundary recheck | `cc7cee3887ad70f7e6a796bb581868d1797c0b6f` |
| Progress / lockfile / toolchain file / CI workflow | `b560554b4ebe0798fde89c4806a9dc9a61d98ac7`, `1c5c0fcf2b577f89ed998826ddf6608338b4b523`, `3417ee590341edc47a046d94e77ee9bee2b33bf4`, `fc065af9786e75c52cfb7668b726d0a89931cd5b` |

`d863c101` and `88986fa5` differ as commits and match as trees. The reviewed
text is blob `4e5f8eeb`. Workspace members are `crates/arxd` and
`crates/arx-guest`. Verify job `timeout-minutes` is 12. Toolchain file channel
is `1.95.0`. This review did not execute `rustc` or Cargo.

Docs read: `AGENTS.md`, full `docs/KICKOFF.md`, `CONTRIBUTING.md`,
`SECURITY.md`, ADRs 0010 and 0013, the accepted Windows proposal, the Sol and
Opus contract reviews, both Grok source reviews, the pure-boundary recheck,
progress, CI, and `crates/arxd/src/development/windows.rs` at the pin above.
The Review and Debug skills were read, including their active profiles. The
task restriction overrides kickoff delegation, babysitter, and progress
writing. No babysitter operation was performed.

## Standards

**PASS. No finding.**

The proposal is one docs concern. It labels itself proposed, records M2
incomplete, leaves real-Runner PA6 open, and marks native paths unverified on
real hardware. It does not claim an unrun test, a measured budget, a
reproduction, or a hardware pass. Repo-relative pins and sanitized placeholders
only. No credential or owner path was added. The assignment's ban on editing
`docs/PROGRESS.md` accounts for the absent progress bullet.

Fowler baseline: no smell is filed. There is no code diff in the reviewed
proposal.

## Spec

**CHANGES REQUIRED. Two blockers.** The route table, counts, stream
short-circuit, and budget arithmetic match the source. The oracle and the new
counters do not yet survive the rules the proposal sets for them.

Checked, not blocking:

- Historical blob `85f0c0e` lines 64, 89, 221, 231, 240, 286, 289, 324, 329,
  414, 420, 423, 430, 448, 471, 478, 508, and 512 are the accepted origins.
  Line 221 is the null-job failure return. Line 324 is the peek failure
  return. Line 471 is the assign failure return after the inner
  `TerminateProcess`. Line 478 is the resume failure return. The current file
  keeps those APIs and inserts `stage!` ahead of them.
- `Stage::ALL` and the pure manifest are 18 origins and 28 labels. The pure
  module holds six tests. They do not call `run_inner`. They are not native
  proof. SP1 and SP2 remain closed dormant seams: `observe` copies
  `create_calls` before `Scope` drops, and `capture_injected` stores
  attr-size `valid=false`, wait raw `u32::MAX`, and peek 109 as success with
  no record.
- Create counter: `stage!(CreateProcess)` returns before `create_call()` at
  line 1352, which is the only product increment and sits before the only
  `CreateProcessW`. Injected `create_process` observes 0. Every later row that
  reaches the API observes 1. There is one `ResumeThread`, at line 1388, after
  assign and the identity/policy check, and only if `stage!(Resume)` did not
  return.
- Child inventory for error 55: 14 rows at or before create launch nothing; 4
  rows (`assign_job`, `child_token_open`, `child_token_info`, `resume`) create
  one suspended child and must observe resume attempts 0; 10 rows resume once.
  28 cases and 14 fixture children. Plus four peek-109 rows and two controls:
  35 cases, 20 children, 16 resumes. The kernel row adds one case and no
  child. `ExpectedCases 35` and `ExpectedChildren 20` match. Harness processes
  are outside the 20.
- Loop stderr uses `||` at the current drain pair. One byte is below
  `output_bytes` 8192, so the out drain returns false and the err drain runs.
  Final drain uses `|`, so an out overflow still evaluates err; an out `Err`
  still skips err through `?`. Final-err rows depend on the out drain
  succeeding. Read stages run only when the real peek reports `available > 0`,
  so reaching a read label is the availability proof. Peek labels run even
  when the pipe is empty. Peek 109 returns success before `PeekNamedPipe` and
  can leave the byte for a later pass.
- `touch` writes stdout only. `echo` can lose the race into the loop drain, and
  bytes consumed there are gone before final drain. Parent prefill is the seam
  that removes scheduler timing from stream reachability. Wait is armed on the
  first loop pass, before the 1 second request timeout. `exit_code` is reached
  after a normal break or after that timeout, and it is before `job.stop`.
  Final labels require `job.stop() == true`. A stop failure is
  `CleanupUnconfirmed` and blocks the row.
- Worst final-drain failure: the loop can spend the 1 second request timeout,
  the inner `job.stop` can spend 2 seconds, and the outer error `job.stop` can
  spend another 2 seconds. That is 5 seconds. The outer call ignores its
  result. Rows that fail before the inner stop pay one 2 second stop. The 10
  second row cap covers the 5 second path and leaves 5 seconds for setup,
  writes, queries, and teardown. 35 times 10 seconds is 350; the 360 second
  envelope leaves 10 seconds outside the row caps. 300 + 360 + 60 = 720, the
  12 minute job. The 15 minute wall is 900. The 300 second non-replay share is
  unmeasured. Ordinary-discovery admission stays blocked. The recorded 232
  second job with 307 unrun tests, and PR74's recorded 468 second PR CI, remain
  historical local notes. This review did not remeasure them and does not treat
  them as headroom.
- Error 55 metadata matches `capture_injected`: `valid=true` except attr-size,
  wait missing except raw `4294967295` with `valid=true`. `ms` stays missing.
  Token-info still merges four classes. The first `info` call is the labeled
  one; its size probe is an earlier unlabeled call. Kernel `drain` on a null
  handle stores a record only when `Invocation` is running. The proposal's
  order, observe then Invocation then LoopOut role, matches `Invocation::enter`
  clearing role. Expected error 6 was not executed here.
- Gates 2, 3, and 4 keep preparation, native run, and ordinary discovery
  separate. A new SHA or test name confers no execution authority. No
  `workflow_dispatch` and no historical rerun are proposed. Zero selected tests
  fail the future driver. Unknown cleanup stops the matrix and forbids reuse,
  broad kill, and broad delete.

**SP-ORACLE. Blocking.** The published 28-row table has label, route, and
create count. It does not have pass, stdout total, or stderr total.
`drain_at!` stores those fields at the start of `drain`, and capture copies
that stored tuple. A read failure therefore records the pre-read totals. The
same document requires a literal table and forbids building it from the
captured record. "Where applicable" can be satisfied by echoing the capture.

Source inspection of blob `4cb039f4` fixes the independent expectations.
Missing means `None`, not zero. These values were not executed.

| Rows | pass | stdout total | stderr total |
| --- | --- | --- | --- |
| At or before resume, all 18 non-loop rows through `resume` | missing | missing | missing |
| `wait`, `exit_code`, with no prefill | missing | 0 | 0 |
| `loop_out_peek`, `loop_out_read` | missing | 0 | 0 |
| `loop_err_peek`, `loop_err_read` | missing | 1 | 0 |
| `final_out_peek`, `final_out_read` | 0 | 0 | 0 |
| `final_err_peek`, `final_err_read` | 0 | 1 | 0 |

Prefill belongs only on the eight stream error-55 rows, the four peek-109
rows, and the two controls. `wait`, `exit_code`, and every pre-resume row stay
empty. The payload byte is ASCII `X` (`0x58`). Controls assert the returned
stdout and stderr strings equal `X`, numeric totals 1/1, and dropped totals 0.
That byte survives UTF-8 and the existing redaction needles.

**SP-COUNTER. Blocking.** `observe` copies state after the closure returns.
`run_inner` drops `Invocation` before that copy. `Invocation::drop` clears
`armed` and leaves `calls` and `captured`. A new consumption or resume counter
that the drop clears is 0 at the copy. Empty `armed` remains non-evidence, as
the proposal says.

The counters have to live in `State` fields that `Invocation::drop` leaves
intact, and `observe` has to copy them with `create_calls`. The resume
increment has to sit after `stage!(Resume)` returns and before the
`ResumeThread` call, so the injected resume row measures 0. Consumption counts
successful `armed.take()` inside `stage`. Peek 109 takes that arm and then
`capture_injected` returns false without a failure record. Counting stored
records would report consumed 0 on a 109 success.

## Security

**CHANGES REQUIRED. Three blockers.** Token, path, Job limit, cancel, and
shutdown checks stay on the `run_inner(..., false)` path. The text still
allows the wrong image, a duplicate taken after the writers are gone, and a
second direct call on a quarantined `Execution`.

Checked, not blocking:

- `CREATE_SUSPENDED` is unchanged. Resume stays after assign and the
  child/daemon user comparison, and after policy when the fixture flag is
  false. The proposal keeps that flag false and refuses `policy_fixture` and
  any elevated workaround. ADR 0010's refusal of service, elevated, non-medium,
  and enabled-admin tokens still applies. ADR 0013 does not alter this
  executor.
- Assign injection returns before `AssignProcessToJobObject`, so the inner
  `TerminateProcess` does not run. The outer error path at lines 1491–1495
  still terminates, calls `job.stop`, and sets `quarantined`. Native assign
  failure still captures before its inner terminate and snapshots after it.
  Injected assign snapshots before the outer terminate. Those observations
  stay distinct.
- Post-create `Err` sets the quarantine flag. Pre-create `?` returns before
  that store. Job kill-on-close, process and memory limits, UI restrictions,
  and the two-second stop clock are the current statements. Cancel and
  shutdown are still checked in the loop before wait. No S4U, owner
  administration, breakaway, or fence disable is proposed. Nested-job
  incompatibility blocks native execution rather than relaxing either job.
  This review does not claim the current limit flags nest.
- The extra parent writers are outside the dormant capture contract. As a
  future fixture-only exception they are acceptable in principle: non-inherited,
  absent from the child handle list, closed with the function, and absent from
  non-fixture and non-test builds. They change EOF for fixture cases. That
  limitation stays a reported limit and is not ordinary EOF proof. No
  implementation of the exception is authorized.
- Unknown cleanup, failed queries, timeout, unexpected create/resume, and
  containment failure stop the matrix, keep the sanitized record, and retire
  the disposable environment through its controller. No kill-by-name, PID
  guess, process enumeration, or glob delete. Irreversible administration
  stays owner-only. The kernel row must not close the null handle.
- Reports stay on the fixed diagnostic line, `ms=-`, with new counters in a
  separate numeric record. No account name, SID, profile path, or token dump.
  The old Lab Manager tree and Hyper-V VM `agent-windows-test` stay untouched.

**SE-IMAGE. Blocking.** `resolve` returns the first `PATH` entry's
`<tool>.exe`. The request tool is `cargo` because that name is on the allowlist.
The proposal copies a helper to `cargo.exe` and does not require `PATH` to be
exactly one fixture-owned directory, does not require the resolved file to be
the hashed helper, and does not pin empty arguments. An ambient `cargo.exe`
can run and can spawn descendants. The one-create and no-descendant counts
then fail open.

The helper manifest also has to stay outside workspace members. Adding it
would place it on `cargo clippy --workspace` and `cargo nextest run
--workspace`. Current members are only the two crates above.

Required text: `PATH` is one checked reparse-free directory containing only
the pinned helper named `cargo.exe`; the resolved path equals that hashed file
before `CreateProcessW`; arguments are empty; the child environment has no
action variable; `run_inner` is called with false; `policy_fixture` stays
false; the helper is not a workspace member and has no tests.

**SE-DUP. Blocking.** The parent stdout and stderr writers are dropped at
lines 1371–1372, immediately after successful `CreateProcessW` and before
assign. A duplicate taken after that drop has no handle left, and the stream
rows fall back to child timing. The duplicate must be taken after the inherit
list at line 1315 is sealed, it must be non-inheritable and absent from that
list, and it must outlive the original drop. It closes when `run_inner`
returns, including `?` and unwind, and before temp removal. A failed duplicate
is an earlier unexpected failure and stops the row.

**SE-REUSE. Blocking.** `run_inner` stores `quarantined` at line 1494 and does
not read it. `Execution::work` reads it and returns `Quarantined`. A second
direct `run_inner` on that same value starts another child. One fresh
`Execution` per row is the reuse fence. The product latch does not cover this
seam. The text has to say that, because "post-create failure quarantines it"
describes `work`, which this matrix does not call.

## Local code without native execution

Non-discovered helper source and `cfg(test)` functions that no harness names
can be written without launching a child. That writing is a later coordinator
decision under gate 2, and this review does not make it. `#[test]`
registration, the two native Cargo filters, and `Invoke-BoundedReplay.ps1`
execute the matrix as soon as an ordinary test command can see them.
Registration is not execution authority. The six pure tests remain pure-module
evidence. Current source is safe to leave untouched.

## Smallest correction

Amend the proposal text only, in one docs change, with these sentences:

1. `PATH` is exactly one fixture-owned reparse-free directory whose only
   executable is the hashed helper `cargo.exe`. The resolved path equals that
   file before create. Arguments are empty. The helper stays outside workspace
   members. `policy_fixture` stays false.
2. Add the pass and total columns from SP-ORACLE, name payload `X`, and limit
   prefill to the eight stream rows, four 109 rows, and two controls.
3. Consumption and resume counts survive `Invocation::drop` as `calls` does.
   `observe` copies them. The resume increment is after the resume injection
   return and before `ResumeThread`.
4. Duplicates are taken after the inherit list is sealed and before the
   writer drops that precede assign. Direct `run_inner` does not honor
   `quarantined`; each row uses a new `Execution`.

No source, driver, test, or workflow change belongs in that amendment.

## Review questions

1. After that amendment, does the coordinator limit gate 2 to non-discovered
   functions, with registration still requiring its own sentence in the
   preparation disposition?
2. Which already-approved disposable standard-account controller will run the
   nested-job check? This review selects none and does not treat nesting as
   already safe.
3. Is the parent-writer duplicate accepted as a fixture-only exception to the
   dormant no-new-handle rule once SE-DUP is in the text? Acceptance of the
   exception is still not permission to compile or run it.

## Close

Standards: 0 blockers. Spec: 2 blockers; worst is SP-ORACLE, the missing pass
and totals can be copied from the capture the proposal forbids using.
Security: 3 blockers; worst is SE-IMAGE, an unpinned `cargo.exe` can spawn
real descendants. No implementation or run authority.

M2 is incomplete. Real-Runner PA6 record/replay remains OPEN. Native,
installed, and dedicated paths are **unverified on real hardware**. The cause
of run `43545244266` attempt 1 is unknown. No retry is authorized.

Performed: read-only Git inspection, one SHA256 of blob `4cb039f4`, and this
file's write and commit. Not performed: source or proposal or PROGRESS edits,
build, test, native API, child launch, owner or account or service or VM
inspection, private-state inspection, network, GitHub, install, CI, subagents.
