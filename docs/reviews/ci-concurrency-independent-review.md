# CI concurrency independent review

Anthropic / claude-opus-5-5 / high, single local agent, 2026-11-12T04:07Z. Different-provider Standards/Spec review of the Sol medium change.

- Target: `406b5ef152f8e61bf2c1fa49783ced719a67d1f0` on `b5e65561b001a3616fa8a6623512e7ff8aa0e346`, branch `t3/91e1f863`, clean tree. Four files: `.github/workflows/ci.yml`, `scripts/ci-workflow.test.mjs`, `docs/PROGRESS.md`, `docs/reviews/actions-queue-reset.md`.
- Read: AGENTS.md, KICKOFF CI rules (lines 63–64, 136), `ci.yml`, `policy.yml`, `policy-signal.yml`, `scripts/ci-budget.mjs`, `evaluate-budget.mjs`, `policy.mjs`.

**Verdict: changes requested. Two blocking findings.**

## Blocking

**B1. Main and manual-dispatch runs share a group, and their pending runs can still be cancelled. This weakens `ci-time-budget`.**
`group: ci-${{ github.event.pull_request.number || github.ref }}` puts `push` to main and every `workflow_dispatch` on main into `ci-refs/heads/main`. `cancel-in-progress: false` protects only the *running* run. GitHub's documented concurrency behaviour allows one running and one pending run per group. A newly queued run cancels the existing pending one. So if push A is running, push B is pending, and push C (or a manual dispatch on main) arrives, B ends as `completed/cancelled`. Its duration is only its queue time.
`ci-budget.mjs` `getMainRun` takes the highest `run_number` among `branch=main&event=push&status=completed`. Neither it nor `duration()` nor `budget()` checks `conclusion`. The cancelled B would become the authoritative baseline at a few seconds, so a main run longer than 900 s would not block PRs. This is likely while the queue drains through several sequential merges.
The change also contradicts its own claims: the test name, the reset evidence ("Main and manually dispatched measurements are not cancelled automatically") and PROGRESS. Unverified on hosted runners: this comes from documented semantics and has not been reproduced.
Suggested fix: group only pull_request runs, and give every other event a unique group:

```yaml
concurrency:
  group: ${{ github.event_name == 'pull_request' && format('ci-pr-{0}', github.event.pull_request.number) || format('ci-run-{0}', github.run_id) }}
  cancel-in-progress: ${{ github.event_name == 'pull_request' }}
```

Update the regression test to match. Hardening the budget to ignore non-success main baselines (A4) belongs in a separate PR.

**B2. PROGRESS deletes the only committed record of these facts:**
- Main push runs `43088994955` (461 s) and `43091130349` (549 s). The last entry in `docs/ci-duration-history.json` is `42984281226`.
- The first reviewed disposable Linux experiment failed with `E_PROTECTION`. The reviewed-SHA variable was cleared and no retry was made.
- E1 (Windows owner API narrowing) is implemented locally, unpublished and unreviewed.
- #36 has three review blockers.

`git grep` at HEAD finds none of these in docs. PROGRESS line 94 still says "No privileged Linux experiment has run", which is now stale and contradicts nothing left in the file. AGENTS requires PROGRESS to keep acceptance evidence and blockers current. Restore them, for example under a dated history bullet.

## Advisories

- **A1. The test pins source text only.** It killed 4/4 in-scope mutations: cancel always, group by ref only, de-indented concurrency, renamed prefix. It does not catch an added job-level `concurrency: {group: global, cancel-in-progress: true}`, or `timeout-minutes: 60`. Consider asserting that there is no job-level concurrency and that the 12-minute limit is kept.
- **A2. The hosted evidence is stale, and no green should be claimed.** Read-only metadata at 04:07Z for the five fresh dispatches:
  - macOS succeeded on all five.
  - Windows is in progress on all five.
  - Ubuntu **failed** on main `43152187271`, PR36 `43152254254`, PR26 `43152261583` and PR34 `43152265161`. On main the failed step is `npx playwright install --with-deps chromium` (04:02:59→04:05:51Z); I did not open the step logs on the other three.
  - Ubuntu on PR9 `43152257543` is still in progress.

  These runs used the base ci.yml, not this change. The evidence file says only main and PR36 had jobs observed. The brief says all 15 jobs were observed in progress. These are snapshots from different times, so not necessarily a contradiction. Update the evidence before merge.
- **A3. Two triggers on the same PR branch use different groups.** A manual dispatch uses `ci-refs/heads/<branch>` and the PR's own runs use `ci-<n>`, so neither cancels the other. That is acceptable, but say so.
- **A4. Pre-existing gap:** the main baseline ignores `conclusion`. A manually cancelled or failed main push already counts as the budget baseline.

## Confirmed preserved

- The diff only adds three top-level lines. Triggers, `permissions: contents: read`, the matrix, `fail-fast: false`, `timeout-minutes: 12`, pinned action SHAs and all steps are unchanged.
- `policy.yml` keeps its separate `trusted-ci-policy` group and its permissions.
- Grouping across PRs is correct: groups are keyed by PR number, numbers cannot collide with `refs/…`, and fork PRs are covered.
- Cancelled PR runs still fire `workflow_run completed`. They cannot satisfy `ci-speed`, because the budget requires `conclusion === 'success'`.
- A `reopened` event on the same head replaces the run rather than dropping it.
- No speedup is claimed, and that is correct.

## Commands and limits

- `git rev-parse HEAD HEAD~1` matched both pins; `git status --short` was clean.
- `node --test scripts/*.test.mjs` (Node v24.18.0): **19/19 passed**.
- Six mutations of `ci.yml` were run with `node --test scripts/ci-workflow.test.mjs`, then restored. `git diff --quiet` confirmed a clean tree.
- `gh api` GET on runs and jobs for the five run IDs was read-only.
- Not done: no cargo, native or ignored tests; no network writes, cancellation, dispatch, push, PR, watch, merge or settings changes; `.git/acceptance/private-state.json` was not read. B1 was not reproduced on GitHub.
