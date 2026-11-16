# Coordinator CI administration

Implementation and settings are awaiting independent review. No protection or auto-merge setting has been applied. Follow docs/adr/0002-merge-gate-bootstrap.md for the initial installation exception and subsequent acceptance sequence.

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

A PR is required with zero additional GitHub approving users: the same owner login cannot approve its own PR. The independent other-provider reviewer must actually read the exact base/head diff, recheck the current SHA, publish its findings and post agent-review success or failure on that SHA. New commits require a new review. Existing owner credentials can publish this status; no new App is required. Do not include private T3 links in public review comments. See the ADR for the trust limitation of shared owner credentials.

## Guarded auto-merge

The owner chose Dytschgo as the agent identity for now, including their own PRs under that login. `.github/agent-authors.json` is the trusted allowlist. Outsiders always wait for the owner; agents do not approve fork workflows.

After activating protection and enabling the repository setting, run from independently reviewed trusted source:

```text
node scripts/enable-auto-merge.mjs Dytschgo/arxcovo PR_NUMBER EXACT_HEAD_SHA
```

The command checks the allowlist, same-repository source, OWNER/MEMBER/COLLABORATOR association, open/non-draft state, main target, exact SHA and all required protection settings before requesting `gh pr merge --auto --squash --match-head-commit`. It never bypasses checks. Do not run candidate PR code with privileged credentials. Leave default Actions token permission read-only, Actions PR creation/approval disabled, and external workflow runs subject to owner approval.

## Measurement and records

Policy runs trusted default-branch code on workflow_run for CI and label signals, with a periodic retry. It never executes candidate code. API calls are bounded; discovery at 100 open PRs fails closed and needs pagination before scaling. Head and labels are read live; statuses are per SHA. Attempt 1 supplies the wall-clock measurement so a partial rerun cannot appear faster. The latest completed main push run is authoritative. Missing baseline fails. The currently active wall policy permits only a successful faster same-head/repository ci-speed candidate when main exceeds 900 seconds.

**Prepared amendment, conditional and not active:** [ADR0025](adr/0025-ci-queue-outage-budget.md) retains wall seconds and adds charged-v1. A validated sweep credits only intervals with a created pending required job and no executing job. Execution, overlapping waits, prefix/tail and no-pending idle remain charged. The exact three completed platform jobs must match attempt 1, run/head, names, labels and strict timestamp ranges. One bounded jobs page is read; invalid metadata withholds credit with run-invalid/jobs-invalid, using the original wall gate. Transport/8 MiB overflow still fails closed. Failed runs or jobs retain computed measurements as not-green but receive no credit. A green valid baseline may use charged time at the unchanged 900s threshold. Above it, ci-speed requires BOTH baseline and candidate green/creditable and strictly reduced charged time AND longest execution. These metrics alone do not prove a source speedup or exclude runtime noise.

Main43880904887 remains RED: wall1475s/charged738s/Q737s/longest372s, credit withheld. PR162 run43889323450 is green: wall1311s/charged877s/Q434s/longest585s. It cannot qualify for ci-speed against this failed main. The separate exact-main health prerequisite still blocks feature merges.

Status publication compares the current context's state and description and skips unchanged results. Unchanged heads are not cycled through pending on periodic checks. The `Main run {id}:` marker remains intact; descriptions report wall, charged or unavailability, and verdict within 140 characters. A changed main measurement invalidates old successes; errors for one head do not prevent evaluating the others. The evaluator reports aggregate failure if a head cannot be updated.

The 30 existing duration-history records remain untouched and wall-only. After separately authorized activation, `--record` may append new run ids with charged-v1 fields and validated sorted job timestamps; duplicate ids are not rewritten. Preparation executes only synthetic VM record tests with intercepted filesystem/API operations, never live `scripts/ci-budget.mjs` or `--record`. A GitHub API outage can prevent status updates; evaluation fails rather than claiming to update unreachable heads.

This is preparation only, pending root review and independent Opus high exact-source/security review before publication. The old bootstrap/installation text elsewhere in this document is historical, stale planning material, not a new grant. ADR0022 is exhausted. User preparation does not accept KICKOFF's wall-clock deviation or authorize activation, bootstrap, status/label/protection changes or merges. A concrete separately authorized bootstrap route remains unresolved.

## Acceptance remaining

After the reviewed installation merge: measure main CI, commit the first baseline, install/read back protection and settings, demonstrate an allowlisted trivial PR auto-merging after all gates, and demonstrate a slow-main probe remaining blocked plus measured ci-speed recovery. Unit tests do not prove this end-to-end acceptance. Nothing has merged yet.
