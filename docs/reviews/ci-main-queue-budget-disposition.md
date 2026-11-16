# Main CI queue/budget disposition (design only)

Anthropic / claude-opus-5-5, research sub-agent, 2026-11-16. Root `83bc92d`
on exact main `c612152`. This is a design disposition, not a fix attempt. No
workflow, policy, PROGRESS or source edit; no push, rerun, dispatch, label,
status, merge, admin or native operation. Read-only GitHub API and one
official docs page only. It grants no CI-optimization authority.

## Measured facts (API, attempt 1 of run 43880904887, push, `c612152`)

| Job | Queued (created→started) | Executed | Result |
| --- | --- | --- | --- |
| ubuntu-latest | 37s | 228s | success |
| windows-latest | 69s | 372s | failure (R5, separate) |
| macos-latest | **1141s** (13:58:03→14:17:04) | 333s | success |

Run 13:58:02→14:22:37 = 1475s = policy `updated_at - run_started_at`.
macOS queue alone exceeds 900s; even zero macOS execution would leave the run
over budget. GitHub posted two annotations on the macOS job: "Due to capacity
constraints, jobs targeting macOS arm64 runners may experience longer queue
times." They are GitHub's, not repository text. The job log names image
`macos-26-arm64` 20261112.0504.1, so `macos-latest` currently resolves to
macOS 26 arm64.

Previous 11 main push runs (attempt 1): macOS queue 2–6s in nine, 149s and
187s in two; macOS execution 139–317s; Windows execution 279–708s and usually
the critical path; totals 296–713s. Windows execution on the two latest green
runs is dominated by nextest (355s, 338s) and clippy (111s, 127s). Precedent:
attempt 1 (run 43248592153) already recorded a 612s macOS queue. The recorded
conclusion was "No workflow change can shorten a hosted-runner queue."

## Official runner facts (docs.github.com github-hosted-runners, read today)

- Standard arm64 macOS (3-core M1, 7 GB): `macos-latest`, `macos-15`,
  `macos-26`, `xcode-27`. Intel (4-core): `macos-15-intel`, `macos-26-intel`.
- "During periods of high demand, jobs that run on macOS runners may take
  longer to start." The docs give no queue SLA or per-label capacity data.
- Larger runners are for Team/Enterprise Cloud organizations and are billed.

## Alternatives assessed

| Option | Effect on 1141s queue | Coverage | Verdict |
| --- | --- | --- | --- |
| `macos-26` pin | Same image and pool as today | same | No fix (alias only) |
| `macos-15` | Real image change, still standard arm64; the GitHub notice covers all macOS arm64 | arm64 kept, OS 26→15; ADR 0015 stock-directory evidence is bound to macos-26-arm64 | Rejected: no capacity evidence, coverage regression |
| `macos-*-intel` instead | Different pool | Loses Apple Silicon | Forbidden |
| Intel added alongside arm64 | arm64 job still gates run | — | No fix |
| `-xlarge` / self-hosted | Maybe | — | Paid/admin, forbidden |
| macOS non-blocking, separate workflow, `continue-on-error` | Hides queue from `ci.yml` | Weakens a required OS | Exclusion/skip, forbidden |
| Faster macOS steps | ≤333s off a 1475s run | — | Cannot reach 900s |
| Faster Windows steps | Normal critical path only | — | Does not address this cause |

## Verdict: precise external capacity blocker

The critical path of the baseline run was GitHub-hosted macOS arm64 queue
time, which no repository-controlled workflow change can bound. No in-scope
change keeps an arm64 macOS job inside `ci.yml` while capping its queue. An
alias or image swap on the same standard arm64 pool is not a fix. One sample
cannot show that another label queues less. Whether queues recur at the next
main push is uncontrolled and unpredictable. No speed or availability claim
is made.

Structural consequence: `ci-time-budget` reads only the latest completed main
push run. It recovers only after a new main push finishes within 900s. That
needs a merge, and merges need a green budget, which `ci-speed` alone can
grant. This also holds the separately built R5 repair PR. The gate, its
queue-inclusive measurement, required-check names and thresholds stay as
they are. Escalate to the coordinator/owner as a capacity blocker; this note
proposes no owner or admin step.

## Contract for any single `ci-speed` candidate (if one is proposed)

`budget()` only needs a successful candidate run shorter than 1475s, so a
normal-queue run of any head would pass numerically. Such a pass is queue
variance, not a reduction. A candidate qualifies only if all of these hold:

1. One concern: a genuine execution-time change, reviewed by the other
   provider before any push. No label, matrix, runner, timeout, test, skip,
   quarantine or policy change. The matrix stays ubuntu/windows/`macos-latest`
   (arm64).
2. A predeclared hypothesis names the steps it shortens. Per-step ceilings
   and the baseline (at least 5 recent green main first attempts of those
   steps) are frozen in the reviewed proposal.
3. Measurement is the first attempt of one normal push of the reviewed head.
   Its execution of the changed steps beats every frozen baseline value, its
   total is under 900s, and all three OS jobs succeed with unchanged test,
   smoke and desktop counts.
4. Queue is reported per job but never credited. A total that meets the gate
   only through a shorter queue does not qualify.

## Stop rule

Stop on any of these and record the result as NOT QUALIFYING: any required job
fails (including Windows R5), any job's queue is over 120s (confounded), a
per-step ceiling is missed, or the total is at or over 900s. Do not rerun,
dispatch, add an empty commit, re-push or re-label. A further attempt needs a
new substantive reviewed proposal. Any later merge also still needs a green
main, exact-head checks and agent-review.

## Status

M2 open; M3 unchecked; eight Windows quarantines unchanged. R5 is red and
remains separate. Native/installed paths are **unverified on real hardware**.
