# M2 remaining gates: narrow Opus recheck of the Sol correction

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
Single-agent, read-only, docs-only review; no subagents. **Changes requested
(one narrow Spec blocker).** The proposal stays unaccepted. This report
grants no implementation, dispatch, run, activation or owner step.

Authorship note: I (Opus high) wrote the original proposal (`1642726`, ported
`4662fb0`). That authorship stands, and I claim no independent architecture
credit for it or for ADR 0019/0020 text I helped write. This recheck covers
only the Sol medium correction delta against the Sol high review findings.
The correction was made by a different provider, so this is a cross-provider
check of that delta and nothing more.

## Scope and identity

- Cwd equals Git top level in an isolated T3 worktree (not the canonical
  checkout), branch `t3/review-m2-gates-correction`. Initial status clean;
  HEAD `a7b98df281226ef0bd69400907ae90e6d857e1b8`.
- Target: `git diff 8b8fca3b55915d3e3693dba5a11753de221018a3..a7b98df281226ef0bd69400907ae90e6d857e1b8`,
  limited to `docs/reviews/m2-remaining-gates-proposal.md` (blob
  `c997ddb` → `5d95578469b03f734028aa14b0c52f63fdc406ef`) and
  `docs/PROGRESS.md` (→ `7b64c4a5c4a1cb0aaf0740e3ccb7e34e29d1d1c6`). Only
  those two files changed. `git diff --check` passes.
- Findings being rechecked: Sol high review `9b62dd210f754ebdd321a2003a89c4b032cf4182`,
  kept as `8b8fca3` (`docs/reviews/m2-remaining-gates-sol-review.md`). Its
  content is identical at both pins.
- Read: AGENTS, KICKOFF, the installed review skill and its active profile,
  ADR 0020 D5/D6, and source at HEAD: `crates/arxd/src/service/unix_bridge.rs`
  (blob `0514083`), `unix_bridge/readiness.rs`,
  `crates/arxd/src/service/executor.rs` (blob `707ed16`), and the
  experiment workflow's `if:` fence.
- Not done: no build, test, mutation, fixture, experiment, external request,
  GitHub write, push, CI, owner/admin, native or private-state action. Old
  Lab Manager and `agent-windows-test` were not touched. Hardware paths
  remain **unverified on real hardware**.

## Standards

**0 blockers; 2 advisories.** Earlier findings: ST-A1 resolved, ST-A2
resolved.

- **ST-A1 resolved.** Proposal lines 199–214 add the fixed schema/version,
  reviewed source and workflow SHAs, run/attempt association, bounds on
  predecessor count and total input, reject-not-truncate handling, a test of
  the real predicate across both exception orderings, hostile-payload
  suppression, and the mechanism-only label until a captured refusal
  reproduces. The text states that no historical red replay exists.
- **ST-A2 resolved.** Lines 267–272 keep the three-failed-fix handoff and
  KICKOFF's both-provider stop condition. They also state that no counter
  resets or gate waivers apply, credits stay at their original pins, and the
  drain advisories remain without hardware credit or parked-work retry.
  Lines 33–38 correct the current-state paragraph to merged PR66 `1ac4047`,
  with the main-after-PR66 run still pending.
- **ST-R1 (advisory): where the envelope lives.** Lines 199–205 add a
  "bounded envelope" next to "one schema-validated JSON line of at most
  4 KiB", and the workflow "prints only that validated line". The text should
  say whether the envelope fields sit inside that single line, so they count
  toward the 4 KiB cap and the schema covers them. Otherwise it should say
  how a separate envelope is bounded and kept without being printed. It
  should not leave two artifacts with one cap between them.
- **ST-R2 (advisory): attribution.** The header (lines 3–9) still names
  only the Opus author and uses Opus's first person. The F2 section, the
  owner item and the failure-accounting text are now Sol medium wording.
  Add a single line such as "Revised: OpenAI / gpt-6.1-sol / medium,
  `a7b98df`, addressing `9b62dd2`" so provenance is not misattributed. The
  PROGRESS line already records the split correctly.

## Spec

**1 blocker; 2 advisories.** Earlier findings: SP-B1 resolved, SP-B2
partially resolved (see SP-R1), SP-A1 mostly resolved.

- **SP-B1 resolved, without widening authority.** F2 (lines 222–230) now
  says a new SHA does not discharge ADR 0019 D7's no-retry restriction or
  the evidence gap. Root must disposition both explicitly. Any observation
  needs its own reviewed scope, pins, a fresh environment, technical fences
  and owner-only dispatch. It is not A1–A13 acceptance. F0 replay comes
  before hypotheses, and an environment with unconfirmed cleanup is excluded.
  The owner item (lines 244–249) makes no request or preapproval. Its fence
  list matches the workflow `if:`: owner actor and triggering actor, main
  ref, `github.sha` and `github.workflow_sha` equal to the reviewed-SHA
  variable. F0 stays local artifact/replay only, and no dispatch is
  authorized.
