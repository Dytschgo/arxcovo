# CI queue activation independent documentation review

Reviewer: OpenAI / gpt-6.1-sol / high, 2026-11-17; independent of builder.
Single reviewer, no children. Documentation-only scope; no merge authority.
Base: 10aa891f27906e0c73e48396c5c812cab7d4523a
Head: c01b92e85ca099b0f4a067eb720470a05b35e64a
Commits: 331706ad, 2c59b022, c01b92e8.
Reviewed PROGRESS.md, ADR0025 and evidence/ci-queue-activation.md under docs/.
Standards: AGENTS.md, CONTRIBUTING.md and KICKOFF owner routing update.
Spec: assigned activation checkpoint and ADR0025 charged-v1 definition.
Source policy has a separate earlier independent review; this is not its renewal.

## Standards

- P2: Restore the historical last-merged records in docs/PROGRESS.md:781 and
  docs/PROGRESS.md:788. Both now repeat "Last merged: PR163 as10aa891f" inside
  historical Stage C checkpoints. The base records PR54/79276b93 at the first
  location and PR57/f1340a4, its timestamp and consumed ADR0022 exception at the
  second. This broad replacement removes historical evidence and falsely moves
  the November 17 activation into the older Stage C sequence. Restore both full
  base lines; keep the new PR163 entry only in current state/new dated history.
  AGENTS.md requires accurate acceptance evidence and last-merged accounting.

No other standards finding. Fowler code-smell baseline has no applicable issue
in this documentation-only diff. Current work/Blockers replacements are confined
to current state; M2 open/M3 unchecked, Sol3/3 and historical Opus0/2 remain.

## Spec

No additional finding. Activation status and first-main budget claims agree with
retained evidence. Earlier dated preparation checkpoints are historical; ADR0025
explicitly marks earlier preparation/approval text historical in its new footer.
This does not grant another exception or a feature merge.

## Security

No documentation security finding. Protection-before and protection-after are
byte-identical, including strict=true, five contexts and their bindings, admin
enforcement and every other retained fence. The agent-review binding remains
null as originally configured; it is not newly app-pinned by this activation.
Owner authorization pins head22a09eae and the root executor digest below.
Approval is consumed. This review made no protection/status/network writes.

## Verification actually performed

- Local diff, commit list, clean starting worktree and diff whitespace check.
  Only the three assigned documentation files differ from base; scripts and
  docs/ci-duration-history.json have no diff.
- Rehashed all eight artifacts listed in docs/evidence/ci-queue-activation.md:
  every SHA256 matches, using bootstrap/ for executor outputs.
- Rehashed bootstrap-pr163.py without executing it; digest matches
  2b2866045120e8ba4d8cde357640aef0fbdc2662e5eb21363f14d108c4aaa7b9.
- Parsed authorization, merge result/readback and complete protection snapshots.
  Local git trees for merged10aa891f and reviewed22a09eae both equal
  bc4221f8bc0461f8add768c07506fab17fa94db9, matching retained readback.
- Independently swept raw attempt1 job timestamps in Python: sole credited
  interval [S,S+3); wall632, charged629, credit3, longest628 seconds.
  Windows628/macOS435/Ubuntu286 seconds; all three jobs completed successfully.
- Imported only pure scripts/policy.mjs functions in Node. duration() plus
  queueMeasurement() exactly matches main-charged-measurement.json, and normal
  budget() passes. No publisher, recorder or executor was invoked.
- main-first.json pins run44177880021, attempt1, main push10aa891f, success.
  postmerge-runs.json pins workflow_run policy44179754154 to the same SHA,
  completed/success; trusted-policy.log corroborates the normal evaluator step.
- hourly-retry-plan.json records merged=true and one of four extra requests used;
  remaining scheduled slots are intended no-ops, not claimed observed executions.

Evidence root: D:/Temp/arx-ci-queue-root-refresh. These are retained snapshots,
not a live-state attestation. No build, Rust, engine, hardware, live status,
PR, push, merge or network call was performed. Hardware remains
**unverified on real hardware**. Existing exhausted counters are unchanged.
Documentation corrections are reversible; restore the two historical lines
before acceptance. Only this report is committed by the reviewer.

Standards: 1 finding (P2 historical evidence overwritten); Spec: 0; Security: 0.
