# Coordinator CI administration

Implementation and settings are awaiting independent review. No protection or auto-merge setting has been applied. Follow docs/adr/0002-merge-gate-bootstrap.md for the initial installation exception and subsequent acceptance sequence.

## Review and protection

Apply `.github/main-protection.json` only after infrastructure installation, a real main CI baseline and review. Read the resulting API configuration back and compare it with this payload. It requires all three platform checks, ci-time-budget and agent-review, up-to-date branches, resolved conversations and linear history. Administrators are included; force pushes and deletion are blocked. GitHub Actions checks are pinned to App 15368, confirmed through the repository's check-runs API. Verify that the Actions token's budget status satisfies that source pin during activation.

A PR is required with zero additional GitHub approving users: the same owner login cannot approve its own PR. The independent other-provider reviewer must actually read the exact base/head diff, recheck the current SHA, publish its findings and post agent-review success or failure on that SHA. New commits require a new review. Existing owner credentials can publish this status; no new App is required. Do not include private T3 links in public review comments. See the ADR for the trust limitation of shared owner credentials.

## Guarded auto-merge

The owner chose Dytschgo as the agent identity for now, including their own PRs under that login. `.github/agent-authors.json` is the trusted allowlist. Outsiders always wait for the owner; agents do not approve fork workflows.

After activating protection and enabling the repository setting, run from independently reviewed trusted source:

```text
node scripts/enable-auto-merge.mjs Dytschgo/arxcovo PR_NUMBER EXACT_HEAD_SHA
```

The command checks the allowlist, same-repository source, OWNER/MEMBER/COLLABORATOR association, open/non-draft state, main target, exact SHA and all required protection settings before requesting `gh pr merge --auto --squash --match-head-commit`. It never bypasses checks. Do not run candidate PR code with privileged credentials. Leave default Actions token permission read-only, Actions PR creation/approval disabled, and external workflow runs subject to owner approval.

## Measurement and records

Policy runs trusted default-branch code on workflow_run for CI and label signals, with a periodic retry. It never executes candidate code. API calls are bounded; discovery at 100 open PRs fails closed and needs pagination before scaling. Head and labels are read live; statuses are per SHA. Attempt 1 supplies the wall-clock measurement so a partial rerun cannot appear faster. The latest completed main push run is authoritative. Missing baseline fails. Above 900 seconds, only a successful faster PR CI run at the same source repository/head with ci-speed passes.

Record completed main runs separately with authenticated `gh`, GITHUB_REPOSITORY=Dytschgo/arxcovo and `node scripts/ci-budget.mjs --record`; commit docs/ci-duration-history.json in follow-up work. History absence never blocks an otherwise fast PR. No timings are invented. A GitHub API outage can prevent status updates; the evaluator fails and retries, rather than claiming it can update unreachable heads.

## Acceptance remaining

After the reviewed installation merge: measure main CI, commit the first baseline, install/read back protection and settings, demonstrate an allowlisted trivial PR auto-merging after all gates, and demonstrate a slow-main probe remaining blocked plus measured ci-speed recovery. Unit tests do not prove this end-to-end acceptance. Nothing has merged yet.
