# Owner Sol routing independent delta recheck

- Date: 2026-11-17.
- Reviewer: independent GPT-6.1 Sol, high effort; no children.
- Builder: root Sol; separate reviewer, not cross-provider independence.
- Base: 3084f8e5ef87f9aaa11e12e780a5adfe62e87f6a.
- Original target: d0f431a41ac1763e7dace3cdb9f796c26652730f.
- Retained review: 5b25560dec94174962829b189a07d2163a15dfae.
- Recheck target: 04060273b315eec847c31ff4f522e770063a05f4.
- Exact delta: retained review to recheck target; direct parent confirmed.
- Scope: docs/KICKOFF.md and docs/PROGRESS.md delta; prior review retained.
- Result: PASS for preparation; R1 and S1 closed, no new findings.

## Standards

- Read repository AGENTS and applied dytschgo-review's separate axes.
- Inspected actual pinned Git blobs and both routing and correction diffs.
- Delta is four insertions and two deletions across the two named documents.
- No production, workflow, dependency, quarantine or gate file changed.
- Documentation-only correction introduces no applicable code-smell finding.
- S1 closed: KICKOFF stop condition now stops when authorized attempts are
  exhausted, preserves counters and directs work to other unblocked tasks.
- This no longer requires three attempts by each provider. AGENTS and the
  routing stuck rule still forbid another attempt after an exhausted budget.
- PROGRESS retains Sol3/3 and Opus0/2; unused legacy Opus capacity does not
  authorize Claude non-UI repair work or reset the exhausted Sol budget.

## Spec

- R1 closed: KICKOFF restores the existing five-hour automatic resume policy
  when usage limits block all authorized work, with WIP committed and a
  PROGRESS resume note. Resume must follow the owner-approved model routing.
- Review stays pending if an authorized independent reviewer cannot run.
- The babysitter's scheduled-resume check now agrees with this policy.
- Owner routing remains Sol 6.1 for engineering, architecture, security and
  reviews; Claude only for UI. A separate Sol non-UI reviewer is valid.
- Independent review, exact-head evidence, required checks, coordinator-only
  merge and maximum two builders plus one reviewer remain explicit.
- Original babysitter setup and oversight sections are byte-identical to
  base. Its prompt differs only in the owner-approved reviewer-routing rule.
- No source change to fix budgets, quarantine, runtime fences or gate logic.
- PROGRESS records that no actual schedule was created, cancelled or changed.
  This local review made no scheduling call; live schedule state was not queried.

## Actual checks and limits

- Ran local Git rev-parse, show, log, diff, ls-files, ls-tree and initial
  working-tree inspection; used Python for bounded static assertions.
- git diff --check passed for 5b25560..0406027 and 3084f8e5..0406027.
- Nine pinned AGENTS/KICKOFF/PROGRESS blobs passed strict UTF-8, no-CR and
  final-LF checks; exact delta paths and parent identity passed assertions.
- Full base-to-target inventory contains only AGENTS, KICKOFF, PROGRESS and
  the retained review; retained review bytes are unchanged at the target.
- Initial read used a nonexistent retained-report name; read the actual
  owner-sol-routing-review.md from 5b25560 afterward. No finding uses that error.
- No builds, test suites, network, hosted checks or hardware were exercised.
- Hardware remains **unverified on real hardware**.
- Main CI 44077792712 is pending root wait per supplied context, not queried;
  no green CI claim, publication authority or merge authority is granted.
- Ready: documentation recheck complete; coordinator still owns pending CI
  and all normal publication/merge gates. No review blocker remains here.
- Findings: Standards 0 (S1 closed); Spec 0 (R1 closed); no security blocker.
