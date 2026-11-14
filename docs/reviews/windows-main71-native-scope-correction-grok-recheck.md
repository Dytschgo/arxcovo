# Windows main71 native scope correction: Grok recheck

Disposition: **CHANGES REQUIRED.** Standards pass. Security pass. Spec has
one blocking oracle misstatement. The eight published stream rows match
`drain_at` copy timing. `wait` and `exit_code` do not belong in the "other
twenty missing" sentence. This recheck does not accept the contract and grants
no root-contract disposition, implementation, compile, or run authority.

Reviewer: xAI / grok-4.7, one independent pass, 2026-11-14. The assignment
forbids subagents, so standards, spec, and security are this single reviewer.
Independence is from the Sol correction, not from a second reviewer. Label:
single-agent review.

Debug phase 1 for historical main71 was not run. No cause and no fix are
claimed. M2 is incomplete. Real-Runner PA6 remains OPEN. Native, installed,
and dedicated paths are **unverified on real hardware**. The cause of run
`43545244266` attempt 1 is unknown. No retry is authorized.

## Pins

| Item | Identity |
| --- | --- |
| Review cwd | Linked worktree. Git top level equals the review cwd. Porcelain was empty. Per-worktree Git dir under the common Git dir. `<isolated-worktree>` |
| Branch | `t3/windows-main71-scope-review` |
| Reviewed head | `6c98d2cdf2548d6188315b1356f7e06fd61be068` |
| Reviewed base | `d63ae19273b655b1f6e68019bbd89a12a7cd0f1e`, parent of head |
| Delta | `git diff d63ae19273b655b1f6e68019bbd89a12a7cd0f1e 6c98d2cdf2548d6188315b1356f7e06fd61be068` — `docs/PROGRESS.md` and `docs/reviews/windows-main71-native-replay-scope-proposal.md` only. 2 files, 55 insertions, 11 deletions |
| Proposal blob at base | `4e5f8eeb3a91f711c64bd1b04312ee9f7be7993f` |
| Proposal blob at head | `403a1bfc79951ea9c90d6431c4b5e08ad0456896` |
| Progress blob at base | `b12bd431852d77ac9e3f1be01940bcbf85f50a5b` |
| Progress blob at head | `e24c4417a2777a08963d6d26a6be87db3fe3d49c` |
| Prior Grok review | `b0b4581df6cf7bc1f6650ff50dee9e2e038f1d66` is not an ancestor of `d63ae19`. Review file blob `7a532db7d96ebb34f34851ea0293c60e7beefe60` is identical at `b0b4581`, `d63ae19`, and head |
| Windows source | `crates/arxd/src/development/windows.rs` blob `4cb039f4742568a3eb3a124ae3626ac9511b8be2` at head. 75184 LF bytes, 2050 LF, CR count 0. SHA256 `e2c91088e83ac15984219ea4581f5effe77c0e2089a9222fa1bb4eb764e1433a` |
| Workspace members | `crates/arxd`, `crates/arx-guest` |

