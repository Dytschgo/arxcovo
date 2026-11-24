# Coordinator CI administration

Protection is installed. The installation and bootstrap plans below are historical. ADR 0025 records the charged-v1 activation and the consumed exceptions.

## Review and protection

Apply `.github/main-protection.json` only after infrastructure installation, a real main CI baseline and review. Read the resulting API configuration back and compare it with this payload. It requires all three platform checks, ci-time-budget and agent-review, up-to-date branches, resolved conversations and linear history. Administrators are included; force pushes and deletion are blocked. GitHub Actions checks are pinned to App 15368, confirmed through the repository's check-runs API. Verify that the Actions token's budget status satisfies that source pin during activation.

After those prerequisites, the coordinator applies and reads back the configuration with:

```text
gh api -X PUT repos/Dytschgo/arxcovo/branches/main/protection --input .github/main-protection.json
gh api repos/Dytschgo/arxcovo/branches/main/protection
gh api -X PATCH repos/Dytschgo/arxcovo -F allow_auto_merge=true
gh api repos/Dytschgo/arxcovo --jq .allow_auto_merge
```

These commands are instructions for activation; their presence is not evidence they ran.

A PR is required with zero additional GitHub approving users: the same owner login cannot approve its own PR. The independent reviewer (separate Sol6.1 for non-UI under current routing) must actually read the exact base/head diff, recheck the current SHA, publish its findings and post agent-review success or failure on that SHA. New commits require a new review. Existing owner credentials can publish this status; no new App is required. Do not include private T3 links in public review comments. See the ADR for the trust limitation of shared owner credentials.

## Guarded auto-merge

The owner chose Dytschgo as the agent identity for now, including their own PRs under that login. `.github/agent-authors.json` is the trusted allowlist. Outsiders always wait for the owner; agents do not approve fork workflows.

After activating protection and enabling the repository setting, run from independently reviewed trusted source:

```text
node scripts/enable-auto-merge.mjs Dytschgo/arxcovo PR_NUMBER EXACT_HEAD_SHA
```

The command checks the allowlist, same-repository source, OWNER/MEMBER/COLLABORATOR association, open/non-draft state, main target, exact SHA and all required protection settings before requesting `gh pr merge --auto --squash --match-head-commit`. It never bypasses checks. Do not run candidate PR code with privileged credentials. Leave default Actions token permission read-only, Actions PR creation/approval disabled, and external workflow runs subject to owner approval.

## Measurement and records

The trusted default-branch policy runs on CI and label signals and retries
periodically. It reads PR head and labels live and publishes per-SHA statuses.
API reads are bounded. Discovery at 100 open PRs and transport errors fail closed.

[ADR 0025](adr/0025-ci-queue-outage-budget.md) defines the active charged-v1
budget. Valid green runs get credit only for intervals with a pending required
job and no executing job. Invalid timing metadata on a green run leaves the
wall-time rule in effect with the same 900 s threshold. Above that threshold,
`ci-speed` requires green, creditable measurements for main and the candidate,
with strict reductions in charged time and longest job execution. Candidates
use attempt 1 of the latest completed same-head, same-repository PR run.

With the runner recovery amendment, the latest main push CI run is
authoritative. An unfinished run keeps the status pending, and a missing
baseline fails. A completed non-green baseline blocks even below 900 s unless
it satisfies the recovery rule below. Status descriptions identify the
measurement as `Main run {id}: attempt {n}:`. A change in run or attempt
invalidates old success before evaluating the current head. Unchanged
statuses are deduplicated, and a failed update on one head does not stop
evaluation of others.

`--record` appends completed measurements by run ID with charged-v1 fields and
validated, sorted job timestamps. Duplicate IDs and existing rows are preserved.

## Historical installation acceptance plan

After the reviewed installation merge: measure main CI, commit the first baseline, install/read back protection and settings, demonstrate an allowlisted trivial PR auto-merging after all gates, and demonstrate a slow-main probe remaining blocked plus measured ci-speed recovery. Unit tests do not prove this end-to-end acceptance. This historical plan is not a statement of current installation status or a new grant.


<a name="proposed-adr0025-bootstrap-not-authorized"></a>

## Historical ADR 0025 bootstrap proposal

