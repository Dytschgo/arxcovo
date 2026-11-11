# 0002: Install and activate merge gates

Status: installed and enforced after reviewed PRs #1–2. The trivial auto-merge acceptance passed in PR #3; slow-main rejection and measured recovery are still under test in PRs #4–6.

The repository begins without workflows or a main CI baseline. Install the infrastructure through PR #1 after actual three-platform CI and an independent other-provider review pass. Bootstrap protection requires those available CI/review checks before installation; add the budget requirement after the first real main baseline. Record this first merge as an installation merge before budget enforcement, not milestone acceptance. No status is fabricated and no installed protection is weakened.

Then measure the resulting main CI, commit its duration record, apply and read back the reviewed protection payload, and enable repository auto-merge. Every later merge must satisfy the platform CI checks, ci-time-budget and agent-review. Exercise a trivial PR through auto-merge and a deliberately slow main baseline through rejection and ci-speed recovery before declaring setup complete.

Independent reviewers publish agent-review for the exact SHA using the existing authenticated GitHub account. Both providers currently share that account; provider independence is established by the actual separate model review and its evidence. Owner-level credentials are trusted and can change repository policy. This is not cryptographic proof of model identity. Outsiders lack repository write/status/merge permission; their PRs are never eligible for coordinator auto-merge. Fork workflow approval remains an owner action.

The owner explicitly selected Dytschgo as the current agent author. The trusted allowlist is .github/agent-authors.json; this also permits the owner's own PRs under that login. Execute the guarded command only from reviewed trusted source. A dedicated account/App is an optional future strengthening, not a bootstrap prerequisite.

Use trusted default-branch workflow_run policy evaluation, a label-change signal and periodic retry. Avoid pull_request_target: GitHub announced public-repository default blocking from December 2, 2026. Policy jobs serialize through a cancellation group and read current head/labels immediately before publishing. GitHub has no atomic label/status transaction; evaluation is retriggered after label changes.

History is an independently maintained committed record, not another merge condition. Preserve the kickoff's strict 900-second threshold and any successful measured reduction for ci-speed; no extra 10% margin is imposed. Measure attempt 1 to avoid making partial reruns appear to improve performance. Keep existing explicit matrix checks instead of introducing a redundant aggregate.

Status publication skips identical current results. New heads and old successes measured against a different main run become pending; unchanged final results do not churn. Measurement and per-PR failures publish failure and one bad head does not stop the others. Only a discovery failure invokes the triggering-head fallback, which awaits status publication. If GitHub discovery/status APIs themselves are unavailable, unavailable heads cannot be updated; the workflow fails and the scheduled evaluator retries. No stronger availability guarantee is claimed.

Sources: [GitHub trigger change](https://github.blog/changelog/2026-11-02-pull_request_target-blocked-by-default-december-2/), [branch protection API](https://docs.github.com/en/rest/branches/branch-protection), [workflow attempts API](https://docs.github.com/en/rest/actions/workflow-runs#get-a-workflow-run-attempt).
