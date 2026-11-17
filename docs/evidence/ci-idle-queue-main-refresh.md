# Queue-outage policy current-main refresh

2026-11-17; Sol6.1 high implementation sub-agent; local preparation only, no children.

## Pins and integration

- Assigned worktree branch `t3/refresh-ci-queue-policy`; initial/main/base
  `3022d601f09f24b0e5f092a33f2e9343b500471d` (PR174, reviewed e4f0d48).
- Existing PR163 branch `work/ci-queue-outage-policy`, old reviewed source/head
  `7aefa2cae0abadda594f2a7f4bad02fd8241a92a`. Parent read PR163 open on that head before assignment;
  this child made no PR read/write, push, comment, status, label or protection action.
- Merged old head locally into assigned current-main base. Only PROGRESS conflicted:
  retained current main's complete Task history verbatim, historical PR54/#57 labels
  and milestones; refreshed current health and added this policy checkpoint plus
  historical owner acceptance. No root-owned query design or unrelated feature docs
  imported from old head. No runtime conflict, correction or semantic change.
- All nine runtime/test/fixture bytes equal old7aefa2ca, checked against git blobs.
  Workflows, actual protection payload, permissions/action pins/timeouts/quarantines,
  dependencies and duration history match prepared main. History SHA256 `3b9d9d5494c8aea0ef715348f7e407b1b8cd50016a2c80d8e2776b90ade82e51`.

## Actual checks and single main-health read

Static Python assertions pass for all nine byte identities, allowlisted diff,
protected paths/history equality, strict UTF8 with LF/no BOM/conflict markers,
and preservation of main's full task history. `git diff --check` passes.

Exactly one live GET of run44126173935 immediately before synthetic tests returned
id44126173935, head `3022d601f09f24b0e5f092a33f2e9343b500471d`, attempt2,
status completed / conclusion success. Sanitized local read SHA256 `01ae41cfa01714df4627ec7134de8fb0860455f8d613022ef9274eceae9409be`.
No jobs/log read, polling, wait or further retry was performed by this child.

Root's supplied attempt1 evidence was read locally:904s wall, Ubuntu248s and
Windows553s success; macOS job134190447393 cancelled before runner assignment,
runner_id0/steps[], no-hosted-runner annotation. Root's single macOS-only retry
job134201720165 acquired a runner; carried Ubuntu/Windows job IDs134201723218/
134201723989 retain original times and runner IDs, without re-execution (parent
handoff evidence, not independently re-fetched jobs by this child). Functional
health is restored by attempt2; no first-attempt-green claim. Failed904s attempt1
is still the budget baseline, uncredited under inactive ADR0025. No partial
baseline measurement. PR174 CI44123480876 all5green/Windows541s/all108 tracked
cases allOS/Unix28fixtures/new founder-preproof allOS is parent-supplied evidence.

`node --test scripts/*.test.mjs` ran ONCE (explicit sorted expansion of that glob),
only after that successful exact-head attempt2 read: **48/48 pass,0fail,0skip**,
8105.1264ms, exit0. Synthetic VM gh-stub/FS interception only; no live
ci-budget.mjs or --record. Log SHA256
`0a23148f0c28bfd40d781c3bb62edb51b40e12fcc2e2d863e507f50bed6cf8e6`.
Python then failed printing a Unicode success glyph to the cp1252 console;
its wrapper exit1 is a log-display error after Node exit0. Read saved UTF8 log
with escaped console output to confirm totals, no test fix or rerun.
Lint unrun: node_modules absent; no install. Rust/engine/workflow/native tests
unrun. No cache deletion, admin/bootstrap or hardware operation.

## Source SHA256 (actual old-head equality)

| Path | SHA256 |
| --- | --- |
| scripts/ci-budget.mjs | `7e532f28f6237d7b72396852ce33db68bd87084b8bd30f0e49fd2ed7e25719a6` |
| scripts/ci-budget.test.mjs | `93ae0b07fd41f29dd60ae537e83d6c0f9274b53db544d9dd095f85aceeb1ee6c` |
| scripts/ci-workflow.test.mjs | `a2fe15dc2794456b3838dde627e2a2e7cdf2372b54a0ac04cdc9c21d324780a3` |
| scripts/evaluate-budget.mjs | `a3feae8f74c2507af0dfb5ac5a71300492d4490b57f66a0fdc495768cf7eda05` |
| scripts/evaluate-budget.test.mjs | `a9e2c57fc6aa2dddc4efb0c9a0c76fefb9fa0537025c3e8453027e78e2fa96e6` |
| scripts/fixtures/ci-budget-buffer.mjs | `f1d67deedd0389fc2766918e6b52d0ae40be85933a9acaf8384c8cb911a865e5` |
| scripts/fixtures/ci-queue-credit-runs.mjs | `9ef25a64cb17f191ebc03a39adba1e1e2aaba1bb3412a25a7c4d9c907489622e` |
| scripts/policy.mjs | `6e9911bfaf8c4076583168d896c85b999e0c15593b5cfab3ff98688d0e192480` |
| scripts/policy.test.mjs | `9df77240554a900f2e81ef5d10b43ab430bf2d240f3d20d5a5b229d42956c312` |

