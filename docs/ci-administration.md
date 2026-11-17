# Coordinator CI administration

Historical installation instructions below are not an activation grant. Protection is already installed. PR163 refresh is preparation only: the owner accepted ADR0025's definition, while activation/bootstrap/protection/merge remain unauthorized. ADR0022/#57 and PR164 exceptions are consumed.

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

Policy runs trusted default-branch code on workflow_run for CI and label signals, with a periodic retry. It never executes candidate code. API calls are bounded; discovery at 100 open PRs fails closed and needs pagination before scaling. Head and labels are read live; statuses are per SHA. Attempt 1 supplies the wall-clock measurement so a partial rerun cannot appear faster. The latest completed main push run is authoritative. Missing baseline fails. The currently active wall policy permits only a successful faster same-head/repository ci-speed candidate when main exceeds 900 seconds.

**Owner-accepted amendment, conditional and not active:** [ADR0025](adr/0025-ci-queue-outage-budget.md) retains wall seconds and adds charged-v1. A validated sweep credits only intervals with a created pending required job and no executing job. Execution, overlapping waits, prefix/tail and no-pending idle remain charged. The exact three completed platform jobs must match attempt 1, run/head, names, labels and strict timestamp ranges. One bounded jobs page is read; invalid metadata withholds credit with run-invalid/jobs-invalid, using the original wall gate. Transport/8 MiB overflow still fails closed. Failed runs or jobs retain computed measurements as not-green but receive no credit. A green valid baseline may use charged time at the unchanged 900s threshold. Above it, ci-speed requires BOTH baseline and candidate green/creditable and strictly reduced charged time AND longest execution. These metrics alone do not prove a source speedup or exclude runtime noise.

Historical main43880904887 was RED: wall1475s/charged738s/Q737s/longest372s, credit withheld. PR162 run43889323450 is green: wall1311s/charged877s/Q434s/longest585s. It cannot qualify for ci-speed against this failed main. Current main3022d601 run44126173935 attempt1 failed at905s due to zero-step macOS infrastructure cancellation; the single final read verified root's macOS-only attempt2 completed/success on exact3022d601. Functional health is restored; its outcome cannot credit attempt1. The separate exact-main health prerequisite remains binding.

Status publication compares the current context's state and description and skips unchanged results. Unchanged heads are not cycled through pending on periodic checks. The `Main run {id}:` marker remains intact; descriptions report wall, charged or unavailability, and verdict within 140 characters. A changed main measurement invalidates old successes; errors for one head do not prevent evaluating the others. The evaluator reports aggregate failure if a head cannot be updated.

The 30 existing duration-history records remain untouched and wall-only. After separately authorized activation, `--record` may append new run ids with charged-v1 fields and validated sorted job timestamps; duplicate ids are not rewritten. Preparation executes only synthetic VM record tests with intercepted filesystem/API operations, never live `scripts/ci-budget.mjs` or `--record`. A GitHub API outage can prevent status updates; evaluation fails rather than claiming to update unreachable heads.

This is preparation only, pending root review and independent Sol6.1 high source/exact-head review under current routing. The owner already accepted KICKOFF's definition deviation. Activation, bootstrap, status/label/protection changes and merges remain unauthorized. Historical installation text and consumed ADR0022/#57 or PR164 exceptions grant nothing for this refresh.

## Historical installation acceptance plan

After the reviewed installation merge: measure main CI, commit the first baseline, install/read back protection and settings, demonstrate an allowlisted trivial PR auto-merging after all gates, and demonstrate a slow-main probe remaining blocked plus measured ci-speed recovery. Unit tests do not prove this end-to-end acceptance. This historical plan is not a statement of current installation status or a new grant.


## Proposed ADR0025 bootstrap (not authorized)

The owner accepted ADR0025's budget-definition change on 2026-11-16. Activation and the one-use bootstrap route remain unauthorized. The earlier preparation-only wording is historical.
The historical2026-11-16 mainc612152 snapshot required all three verify jobs
(App15368), agent-review (no app pin), and ci-time-budget (App15368), with
strict=true and enforce_admins=true. The amendment cannot satisfy its own old
budget gate and is not a ci-speed PR. ADR0022's prior exception is exhausted.

After this amendment PR has an exact-head independent PASS and three successful
platform jobs, present its full head SHA and results to the owner. The proposed
one-use route requires separate activation permission for a named root executor to bootstrap ONLY that pinned amendment PR.
Definition acceptance is already settled; do not ask for it again. It grants nothing for PR162, PR159 or feature work.

If separately authorized, the executor must first confirm that no open PR has an armed auto-merge request,
then snapshot the live full protection configuration and pin the full reviewed PR head, PR base SHA and actual main SHA (all must match the approved decision), rechecking all four other gates. Current prepared base/main is3022d601f09f24b0e5f092a33f2e9343b500471d; any later drift requires renewed assessment.
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
