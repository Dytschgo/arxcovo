# ADR 0019 Stage B tool-acceptance recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-12. Review sub-agent,
no subagents, single-agent focused recheck. This recheck closes F1 and checks
the F2 disposition from [the prior Opus review](0019-stage-b-independent-review.md)
(report 8893741 on candidate 454beb6 / source 4fe2bde). It is not a broad
re-review. The production code it relies on is byte-unchanged from that review.

Independence: this counts as cross-provider review only of Sol's Stage B
implementation and Sol's test correction. Opus wrote the ADR 0019
architecture, so this recheck claims no independence for that contract.

**Standards: PASS** (F1 closed). **Spec: PASS** (the earlier result stands,
production unchanged). **F2: accepted as disclosed**, and the audit obligation
binds Stage C/D.

## Pins

- Worktree: runtime-prepared T3 worktree, branch `t3/0d6240c0`. Its top level
  matched the working directory and is not the canonical checkout. HEAD was
  exactly `c60b1cc411b921dbae993f3d9d34a09421fd4097` with a clean status. No
  checkout or worktree was created and nothing was written to the canonical
  checkout.
- Base `f2a8a5a` holds the prior report. Actual main is
  `064736022e559d0fb46a1de3943e57212eca87b0` (#51).
- `git diff f2a8a5a c60b1cc` touches only `exec/stage_b_tests.rs` (+9 lines),
  `docs/PROGRESS.md` (+10) and the new
  `docs/evidence/unix-runner-stage-b-review-followup.md` (+41).
- `git diff 4fe2bde c60b1cc -- crates` outside `stage_b_tests.rs` is empty.
  Blobs: mod `e94549e8`, unix `d3ec3e7e`, cgroup `f6c65008`. These equal the
  prior review's pins. The new stage_b_tests blob is `f17e1b55`.

## F1: closed

The new L2 lines (`stage_b_tests.rs:116-128`) do what the review asked:

- `tool`: shim prefix with `arguments[1] = "tool"` and 128 tail items (132
  items in total). Legacy `validate_spec` returns `Invalid("argv bounds")`,
  and `validate_phased_spec` accepts it.
- Wide `tool`: a 4,096-byte target and a 4,096-byte cwd, plus a tail of
  128 × 512 = 65,536 bytes. It is accepted in path mode and in tool mode.
- Assertion labels identify which case failed.

Own mutant: changed `mod.rs:318` from `"path" | "tool"` to `"path"`, then ran
`cargo nextest run --locked -p arxd --no-fail-fast -E 'test(exec::stage_b_tests::)'`.
Result: **7 passed, 1 failed**. `l2_exact_prefix_and_tail_bounds` panicked at
`stage_b_tests.rs:119` with "tool prefix accepted". Run
`0bb33315-ff24-41d4-a689-bdb0b29897cc`. Before the review this same mutant
survived (run `0d11a6bd-…`, 8/8 passed). The original file was copied to my
scratch directory first and restored by an EXIT trap. SHA256 is `d089fb44…`
both before and after, `git hash-object` gives `e94549e8` and `git status` is
clean.

Standards check of the 9 added lines: test-only, one concern, and they follow
the surrounding L2 style. No smell from the baseline applies. Optional, not
required: L2 does not reject a tool-mode target of 4,097 bytes. Because the
production bound is shared by both modes, the existing path-mode case already
covers it.

## F2: disposition accepted

The follow-up evidence lists exactly the six bypass forms from the prior
probe: `Result::unwrap`, `Option::unwrap` as a function value,
raw-identifier `r#unwrap`, a renamed logging macro import, a `RunFailure`
type alias in a `From` impl, and `writeln!` to stderr. It states that the
reviewed production has none of them, that this is no semantic privacy proof,
and that Stage C/D must audit these forms when adding files and tighten the
checker if the extension creates an ambiguity. The prior review allowed this
"otherwise state these limits" path. No P-SCAN or production change was made,
which is correct.

Binding note for Stage C/D: the reviewer of any P-SCAN extension must check
the six forms. A green P-SCAN alone is not enough evidence.

## PROGRESS

The new current section comes first. The earlier text is kept under
"Historical checkpoint before Stage B review follow-up". Its statement "Main
CI after #51 is pending" is historical and does not contradict the current
line, which credits run `43233144276` (596s, 14:02:42-14:12:38 UTC) as observed
by the coordinator. This recheck did not re-verify that run through the API.
Nit: a doubled blank line follows the historical heading.

## Commands run (Windows, this worktree)

- Focused L2 on the unmodified target: 1 passed, run
  `7ea29928-3f35-4cf4-bd32-fabc74800633`.
- Path-only mutant on the Stage B filter: 7 passed / 1 failed (killed), run
  `0bb33315-ff24-41d4-a689-bdb0b29897cc`, then restored.
- Stage B focused suite after restore: 8 passed, 299 skipped, run
  `fe1997c3-de40-4886-aba0-c29112228d06`.
- `cargo fmt --all --check`: passed.

Not run: Clippy (credited to the coordinator), the full workspace suite (the
builder's 306 passed / 2 skipped stays credited to 4fe2bde), any Unix, root,
native runner, cgroup, spawn, account, service, SCM/LSA, VM, Lab Manager,
ignored fixture, private-state or experiment path, and no toolchain install.
No GitHub write, push, PR, rerun, settings or babysitter action. No token, SID
or private path was printed.

## Disposition

PASS. This allows publication for hosted verification. Merge still needs
exact-head review credit and all five required gates, including Linux and
macOS compile and tests. Unix compilation is still unclaimed. ExecutionFailed
and E_PROTECTION remain unexplained; no pass is a fix or waiver. Native,
installed and dedicated paths remain **unverified on real hardware**. M2 is
incomplete. Stages C/D/E/F/G are not built or activated.
