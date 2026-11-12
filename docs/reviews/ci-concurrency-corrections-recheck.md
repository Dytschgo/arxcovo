# CI concurrency corrections recheck

Anthropic / claude-opus-5-5 / high, single local agent, no subagents, 2026-11-12. This is a different-provider recheck of the Sol medium corrections to B1/B2 from [the independent review](ci-concurrency-independent-review.md). It is not permission to merge.

- Target: `f646589ea040708718d4fa23614f95df6b937c39`. Parents: `52be45c` (review report) and `406b5ef` (original patch). Base: `b5e65561b001a3616fa8a6623512e7ff8aa0e346`. Clean tree.
- Base→HEAD source changes: `.github/workflows/ci.yml` (+3 top-level lines only), `scripts/ci-workflow.test.mjs`, `docs/PROGRESS.md`, `docs/reviews/actions-queue-reset.md` and `docs/reviews/ci-concurrency-independent-review.md`. No other file changed.
- Read: AGENTS.md, KICKOFF CI rules (time budget, merge flow, oversight "CI stays under 15 minutes"), the prior review, `ci.yml`, the concurrency stanzas of all five workflows, the test, and the PROGRESS and reset-evidence diffs.

**Verdict: B1 and B2 are resolved. No blocking findings.** The advisories below do not block. Merge still needs green required checks and the coordinator's gates. The public queue pause and the M2 limits remain in force.

## B1 recheck: grouping (Standards and Spec)

- The workflow group is now `${{ github.event_name == 'pull_request' && format('ci-pr-{0}', github.event.pull_request.number) || format('ci-run-{0}', github.run_id) }}`.
  - For `pull_request`, `format` always returns a non-empty string, so the `&&`/`||` idiom cannot fall through to the run-ID branch.
  - `push` and `workflow_dispatch` each get a unique `ci-run-<run_id>`. A pending main or dispatch measurement can no longer be evicted by a later run, which closes the B1 path into `ci-budget.mjs` `getMainRun`.
  - A re-run attempt reuses the same `run_id`, but the earlier attempt has already completed, so no eviction follows.
- `cancel-in-progress` is still `${{ github.event_name == 'pull_request' }}`. Only superseded runs of the same PR are cancelled.
- The `ci-pr-` and `ci-run-` namespaces cannot collide with each other. The only other repo-wide group is `policy.yml`'s pre-existing `trusted-ci-policy`, which is unchanged. No other workflow declares concurrency.
- The workflow diff from base is still exactly the three-line top-level `concurrency:` block. Triggers, `permissions: contents: read`, the matrix, `fail-fast: false`, `timeout-minutes: 12`, the pinned action SHAs and all steps are byte-identical to base.
- Limit: the fix rests on documented GitHub semantics. No hosted concurrency experiment was run, and the fresh dispatches ran on the base workflow. Hosted behaviour of this patch is **unverified**.

## Test recheck (Standards)

- `node --test scripts/*.test.mjs` on Node v24.18.0: **19/19 passed**.
- I ran mutants against a scratch copy of `ci.yml` plus the test file, so the tracked tree was never edited. Afterwards, `cmp` against the tracked `ci.yml` and `git status --short` were clean.

| Mutant | Result |
| --- | --- |
| Old shared grouping `ci-${{ pr.number \|\| github.ref }}` | killed |
| `cancel-in-progress: true` | killed |
| Job-level block `concurrency: {group: global, cancel-in-progress: true}` | killed |
| Job-level flow mapping `concurrency: { group: global, ... }` | killed |
| `timeout-minutes: 60` | killed |
| Extra step-level `timeout-minutes: 30` | killed |
| Top-level concurrency removed | killed |
| Non-PR group keyed by `github.ref` instead of `run_id` | killed |
| Suffix appended after the expression (`}}-x`, `}}${{ '' }}`) | **survived** |
| Whole file converted to CRLF (valid, should pass) | passed |

All four mutation classes the correction claims (old group, cancel-main, job override, timeout) are caught by the current test.

## B2 recheck: history (Spec)

- `## Current state — 2026-11-12` is the first section. `## Historical checkpoint from main before the Actions reset — 2026-11-11` follows it.
- I extracted the base `## Current state — 2026-11-11` body and diffed it against the historical section body. It is **verbatim**, apart from one added blank line. The following all appear once:
  - `43088994955` (461 s) and `43091130349` (549 s)
  - `E_PROTECTION`, "reviewed-SHA variable is cleared" and no retry
  - "E1 is implemented locally … still needs independent review"
  - "three #36 … review blockers"
- The rest of the base file is retained. The numstat is +16/−2, and the only two removed lines are the old heading and the stale line. The stale "No privileged Linux experiment has run" now reads "At this historical checkpoint, no privileged Linux experiment had run (superseded by the first reviewed experiment failure recorded above)". The grep for the old phrase finds nothing.
- The independent report is unchanged since `52be45c`.
- The new Ubuntu blocker entry matches the evidence file:
  - job `130963746682` in run `43152187271`
  - `ubuntu-mirror-updated:1`
  - Kernel mirror IPv6 connect error 101, Network is unreachable
  - APT/Playwright exit 100

  It claims no root cause, workaround or retry. I did not refetch logs.

## Advisories

Standards:
- **S1.** The group assertion uses an unanchored `includes`, so suffix mutants survive. These suffixes cannot collapse runs into a shared group, because the unique expression is still a prefix, so the risk is low. Anchoring the line like the `cancel-in-progress` regex (`^…\r?$`) would close the gap.
- **S2.** The single `timeout-minutes:` count fences out any future step-level timeout. That is intentional strictness, but such a change will need a test update.
- **S3.** There is a double blank line before `## PR29 compatibility integration`. Cosmetic only.

Spec:
- **P1.** The Current-state reset bullet still says jobs "started on main and PR36. Completion remains pending". Elsewhere in the same file, the Blockers section records the fresh main Ubuntu failure. The evidence doc records 5/5 macOS successes and four Ubuntu failures. Refresh that bullet in the next PROGRESS update.
- **P2.** Current state no longer says whether E1 (unpublished/unreviewed) and the three #36 blockers are still open. They now appear only as dated history. The coordinator should restate their present status if they remain open.
- **P3.** The reset doc's original "Validation … Three workflow mutations" paragraph describes the pre-correction revision. The corrections section that follows supersedes it, but labelling it would help.
- **P4.** A4 still stands: the budget baseline ignores `conclusion`. This is pre-existing and belongs in a separate PR.
- **P5.** Required checks are not green. Ubuntu is failing on the base workflow because of runner network reachability. That failure is unrelated to this patch, but it blocks any merge.

## Commands and limits

- `git log`, `git show`, `git diff --stat/--numstat` against base, and `git status --short` (clean).
- `node --test scripts/*.test.mjs`: 19/19 passed. Mutants ran via `node --test <scratch>/scripts/ci-workflow.test.mjs`.
- Not done:
  - no GitHub API calls or log fetches; no cancellation, retry, dispatch, push, PR, watch, merge or settings change
  - no cargo, native, ignored-fixture, owner/admin, SCM, LSA, VM or Lab Manager actions
  - no implementation or PROGRESS edits
  - `private-state.json` was not read
- Only this report file is committed.