This section records the original proposal. ADR 0025 records the completed
activation and consumed exception. For current operations, see
[Recovering a main CI budget measurement](#recovering-a-main-ci-budget-measurement).

The owner accepted ADR0025's budget-definition change on 2026-11-16. At
the time of this proposal, activation and the one-use bootstrap route
still required separate authorization.
The historical2026-11-16 mainc612152 snapshot required all three verify jobs
(App15368), agent-review (no app pin), and ci-time-budget (App15368), with
strict=true and enforce_admins=true. The amendment cannot satisfy its own old
budget gate and is not a ci-speed PR. ADR0022's prior exception is exhausted.

The proposal called for the coordinator to present the amendment PR's full
head SHA and results to the owner after an exact-head independent PASS
and three successful platform jobs. The proposed route required separate
permission for a named executor and applied only to that pinned amendment PR.

If separately authorized, the executor must first confirm that no open PR has an armed auto-merge request,
then snapshot the live full protection configuration and pin the full reviewed PR head, PR base SHA and actual main SHA (all must match the approved decision), rechecking all four other gates. The prepared base/main for that proposal was 3022d601f09f24b0e5f092a33f2e9343b500471d.
Check again for zero armed auto-merges immediately before the pinned merge; abort if any is armed. Keep the
protection window short and perform no unrelated action within it. Then temporarily
remove ONLY ci-time-budget from the required-check list, and perform the pinned
squash merge. Restore original strict/admin enforcement and the original five context/app bindings in a
finally block even if the merge fails, then read back and compare them exactly. Send every check with an
explicit app_id: retain15368 for Actions checks and encode the snapshot null binding as-1 for agent-review.
GitHub documents-1 as allowing any app; do not omit app_id and let the API infer a recent app. Root PR164 before/after protection snapshots were read locally and are byte-identical; both show original agent-review app_id=null. The authorized PR164 write used-1 (any app), whose resulting null readback is verified there; see refresh evidence for hashes. A fresh mismatch still stops work for owner recovery.
Leave admin enforcement, reviews, linear history, force-push/deletion fences and
all other protection untouched. Abort on drift; do not use manual success status,
force push, an admin bypass or a ci-speed label. A restoration failure stops all
work for owner recovery. No command or protection update has run from this plan.

Verify the actual merge SHA/tree, the first trusted policy evaluation, and CI on
that exact new main. Failed main gets no idle credit; no other merge follows
until a new first-attempt green exact main and normal required gates pass. The failed905s attempt1 remains uncredited even if attempt2 is functionally green. Record the approved scope,
executor, before/after settings and result in PROGRESS and ADR0025. Any further
exception requires a new decision; the route is not reusable.

Source for explicit app binding: [GitHub protected-branch status-check API](https://docs.github.com/en/rest/branches/branch-protection#update-status-check-protection).


## Historical owner acceptance checkpoint - 2026-11-16

The owner explicitly accepted ADR0025's budget definition: exclude verified idle queue gaps
only for fully green runs, retain the 900-second threshold and report total wall time.
This supersedes statements above that definition acceptance is still pending. It grants no
protection change or merge permission. The named-executor bootstrap decision remains separate.

The owner also authorized one infrastructure-only retry of cancelled macOS job133431908247
on old head a26de1b. Readback confirms runner_id=0 and zero steps. That job is superseded:
then-current PR163 head78c72b6 already had successful CI43914905394 on attempt1, all platforms green.
No obsolete-head retry was performed and no attempt2 result is claimed.

## Current-main preparation refresh - 2026-11-17

PR174 merged as3022d601; PR CI44123480876 all five green. Main44126173935
attempt1 is the905s uncredited infrastructure baseline, not a runtime R5 failure.
Only root's single zero-step macOS retry is allowed; no further retry or partial
baseline measurement. See [refresh evidence](evidence/ci-idle-queue-main-refresh.md)
for the single final read (attempt2 completed/success on exact3022d601), source
identity, checks and unchanged activation limits.
A new first-attempt fully green main after eventual activation is still required.

### Recovery measurement clarification and next-merge priority

Run44126173935 recovered over1352s from first start to final job completion.
Its two-attempt executing-job union is901s (Ubuntu overlaps Windows553s, then
macOS348s), not a charged-v1 measurement or a claim that all idle time qualifies.
Trusted current policy reads failed attempt1 at905s; ADR0025 would withhold
credit for that failed attempt. No retroactive pass is promised. #163 is the
next merge candidate, ahead of all features, subject to the reviewed pinned
owner-authorized route above and fresh qualifying main after activation.

### Parallel validation proposal, 2026-11-18

[ADR0031](adr/0031-parallel-ci-validation.md) keeps the three protected `verify`
contexts as fail-closed completion gates over parallel Rust and UI matrices.
No branch-protection edit is needed. The first source head passed every job but
was held at 949 seconds wall with no credit from the unchanged legacy validator.
A prospective exact-nine-job validator adaptation retains charged-v1's existing
900-second threshold and interval arithmetic, strict identities/times, full wall
reporting and conservative fallback; gates cannot be created before all workers
finish. Fresh exact-source review and a normal new-head first attempt are needed.
The held result is not retroactively credited; no speedup or PR182 release is
established yet.

## Recovering a main CI budget measurement

Open the latest main push CI run identified by the budget status. If its
first attempt was non-green and the budget needs a fresh measurement,
choose **Re-run all jobs** on that same run. Policy measures the latest
full rerun if it is green and has valid job timings. GitHub carries earlier
job timings into a partial rerun, so a partial rerun cannot supply the
measurement. A green first attempt still supplies the original timing.
[ADR 0025 recovery rule](adr/0025-ci-queue-outage-budget.md#main-runner-cancellation-recovery-2026-11-24)

An unfinished attempt waits for up to 60 minutes from its reported start
if running, or its last update if queued. For `stale-main-run` or
`invalid-main-run-timestamp`, inspect the attempt in Actions. If the run
is stuck, cancel it, wait for it to complete, and choose **Re-run all jobs**.
If GitHub cannot cancel or rerun it, investigate the Actions outage or
escalate to the coordinator. The budget check stays blocked until it can
evaluate the authoritative run.