- **SP-A1 mostly resolved.** Lines 76–81 specify the owned `Fn() ->
  Result<(), Unavailable> + Send + Sync + 'static` closure, called on every
  preflight, and forbid an eagerly evaluated Result or a NativeResolver.
  L-a/L-b cover the exact token, no derive/factory/setup, and no Ready
  afterward. See SP-R3 for precedence.
- **D6 and the A13/PA6 framing are correct.** Lines 156–164 now present the
  catch-arm `Containment` latch as a separately reviewed inert change that
  aligns code with D6 ("store … then latch, before its slot release"). D6 is
  not reworded. Lines 150–154 and 174–176 keep U-a as real-Group decomposition
  evidence and U-c as a partial lexical guard, with no real-Runner, root or
  cgroupfs credit. They also state that A13 is proposed and not accepted, and
  that real-Runner PA6 stays **OPEN** after S2.
- **SP-R1 (blocker): the U-b gate assertion is still maskable, by the D6
  latch itself, and its ordering is unstated** (lines 136–143, 150–152,
  156–164). From source:
  - The in-slot gate has two steps: `if runner.cleanup_residue() { latch }`
    and then `readiness.check()` (`unix_bridge.rs:86-91`).
  - The pre-admitted request can be held with the existing `admission_barrier`
    seam. That seam sits after executor preflight and before
    `fetch_add` (`executor.rs:388-397`).
  - Quarantine is not re-checked after the barrier, so the quarantine part of
    the masking is resolved.

  Three gaps remain:
  1. **Ordering.** U-b's "ordinary request refuses early after the panic
     latch" and U-a's "also asserts the latch" only hold after the separately
     reviewed D6 catch-arm latch exists. Today the catch arm does not latch
     (`unix_bridge.rs:153`). The proposal must say that the D6 change is
     reviewed and merged before S2's latch-dependent assertions, or else mark
     those assertions conditional. S2 must not ship the latch inside itself.
  2. **Masked step.** With the D6 latch in place, readiness is already
     `Containment` when the pre-admitted request enters the slot. A mutant
     that drops only step 1 (the residue observation) then still refuses
     through step 2, so it survives: readiness masks it. Define
     `in_slot_residue_gate_omitted` as removing the in-slot readiness
     refusal (or both steps), killed by the pre-admitted request's Runner
     call count. Then either kill a step-1-only mutant separately, or claim no
     credit for step 1. To kill it, run it with residue published while
     readiness is still Ready and nothing is reset. For example, use a fake
     port whose `cleanup_residue()` is already true at first admission:
     preflight does not consult residue, so only step 1 can refuse.
  3. **Release point.** With `limit = 1`, releasing the held request "after
     residue publication" can come before the panicking call's
     `fetch_sub`. The request is then refused `Busy`
     (`executor.rs:395-397`), and the real code and the mutant behave the
     same. The text must say the request is released after the panicking
     call's slot release, and that a `Busy` refusal earns no gate credit.
- **SP-R2 (advisory): leftover "fresh proof" framing.** The Stage F heading
  (line 180) still reads "for a future fresh proof", which is the withdrawn
  F2 premise. The F2 body controls and nothing is authorized, but "for any
  separately authorized future observation" would match it.
- **SP-R3 (advisory): check precedence.** Bridge preflight runs the config
  check first, then the stored construction result, then readiness
  (`unix_bridge.rs:140-143`; ADR 0020 D5). The text should say that L-a uses
  the passing FakeResolver check. It should also add a case with
  `Location = None` and a refusing check, asserting that the check's token
  wins. That keeps the existing precedence pinned, as SP-A1 asked.

## Verdict

**Changes requested, narrow.** SP-B1, ST-A1 and ST-A2 are resolved. The
correction does not widen authority: no dispatch, retry, activation, owner
request or approval flow is added, A13 remains proposed, and real-Runner PA6
remains OPEN. SP-B2 is still not closed because of SP-R1, which needs
text-only fixes in the proposal: D6-before-S2 ordering, the mutant's scope
and the step-1 claim, and a release point after `fetch_sub`. After that
correction, a further narrow recheck by a different provider from the one that
writes it is needed. This review grants no implementation, dispatch or
activation.

Standards: 0 blockers / 2 advisories (envelope placement). Spec: 1 blocker /
2 advisories (U-b gate maskable by the D6 latch and by `Busy`).
