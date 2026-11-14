# M2 remaining gates: narrow Opus recheck of the SP-R1 proof correction

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
Single-agent, read-only, docs-only review; no subagents. **No blockers on
either axis.** The proposal stays unaccepted. This report grants no
implementation, dispatch, run, activation or owner step.

Authorship note: I (Opus high) wrote the original proposal and the prior
recheck. I claim no independent architecture credit for the original
proposal or for ADR 0019/0020 text I helped write. This recheck covers only
the Sol medium correction delta against my prior SP-R1 blocker and its
advisories. The correction was made by a different provider, so this is a
cross-provider check of that delta and nothing more.

## Scope and identity

- Cwd equals Git top level in an isolated T3 worktree (not the canonical
  checkout), branch `t3/review-m2-gate-proof-correction`. Initial status
  clean; HEAD `1a173a9159ba00f72a4b18f88ba2ddf204538ea8`.
- Target: `git diff f757b3ac5e22a6a45e5a0f9acad7eb64d17ca4f8..1a173a9159ba00f72a4b18f88ba2ddf204538ea8`.
  Only two files changed: `docs/reviews/m2-remaining-gates-proposal.md`
  (blob `5d95578469b03f734028aa14b0c52f63fdc406ef` →
  `1ebb423b649de49a04373bc5a9e3f8afb64a9215`) and `docs/PROGRESS.md`
  (→ `3a05089e54fcd21be8006a5f147104809b0f9bc6`, two added lines).
  `git diff --check` passes.
- Findings being rechecked: Opus high recheck
  `ae05533d4404dfc511d0d3b77392d13f276b625e`, retained at `f757b3a`
  (`docs/reviews/m2-remaining-gates-correction-opus-recheck.md`, identical
  at both pins).
- Read: AGENTS, the installed review skill and its active profile, and
  source at HEAD: `crates/arxd/src/service/unix_bridge.rs` (blob
  `0514083`), `unix_bridge/readiness.rs`, and
  `crates/arxd/src/service/executor.rs` (blob `707ed16`).
- Not done: no build, test, mutation, fixture, experiment, external request,
  GitHub write, push, CI, owner/admin, native or private-state action. Old
  Lab Manager and `agent-windows-test` were not touched. Hardware paths
  remain **unverified on real hardware**.

## Standards

**0 blockers; 1 advisory.** Earlier advisories: ST-R1 resolved, ST-R2
resolved.

- **ST-R1 resolved.** Proposal lines 223–227 place schema/version, reviewed
  SHAs and run/attempt association in an envelope inside the same single
  validated 4 KiB JSON line, not a separate artifact. Bounds and
  reject-not-truncate remain. Lines 228–230 restate the one-line cap; that
  repeats the earlier bullet but does not contradict it.
- **ST-R2 resolved.** Lines 4–5 credit Sol medium revisions (`a7b98df` and
  the SP-R1 correction) and keep original architecture authorship with Opus.
  The PROGRESS line records the same split.
- **ST-N1 (advisory): stale status line.** Lines 6–8 still say only that
  `9b62dd2` requested changes and that the revision awaits a narrow Opus
  recheck. They do not name `ae05533` (retained `f757b3a`) as the source of
  the SP-R1 correction. One clause would complete the provenance.

## Spec

**0 blockers; 2 advisories.** Earlier findings: SP-R1 resolved, SP-R2
resolved, SP-R3 resolved (see SP-N1).

