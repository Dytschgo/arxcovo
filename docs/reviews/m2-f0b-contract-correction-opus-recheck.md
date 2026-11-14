# M2 F0b contract correction: narrow Opus recheck

2026-11-14. Reviewer: Anthropic / claude-opus-5-5 / high. Single-agent
review; no subagents. Corrector reviewed: coordinator OpenAI / gpt-6.1-sol /
medium. This is a different-provider recheck of the Sol correction delta
only. Opus authored the original F0 design, the F0b contract and the F0b
proposal, so this recheck earns **no independent Opus architecture credit**
for the proposal as a whole. It grants no acceptance, implementation,
execution, publication, dispatch, retry or merge authority.

**Standards: PASS, no blockers (ST-B1 resolved; three advisories).
Spec/security: PASS, no blockers (SP-B1 and SP-B2 resolved; four
advisories).** The proposal stays unaccepted until the root/coordinator
records its disposition.

## Scope, isolation and exact pins

Cwd equalled the Git top level in an isolated T3 worktree on branch
`t3/f0b-contract-recheck`. `git status --porcelain` was empty and HEAD was
the target. Paths below are repository-relative; no owner filesystem paths
or private state are retained.

| Object | Exact identity |
| --- | --- |
| Target (correction) | `34191d4c509b133180a39251147bd592535bc25d` |
| Parent (retained Sol review) | `7d7aa2f277f6088e92ee27c3c65f5f9a8ea75a5a` |
| Original Sol high review commit | `9682af4085ced238a59fcdd694c257acde74c5ef` |
| Sol review blob, identical at `9682af4` and `7d7aa2f` | `d9b9f60b08c56fdd2108a665c2eaf8b5ade70c20` |
| Proposal blob before / after | `62dbc83e1eed6cb8b386eef26315ecc98ff835f8` / `38351bd9dbd07981389fd18d255877bcbb005e3f` |
| PROGRESS blob before / after | `4f6b89141f05ba19d0566158aabf10db28a4e386` / `81d88334d7593d0e12c5737dae7292072324ba6b` |
| AGENTS (unchanged) | `c2ae25947916672b13180778265354907b75e295` |
| PR70 merge / authoring base, same tree `eccb0ce5…` | `affaec91b83d3a3c06a67dc796578a98972c0c2b` / `f0baed412b82abf5ade88e3016853e4bf08b7f66` |
| PR71 head present locally | `e14ae4b0a32eb9715d1969ad24c2b5a812e37197` |

`9682af4` is not an ancestor of `7d7aa2f`; it was retained by an identical
re-commit (same message, same single-file blob). `git diff 7d7aa2f 34191d4`
touches only `docs/PROGRESS.md` (+2) and the proposal (+54/−17);
`git diff 7d7aa2f 34191d4 -- ':!docs'` is empty, so source is unchanged and
no test is needed for this delta.

Read: AGENTS, the dytschgo-review skill and its active profile, the full
retained Sol review, the full corrected proposal and the correction diff.

**Checks actually performed:** Git identity/status/log/diff/rev-parse reads,
file reads and grep only. No build, test, mutation run, Python, network,
GitHub, CI, native command, owner workflow or private-state access occurred.
The local `main` ref was not fetched; PR71 and CI status are recorded
context, not checked here.

## Standards

**ST-B1 — resolved.** Lines 588–595 replace "all 32 tests and all 23
mutants re-run, v2 expectations only" with the requested contract:

- all existing F0 behavioural obligations are retained;
- narrowly scoped anchor and counterfactual migrations are permitted in
  `test_f0_blockers.py`, naming the `xf_success` anchor that V5 replaces;
- the invalid-success case must satisfy V1 so that only the success rule
  can reject it;
- every legacy mutant is re-isolated against the new cross-field rules,
  keeping named assertion attribution and zero error/compile credit;
- "A count of 23 is not a substitute for those properties"; equivalents are
  reported honestly.

The Delivery bullet (line 603) widens the file scope to match and no
further. The remaining "32-test" mention (line 663) describes the historical
CI gap, not a credit.

**ST-A1 — incorporated.** Lines 572–580 state the 33 names are candidate
mutations, not earned kills. Final-file NOFOLLOW removal is equivalent when
EXCL refuses the existing symlink, and no fake OS may follow it to
manufacture a kill. Ancestor NOFOLLOW stays separate. Combined
nonblock/type tests are split if each is claimed, with flags asserted before
a fake FIFO open. Every negative case must otherwise satisfy schema,
ownership, size and canonicality conditions.