Docs read: `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, the Review skill and
its active profile, the full prior Grok review, the full Sol delta, and
`windows.rs` at the blob above, including `drain_at!`, `totals`, `capture`,
`observe`, `Invocation`, the loop and final drains, `resolve`, `pin`, and the
quarantine store. `Execution::work` was read in `crates/arxd/src/development/mod.rs`.
No proposal or progress edit was made. No `rustc` or Cargo was executed.

## Standards

**PASS. No finding.**

The delta is one docs concern: the proposal correction plus one current-state
progress bullet. The proposal stays proposed and unaccepted. It records M2
incomplete, leaves the controller, nested Job, measured budget, and native
authority OPEN, and marks native paths unverified on real hardware. It does
not claim an unrun test, a reproduction, or a hardware pass. Repo-relative
pins and sanitized placeholders only. No credential or owner path was added.
Commit scope is the two docs files above.

Fowler baseline: no smell is filed. There is no code diff.

## Spec

**CHANGES REQUIRED. One blocker.** The published eight-row stream oracle
matches the source. The sentence that covers the other twenty rows does not.

Checked, not blocking:

- `drain_at!` calls `diag::totals` before the drain. `capture` copies `out`,
  `err`, and `pass` from that stored tuple. A read failure records the
  pre-read `Captured` totals, not the byte just written into the pipe.
  Missing means `None`, not zero. These values were not executed.
- Loop rows pass `None`. With payload `X` prefilled on both pipes and
  out-before-err order: `loop_out_peek` and `loop_out_read` copy pass missing
  and totals 0/0; `loop_err_peek` and `loop_err_read` copy pass missing and
  totals 1/0, because the out drain has already added its one byte. Final rows
  pass `Some(0)` on the first final iteration: `final_out_peek` and
  `final_out_read` copy pass 0 and totals 0/0; `final_err_peek` and
  `final_err_read` copy pass 0 and totals 1/0. The eight-row table matches.
- Prefill is limited to those eight error-55 stream rows, the four peek-109
  rows, and the two controls. Payload byte is `X`. Peek 109 takes `armed` and
  stores no failure record. Controls and peek-109 rows are specified as
  successful totals 1/1 with no drops. `wait`, `exit_code`, and the eighteen
  rows through `resume` stay without prefill.
- Consumption and resume counters are required to survive `Invocation::drop`
  the way `calls` does. `Invocation::drop`, while a `Scope` is active, clears
  `armed` and does not clear `calls`. The text copies the new counters before
  `Scope` drop, counts peek 109 as consumed, and places the resume increment
  after the resume injection returns and before `ResumeThread`. The injected
  resume row still requires resume count 0.

**SP-ORACLE. Blocking.** The correction says the pass and total values were
copied into the literal 28-row table. That table is unchanged and still has
no pass or total columns. The only literal oracle is the later eight-row
table. It then says the other twenty error-55 rows have missing pass and
missing totals because no drain snapshot is supplied.

Eighteen rows, `daemon_token_open` through `resume`, return before any
`drain_at!`. `Invocation::enter` leaves `out`, `err`, and `pass` as `None`.
Those eighteen are missing, missing, missing.

`wait` and `exit_code` are the other two. Every loop iteration stores
`stdout.total`, `stderr.total`, and `pass=None` before the loop drains. These
rows have no prefill, so both `Captured` totals stay 0. `stage!(Wait)` and
`stage!(ExitCode)` then `capture` that leftover tuple. The record is pass
missing and totals 0/0. Nothing clears `out` or `err` when the role changes.
`Invocation::drop` does not clear them either. Calling those totals missing
treats `Some(0)` as `None`.

Required text: keep the eight stream rows as published; state the eighteen
rows through `resume` as missing/missing/missing; state `wait` and `exit_code`
as pass missing and pre-read totals 0/0; put those columns on the literal
28-row table, or stop saying they were copied into it. Do not build the table
from a captured record.

## Security

**PASS. No finding.** The three security corrections match the source fences.
They still authorize nothing.

Checked, not blocking:

- `resolve` returns the first `PATH` entry's `<tool>.exe`. The correction
  requires one owned fixture directory, `PATH` exactly that directory, only
  that hashed `cargo.exe`, empty arguments, a byte hash match before every
  row, the helper outside workspace members, and real Cargo unselectable.
  `run_inner(..., false)` stays the policy path, and `policy_fixture` is not
  a bypass. Existing `pin` still rejects a reparse on the resolved path.
- Duplicates are non-inheritable, absent from the sealed inherit list at
  line 1315, and taken before the writer drops at lines 1371–1372. The list
  stays NUL/stdout/stderr. Retained writers close before temp removal.
- `run_inner` stores `quarantined` on the post-create error path and does not
  read it on entry. `Execution::work` is the quarantine check. The matrix uses
  a fresh `Execution` for every row and adds no `work` call to the child budget.
- Gate 2 allows only functions the harness does not discover: no `#[test]`
  registration, ignored-test registration, or new discovery entry. Registration
  needs a separate decision. No controller is named. Nested Job containment,
  helper hashes, and the non-replay CI share stay OPEN. The correction says
  accepting the duplicate design does not authorize compile or execution.

## Close

Standards: 0 findings. Spec: 1 finding; worst is SP-ORACLE, `wait` and
`exit_code` pre-read totals are 0/0, not missing. Security: 0 findings.
No contract acceptance and no run authority.

Performed: read-only Git inspection, one SHA256 of blob `4cb039f4`, and this
file's write and commit. Not performed: source, proposal, or PROGRESS edits,
build, test, native API, child launch, owner or account or service or VM
inspection, private-state inspection, network, GitHub, install, CI, subagents.