- **SP-R1 resolved.** All three gaps are closed in text:
  1. **Ordering.** Lines 114–119 require S2a, which implements the D6
     catch-arm latch, to have its own meaningful tests, independent source
     review and green required checks, and to merge first as a separate inert
     PR. S2b is re-pinned onto merged S2a. U-a's latch assertion and U-b's
     post-panic early refusal are not claimed against the original catch arm
     (`unix_bridge.rs:151-153`, which does not latch today). Line 288 orders
     S1, S2a, S2b, F0.
  2. **Masked step.** U-b.2 (lines 150–157) defines
     `in_slot_readiness_refusal_omitted` as removing the readiness refusal
     (`unix_bridge.rs:89-91`), not only the residue observation. With the
     latch in place, that mutant reaches `runner.run`, so the
     zero-Runner-call assertion kills it. U-b.3 (lines 158–164) is a separate
     case: readiness Ready, residue published, no panic latch and no
     quarantined prior dispatch. Bridge preflight (`unix_bridge.rs:140-143`)
     does not consult residue, so only step 1 (`:86-88`) can latch and cause
     the refusal. `in_slot_residue_check_omitted` then reaches `runner.run`
     and is killed. Neither case needs a reset; line 165 forbids resets of
     readiness, quarantine and the monotone flag.
  3. **Release point.** U-b.2 releases the held request only after the
     panicking call has returned and its slot is observed free. That is
     after `fetch_sub` (`executor.rs:400-406`). A `Busy` refusal
     (`:395-397`) earns no gate credit, nor does early preflight or row-G
     quarantine. The hold point is the existing `admission_barrier` seam,
     after executor preflight (quarantine, stopping, backend) and before
     `fetch_add` (`executor.rs:289-298`, `388-397`). That matches "after
     quarantine checking and before slot acquisition". `catch_unwind` in the
     bridge prevents the panic from becoming a `JoinError`, so the executor's
     quarantine flag is not set by this path.
- **SP-R2 resolved.** The Stage F heading (line 204) now reads "local
  feedback artifact; any dispatch remains unauthorized". It no longer claims
  new-proof authority. The F2 body still controls.
- **SP-R3 resolved, with SP-N1.** L-b2 (lines 96–98) adds a missing location
  plus a refusing live check, requiring that check's exact token and zero
  factory/setup/Runner calls. Under current preflight order (check, stored
  runner result, readiness) the check's token wins.
- **SP-N1 (advisory): L-b2 claims more precedence than it can pin.** With
  `Location = None`, the stored runner is `Err(MechanismUnavailable)`.
  Readiness `fail(Setup)` also maps to `MechanismUnavailable` in
  `readiness.rs` `check()`, through the default arm. So L-b2 pins only
  check-first, not runner-before-readiness. It also proves check-first only
  if the refusing closure uses a token other than `MechanismUnavailable`.
  Either narrow the wording to "check-first", or require a distinct token
  and drop the runner-before-readiness claim.
- **SP-N2 (advisory): the refusal token does not identify the gate.** A
  latched `Containment` maps to `MechanismUnavailable`. That is the same
  token as `Setup`, and also the token for a missing stored runner. U-b.2's
  "observe entry into the actual in-slot readiness refusal" therefore rests
  on three things: the held position after preflight, the absence of
  Busy/Stopping/CancelledBeforeSpawn, and the zero-Runner-call count. It
  does not rest on the token. U-b.3 can observe its latch directly:
  preflight is `Ok` before release and refuses after it. The S2b
  implementation review should check that the named assertions use these
  observables, as line 166 requires, and not a token match alone.
- **No authority widening.** The proposal remains "proposal, not accepted"
  (line 6). F2 authorizes no historical retry or new dispatch (lines
  247–255). A13 is only proposed (lines 196–198). Real-Runner PA6 remains
  **OPEN** after S2 (line 200). S2a is inert, non-activated code with no
  production constructor (line 119). Nothing is activated, and no approval
  flow or owner request is added. The PROGRESS line says the proposal is
  unaccepted and that no implementation or dispatch is authorized.

## Verdict

**No blockers. The SP-R1 correction is resolved; advisories only.** SP-R1,
SP-R2, SP-R3, ST-R1 and ST-R2 are resolved without widening authority.
ST-N1, SP-N1 and SP-N2 are optional text tightenings and do not block.
This review does not accept the proposal. It also does not authorize S1,
S2a, S2b, F0, any dispatch, A13 or activation. Any acceptance, and each
implementation PR, still needs its own decision, a different-provider
review and green required checks.

Standards: 0 blockers / 1 advisory (status line provenance). Spec:
0 blockers / 2 advisories (L-b2 precedence overclaim; token-only gate
observation).