Advisories (non-blocking):

- **ST-R1.** "Ancestor NOFOLLOW remains a separate guard", but no candidate
  name drops NOFOLLOW from the `open_stage` walk; `writer_walk_skips_ancestor`
  is a different mutation. Name one at implementation or state that it is
  covered.
- **ST-R2.** Only `success_accepts_unknown` is a named success mutant; the
  record tests also cover malformed and captured success. Consider matching
  candidates, subject to the same equivalence rules.
- **ST-R3.** Stale prose: line 606 still says "This design first needs a Sol
  high review", and lines 41–43 record the authoring-time "awaiting CI /
  not merged" state beside line 608's merged PR70. The status header
  supersedes both; tidy them in the next proposal edit.

## Spec/security

**SP-B1 — resolved.** Lines 286–294 drop "contains every writer". Cgroup
absence now supports only a conditional, bounded post-stop snapshot under
the existing containment assumption. The text explicitly disclaims
escaped-root coverage, atomic cross-file consistency, immutability and
complete capture. It is labelled a source inference, **unverified on real
hardware**, and requires bounded strict reads to stay safe under privileged
concurrent modification. Section 6 (lines 342–346) recasts monotone creation
as a source assumption, with metadata observed at read time only. Lines
295–298 keep `unknown` with no feedback-channel open when the condition is
unavailable. They permit only trusted-stage opens and marker metadata stats
for progress, never marker contents, and scope the "no open" test phrase to
feedback channels. Section 8 and Known unknowns remain consistent. No
process discovery, containment mechanism or observation run was added.
A grep for `every writer`, `no writer is alive`, `immutab` and
`complete capture` finds only the new disclaimers.

**SP-B2 — resolved.** V5 (lines 393–403) explicitly adopts the narrow
deviation: a telemetry anomaly on otherwise successful execution yields only
the closed `E_RECORD` fallback and exit 1. It forbids inventing first/final
exceptions or cleanup failure to explain it. Existing body/stop/cleanup
exceptions, first/final classification and cleanup tokens stay unchanged.
The ordering test (lines 508–510) is now qualified to existing failures, and
the record test (lines 512–515) requires real entry/run with all effects
mocked. It covers unknown, malformed and captured statuses on apparent
success, including reader-exception-to-unknown, with fixed `E_RECORD`
stderr, exit 1 and no fabricated first/final or cleanup-failure tokens. No
blanket reader-exit invariance survives. The writer's "never changes the
internal exit code" (line 238) concerns internal processes and does not
conflict.

**Q1–Q6.** The retained dispositions (blob `d9b9f60…`) are unchanged. The
delta discharges the conditions attached to Q2 (SP-B1), Q3 and Q5 (SP-B2),
and contradicts none. No authority is widened. "Re-pin onto current main
before implementation/publication" is a precondition, not an authorization.

Advisories (non-blocking):

- **SP-R1.** The name `quiescent` (line 280, headings, lines 444–445, 529,
  Q2) still suggests writer quiescence. Define it once as the conditional
  completion signal, or rename it at implementation.
- **SP-R2.** Line 290's "those writers" lost its antecedent when "contains
  every writer" was removed. Read it as "writers inside the unit subtree".
- **SP-R3.** In PROGRESS, the new top bullet is accurate, but the two older
  current-state bullets still say the Sol review is "active" or "pending".
  The coordinator should mark them superseded in its next PROGRESS update.
  This recheck does not edit PROGRESS.
- **SP-R4.** The Q1 prose in the proposal (line 641, "the statuses already
  give it") is broader than the Sol disposition (only the captured-slot
  count is derivable). It is not changed by this delta and grants nothing.

## Unchanged gates

The proposal remains **unaccepted**, and no implementation starts until the
root/coordinator records a disposition. Later delivery still needs a
different-provider source review and all required CI, `ci-time-budget` and
`agent-review` gates, and only the coordinator merges. F2 stays
undispatched; ADR 0019 D7/C2 stand, with no historical retry. A new SHA
resets no failure counter and closes no evidence gate. **M2 is incomplete.
Real-Runner PA6 and real-record replay remain OPEN. Native, installed and
dedicated paths are unverified on real hardware.**

Standards: 0 blockers, 3 advisories; worst is the unnamed ancestor-NOFOLLOW
candidate (ST-R1). Spec/security: 0 blockers, 4 advisories; worst is the
residual `quiescent` naming (SP-R1).