## Reused evidence, not rerun

Retained design ADR0025, Sol contract review, independent Opus review and S1/A2
recheck remain historical evidence, with no new review claim. Old preparation
statements saying Proposed/unaccepted/R5 current or readback unverified describe
those old revisions; current owner acceptance/health/readback supersede them.
The current refresh changes only documentation beyond the old reviewed policy.

Geometry/green-credit/metadata mutants are reused from
[prior evidence](ci-idle-queue-policy.md), whose restored policy hash
`6e9911bfaf8c4076583168d896c85b999e0c15593b5cfab3ff98688d0e192480`
is unchanged. S1 completion correction and A2 marker mutant evidence are reused
from the [bounded recheck](../reviews/ci-idle-queue-policy-opus-recheck.md),
with ci-budget hash `7e532f28...19a6` still identical. Tests differ from that
recheck only by the previously reviewed globalThis.structuredClone lint correction
in7aefa2ca; all current test bytes equal7aefa2ca. Historical root full Node48/48
log `6a8ceaaee1e911795d0fa9f570173780cbec1b8647c918ff29a78d8032827405`
is reused as provenance, not a new run. No old mutant was rerun.

## Activation boundary and readback provenance

Owner accepted the definition on2026-11-16; implementation remains unmerged and
inactive. Existing PR164 budget-only exception consumed at39b3dd7; ADR0022/#57
also exhausted. No reusable exception or new grant. Only root may present a
concrete reviewed exact head after three platforms plus independent review pass,
and ask for explicit activation/bootstrap permission naming the root executor.
The plan in [administration](../ci-administration.md#proposed-adr0025-bootstrap-not-authorized)
pins exact head/base/main, removes ONLY ci-time-budget temporarily, requires zero
armed auto-merges before the window and before merge, retains admin enforcement,
restores original strict/all five app bindings in finally and compares exact
readback. Drift or restoration failure aborts; no manual success/ci-speed/bypass.

Actual root PR164 local before/after protection snapshots are byte-identical,
SHA256 `41e2ed73ab5516bb5df0c7cfd409b9e41000bf4158e7aa6639fd3bac6c78ef41`;
both contain strict=true, enforce_admins.enabled=true, four Actions bindings15368
and original agent-review app_id=null. Root authorized-action record SHA256
`5e4cb8e3a94d11fdcf036f3dff7c5b929877f7e17bd3b126cae711bb7a69190e`
contains restore strict=true and explicit agent-review app_id=-1. This verifies
the historical-1/null restoration readback; no protection mutation here. Root's
attempt1 infrastructure evidence SHA256
`03083b620918e4867aa8f022173a9a77a29eed4f824c5d6669ac0893a4e70bd3`;
retry evidence `9fda8b4dfbb812aae21c57ca346fadae69460059854d4e6dee423fde59edc575`.
Only sanitized facts/hashes retained, no raw owner payloads committed.

A new first-attempt fully green main after eventual activation is still required;
attempt2 success cannot credit904s failed attempt1. M2 open / M3 unchecked,
Sol3/3 and historical Opus0/2 unchanged. Windowsfixture28 unverified on hosted
Windows; non-gating sample is not acceptance. Native paths **unverified on real
hardware**. Ready for root inspection and independent Sol source/exact-head reviews,
not activation or merge. No public action taken.

## Root current-baseline correction and inspection (2026-11-17)

Root Sol6.1 medium independently read attempts/1 after recovery and called only
trusted main's pure duration() on the saved payload:905s, conclusionfailure.
The earlier904s top-level completed-run snapshot remains historical evidence;
current policy/activation docs now use the905s authoritative baseline. No live
policy/status/history write or new test attempt. Recovery metadata/log hashes:
D:/Temp/arx-pr174-hosted/main-recovery-success.json. All108 tracked cases pass
each OS; Unix fixture28 pass. MacOS's first execution348s passed on infrastructure
retry; original Ubuntu248s/Windows553s were carried, not rerun.

Root verified all9 policy/test/fixture working bytes equal7aefa2ca and six
protected committed blobs equal3022d601, including duration history. Initial
raw checkout check hit31 CRLFs versus LF in unchanged policy.yml. Its committed
blob4925b7f12bdebcd624024e64af5ce9e000743b67 is identical; normalized working
bytes match. No source fix. Lint ran once after that checker assertion (disclosed),
exit0 in6.922s; no lint or Node retry. Log SHA256
6cc523691fae5355c9180a5372974a6ae92b938911703530b5046d48c6704925.
Root identity/lint records: D:/Temp/arx-ci-queue-root-refresh.

Source remains unchanged. Activation route is still preparation only: exact
head review,3 green platforms then owner's named-executor one-use decision.
No activation, protection write, exception reuse or merge authority.
