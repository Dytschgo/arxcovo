# Grok recheck: pure-boundary correction of the main71 Windows feedback proposal

Disposition: **NO BLOCKERS in this narrow delta. The proposal stays unaccepted. No implementation or run authority.**
xAI / grok-4.7, high, review sub-agent, 2026-11-14. Single independent reviewer.
The assignment forbids further delegation, so both axes and the security check are this one pass.
This checks only Sol medium's correction against the prior different-provider recheck.
It gives no independent credit to the original Opus architecture (`cc44625` / `1ddd138`).
That architecture's independent review remains Sol high `43bb26d` / `d2bd9d3`.

## Exact pins

| Item | Identity |
| --- | --- |
| Review checkout before this commit | `73aa2e234eab56f7c6e736a60abbd2ba7b972a97`, clean, cwd = Git top level |
| Checkout role | Isolated linked worktree. Private worktree Git dir under the common Git dir. No source or runtime checkout was created for this review |
| Target (Sol medium correction) | `73aa2e234eab56f7c6e736a60abbd2ba7b972a97`, parent `5f357fd6fd4321940799d8835766e3158371499a` |
| Base (retained Opus recheck) | `5f357fd6fd4321940799d8835766e3158371499a` (tree `1a40b4e50986944af221773b8393c9e3d21d7422` equals `0da16cd693742ad11d1116af5e10657084d786f3`) |
| Retained Sol high review | `d2bd9d3c5140b3b540b07093bb9bae8930df5513` (tree `3692614d76a9eedf33464ee9b30f538155185fec` equals `43bb26d0cd8342b9c1d9887e6bc84b2cd18b3fac`) |
| Diff `5f357fd..73aa2e2` | `docs/reviews/windows-main71-feedback-proposal.md`, `docs/PROGRESS.md` only. 2 files, +41 / −14 |
| Proposal blob before / after | `eb7c712e8189a804e48c85e32f8a2d1da4066ff1` / `46d378775a5639fb4138eddbfd21b0d2d532c171` |
| PROGRESS blob before / after | `6b4484dab68164f73c791b3411dc48beb61a2bfe` / `5216f7040c7f60e6fa53ce46a6319a9c3df8e104` |
| Opus recheck blob | `docs/reviews/windows-main71-contract-opus-recheck.md` `4054e511dea983287eeccd1cd27241cc8d84a73f` |
| Sol high review blob | `docs/reviews/windows-main71-feedback-sol-review.md` `44daccc8a24bc96fa9ac56c9ea2180cc683e136f` |
| AGENTS.md blob | `c2ae25947916672b13180778265354907b75e295` |
| Source blob, failure commit `a9ec0ac`, base, and target | `crates/arxd/src/development/windows.rs` `85f0c0eca19950fc65a02b621ff752eb6ba61551`, 1085 lines, 41037 bytes. Not in the diff |

I read AGENTS.md, CONTRIBUTING.md, the Review and Debug skills, the retained Sol high review, the retained Opus recheck, the full proposal, and the correction diff. Source was read with `git show` of blob `85f0c0e` only. `facts` calls `info` for TokenUser, TokenElevation, TokenIntegrityLevel, and TokenGroups; each of those four reaches `ok(GetTokenInformation)` at line 64, so one token-info label cannot name the class. `CreateProcessW` is the direct call at line 448. The file has no Win32 mock layer. No feedback loop was run: this assignment forbids build, test, native API, and process launch. Debug phase 1 stays open. No cause is offered here.

## Standards

**PASS. The prior Standards pass stands. No new blocker.**

The delta is prose. It does not claim an unrun test, a reproduction, or a hardware result. The PROGRESS bullet records the owner routing update, names the two prior text blockers, describes the text narrowing, and states that no native run or retry is authorized. It does not say those blockers were already closed and it does not authorize implementation. Same-provider review is still not the cross-provider merge gate. Repo-relative paths only; no credentials or owner data.

No Fowler smell is asserted. The new correction section repeats P3/P4 limits so the delta can be checked without treating that repetition as a second contract.

## Spec

**PASS. Prior blocker SP1 is resolved. No new blocker.**

Resolved:

- **Mock seam.** P3A limits pure tests to the diagnostic module: record lifecycle, closed mapping, formatter, error validity, index preservation, and a failure-capture helper with an injected pure error-reader closure. The clobber uses that mock value. The pure test has no GetLastError or SetLastError call. The text forbids adding a Win32 mock layer and forbids claiming that pure tests reach real failure sites.
- **Real-site coverage.** Marker reachability, deletion and mislabel mutation coverage, and quarantine effects stay UNVERIFIED until separately authorized native replay. The 18-origin / 28-label manifest is static inventory and module mapping only.
- **Buys bullet 1.** Pure tests are red-capable for module formatting, record lifecycle, and closed mapping only. They do not prove marker reachability, native label or deletion mutations, or a failure cause. Phase 1 remains open.
- **Token granularity.** The correction states that `daemon_token_info` and `child_token_info` each merge four token classes and do not identify the class. That matches `facts` / `info` at line 64.
- **Questions 3, 4, and 6.** The correction says they are conditional requirements already in the proposal, not fresh requests and not run authorization. P1 rejects nextest process isolation as thread-ownership proof. P4's budget rules remain preconditions of a later native proposal. The named field words are a design inventory, not an implemented layout.

Checked, not blocking:

- **P4 mutations are not current native authority.** The capture-ordering, context-index, and mapping sentences sit in "later native acceptance contract." They require a real failure-capture branch when that later replay exists. They do not say those mutations have been run, and they do not say pure tests satisfy them. P3A and P3C already keep that work unverified and unauthorized. The same paragraph says replay cannot prove native last-error order, cleanup equivalence, timing, or the historical cause.
- **No focused repro, cause, or cleanup equivalence.** Status and the buys section keep phase 1 open and name no hypothesis. The limits section still says cleanup equivalence is unproved, and the assign snapshot is still not the replay snapshot. "Cleanup is equivalent" is not in the proposal.
- **Buys bullet 2** still summarizes the future natural CI line as "stage, Win32 error, iteration, numeric state." P1 still says attr-size may lack a valid failure error and is a last-error observation with validity, not a cause. P1 controls. The summary is not a cause claim and does not reopen SP1.
- The old "Review needs" questions remain on the page. The new paragraph stops them from being fresh requests. Question 3 does not pin the closed field list. This recheck does not accept a field layout.

## Security and execution authority

**PASS. Prior blocker SE1 is resolved. No new blocker.**

- **Zero CreateProcessW.** Every separately authorized native replay case at or before `create_process` must assert zero `CreateProcessW` calls with a `cfg(test)`-only counter at the actual call site. Marker absence is not enough. Pure counter bookkeeping does not earn native zero-call evidence. There is no current native case, so the assertion is not treated as already proved.
- **Injection.** Test code only, before the selected API, and that API is not called. Not selectable from CLI, config, or MCP.
- **Success path.** Instrumentation adds no native calls or handles.
- **Post-create and quarantine.** A later assign, token, or resume case must still show the child never resumes and must stay on the original error and quarantine path. No token, Job, path, bounds, or quarantine bypass. Those effects stay unverified.
- **Retry.** A new SHA, a new test name, and past ordinary CI do not authorize a historical, privileged, or extra retry. The PROGRESS bullet repeats that no native run or retry is authorized. Run `43545244266` is not reclassified as reproduced, fixed, or cleared. Future ordinary CI of an implementation PR would not be that authority either. This correction does not grant it.

## Disposition

SP1 and SE1 are cleared inside the proposal text. This report does not accept the proposal. Root disposition is still required. Debug phase 1 is not done: no red-capable loop for the main `ExecutionFailed` was run, and none is authorized. M2 is incomplete. Native, installed, and dedicated paths are **unverified on real hardware**. The main failure at `windows.rs:1072` in run `43545244266` stays unresolved, with no cause and no retry.

This report grants no implementation, build, test, native call, process launch, CI dispatch, retry, publishing, or merge authority.

Performed: read-only Git object inspection and this file's write and commit. Not performed: implementation, build, test, native API call, process launch, network, GitHub, owner or private-state operation, install, subagent, or edits to source, the proposal, or PROGRESS.

Standards: 0 blockers. Spec: 0 blockers (SP1 resolved). Security: 0 blockers (SE1 resolved). No run authority.
