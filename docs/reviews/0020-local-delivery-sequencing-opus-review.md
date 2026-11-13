# ADR 0020 local delivery sequencing independent Opus review

Reviewer: Anthropic / Opus 5.5 / high, 2026-11-12. Single review agent; no
subagents. Independence is claimed **only** for the coordinator's
OpenAI / gpt-6.1-sol / medium delivery-sequencing delta `4272291`. The ADR
0020 architecture keeps its existing credit: the original Opus proposal and
correction 1 keep their Sol high reviews
([original](0020-sol-independent-design-review.md),
[correction 1](0020-correction1-sol-recheck.md)), and Sol correction 2 keeps
its Opus high check ([recheck](0020-correction2-opus-recheck.md)). PR56
published head `4627b83` passed exact-head Opus review.

Disposition: **PASS, no blocker; three non-blocking advisories.** The delta
changes delivery sequencing only. It does not change native, privileged,
owner or activation authority, any technical fence, or any merge gate. This
docs-only review grants no merge, publication, activation, experiment or
owner operation, and does not count as a required check.

## Exact scope and pins

- Target: `4272291aa199c4f32a5bcecbfeedbfd76349c2db`; base `20f39c0`.
  `git log 4627b83..4272291` lists `d5a9543`, `20f39c0` and `4272291`. PR56
  head `4627b83` is an ancestor (`git merge-base --is-ancestor`).
- Reviewed delta: `git diff 20f39c0 4272291` over
  [PROGRESS](../PROGRESS.md),
  [ADR 0020](../adr/0020-linux-execution-hierarchy-and-janitor.md) and the
  [brief](../briefs/0020-linux-hierarchy-implementation.md) (3 files,
  +38/-14).
- Runtime: the working directory and `git rev-parse --show-toplevel` agreed
  on an isolated worktree separate from the canonical checkout. Status was
  clean at HEAD `4272291` before this report was written.
- Spec and standards: AGENTS.md, docs/KICKOFF.md (merge flow, v1 order) and
  the dytschgo-review axes. User authority: the explicit product-first
  instruction to keep building features instead of waiting on the hosted
  macOS runner, given that slow macOS runs stay within budget brackets.
  Existing merge gates are **not** waived.

## Spec

1. **Delivery-only change: PASS.** Every changed line covers one of three
   things: when local implementation may start, when publication or
   integration may happen, or the Stage F status. The technical fence
   statements stay intact: production `MechanismUnavailable`, no privileged
   operation or activation, M2 incomplete, and native paths
   **unverified on real hardware**. Brief gate 3 (no slice activates
   anything) is unchanged. No ADR or brief section outside the status line,
   the gates header and the new dated clarification is edited.
2. **Merge gates preserved: PASS.** The ADR status line, the new ADR section
   and brief gate 2 all require the contract PR, Stage C PR53 and Stage D
   PR55 to merge through all required gates before any implementation is
   published or integrated. They then require a re-pin and a re-review on
   the merged base. This matches KICKOFF's merge flow: required CI,
   `ci-time-budget` and a different-provider `agent-review`, with only the
   coordinator merging. Per-slice independent review stays in force through
   the slice table ("Sol builds, Opus reviews each").
3. **Grounded in user authority and precedent: PASS.** PROGRESS already
   records the product-first direction ("pending hosted CI does not stop
   local implementation"). It also records the Stage D precedent: prepared
   locally on an explicit unmerged Stage C dependency, with no gate claimed
   cleared. The earlier "no implementation starts until merge" wording was
   stricter than that direction. Loosening only the local start is
   consistent with it. No product approval flow is added.
4. **E1 scope: PASS.** The first local slice is E1, which is pure parsers and
   classifiers: byte and string inputs with no I/O, tested on all OSes. Its
   only change to prerequisite code is the bridge's `cgroup_parent` delegating
   to `locate`, which sits inside the declared Stage D stack. Nothing native,
   owner-side, cgroup-mutating or VM-related starts.
5. **Stage F: PASS.** PROGRESS and the ADR agree with the phase-1 report
   `afe5434`, retained as `20f39c0`. The historical E_PROTECTION failure was
   recovered, but no exact trace or safe replay exists. The site, the
   mutation phase, cleanup and the cause stay unknown, and there are no
   hypotheses, fix or retry. Diagnosis stops at phase 1. This review starts
   no diagnosis.

## Standards

- Encoding: the added lines are ASCII only, `git diff --check` is clean, and
  the three files are UTF-8 without a BOM. The brief's `E4–E7` became
  `E4-E7`, which is harmless.
- Links: `reviews/unix-protection-phase1-evidence.md` resolves from
  PROGRESS. No other links were added.
- Residual stale wording: no "no implementation starts" or "before any slice
  starts" phrasing remains in ADR 0020 or its brief. Similar wording in ADR
  0019 and older PROGRESS entries belongs to other, historical contracts and
  is out of scope.
- No smell-baseline findings apply to this docs-only delta.

## Advisories (non-blocking)

- **A1, publication path of this delta.** `4272291` sits on top of PR56's
  reviewed head `4627b83`, on a separate branch. If it is folded into PR56,
  that PR's exact-head review and required checks must be redone, as the ADR
  status line already says. If it is published separately, record which PR
  carries it so local E1 work can cite a pinned design source.
- **A2, "after independent design review".** The phrase is clear for the ADR
  0020 contract. Consider also saying that each local slice is pinned to the
  exact contract head it was built from (today `4627b83` plus this
  docs-only delta), so the later re-pin has a defined starting point.
- **A3, cosmetic.** The ADR status line "No privileged operation or
  activation is authorized. M2 remains incomplete and native paths remain"
  goes past the surrounding wrap width.

## Not run

No builds, tests, CI, GitHub, push, native, owner, service, account, cgroup,
VM, experiment or private-state actions were performed. No test results are
claimed. Only this report is committed.
