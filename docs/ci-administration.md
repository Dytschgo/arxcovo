# Coordinator CI administration

No protection, reviewer integration or auto-merge configuration was applied by the implementation agent.

Proposed main protection:

- Require PRs, one independent approving review, dismissal of stale reviews, last-push approval, resolved conversations and up-to-date branches.
- Required checks: `verify (ubuntu-latest)`, `verify (windows-latest)`, `verify (macos-latest)`, `ci-time-budget`, `agent-review`.
- Pin GitHub Actions as expected source for CI and budget. Pin a separately controlled reviewer GitHub App as expected source for agent-review. Shared builder credentials cannot establish reviewer independence.
- Block force pushes and deletion; enforce protections for administrators. Restrict direct main writes to the coordinator/bootstrap identity. Independently review workflow and policy changes.
- Reviewer reads the exact base/head diff, verifies Codex Sol medium builder versus Claude Opus 5.5 high reviewer, and posts agent-review on that exact SHA with linked review evidence. Fallback grok-4.7 high applies only on provider limits. Never fabricate success for bootstrap.

Keep repository auto-merge disabled until coordinator-owned automation is installed. Only coordinator automation may enable auto-merge, after checking the PR author against an explicit owner-agent allowlist, same-repository head and OWNER/MEMBER/COLLABORATOR association. Initial allowlist is empty: the owner's ordinary login alone does not prove agent authorship. Outsiders always wait for the owner. `eligible` in scripts/policy.mjs defines and tests the predicate; it does not merge anything.

## Baseline and activation

Main initially contains only the settled documents. It has no CI workflow or duration baseline. This is an unresolved bootstrap dependency: trusted policy cannot activate until installed on main, and absent baseline fails closed. Coordinator must reconcile a reviewed bootstrap procedure before acceptance. This implementation does not bypass gates.

After a completed main CI run, use an already authenticated environment to set GITHUB_REPOSITORY to Dytschgo/arxcovo and run `node scripts/ci-budget.mjs --record`. Commit docs/ci-duration-history.json in the next PR. Empty history is intentional; no duration is invented. The baseline is the latest completed main push run, including failed runs. Wall-clock is workflow start to completed update, not sum of parallel jobs. Candidate CI must succeed at the exact PR head for a ci-speed exception and take less time than the slow baseline. workflow_run re-evaluates open heads on completion. API queries are bounded to 100 entries; larger histories need pagination before scaling.

The policy workflow checks out only the trusted default branch, does not install PR packages, and reads candidate history through the API as bounded JSON. Its write permission is limited to commit statuses.

Acceptance still needed: install protection and reviewer integration, establish and commit measured baseline, run a trivial owner-agent PR through every gate and coordinator auto-merge, and prove slow-main blocking. Local policy tests alone do not establish end-to-end acceptance.

Primary references consulted: [workflow runs API](https://docs.github.com/en/rest/actions/workflow-runs), [nextest installation](https://nexte.st/docs/installation/pre-built-binaries/).
