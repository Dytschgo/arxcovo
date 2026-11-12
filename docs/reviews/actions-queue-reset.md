# Actions queue reset and CI concurrency audit

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12T04:02:05.681536+00:00. User explicitly requested cancellation of all queued/running runs and a fresh start.

## Recovery results

- Enumerated 12 queued CI/policy runs and no in-progress runs before the reset. Each received one normal cancel and one force-cancel request. All 24 requests failed with HTTP 409: `Cannot cancel a workflow run that has not been queued yet.` None is claimed cancelled.
- One rerun request for each of the five current main/PR CI runs failed with HTTP 403: `This workflow is already running`. No rerun succeeded.
- Fresh dispatches of `ci.yml` were accepted on main and the four current PR branches. Main and PR36 were observed in progress with real jobs; the remaining dispatches are accepted, completion still pending. No native experiment workflow was dispatched.
- This proves fresh scheduling can work. It does not explain or remove the old inconsistent records. No account restriction is established. Do not delete historical runs to disguise the queue.
- The original main push run remains uncompleted; the fresh main workflow_dispatch is separate evidence, not a replacement first-attempt push measurement for the time-budget gate.

| Ref | Fresh CI run |
| --- | --- |
| `main` | [43152187271](https://github.com/Dytschgo/arxcovo/actions/runs/43152187271) |
| `feat/service-executor-interface` | [43152254254](https://github.com/Dytschgo/arxcovo/actions/runs/43152254254) |
| `test/await-policy-failure` | [43152257543](https://github.com/Dytschgo/arxcovo/actions/runs/43152257543) |
| `docs/ci-duration-refresh` | [43152261583](https://github.com/Dytschgo/arxcovo/actions/runs/43152261583) |
| `test/unix-manager-observations` | [43152265161](https://github.com/Dytschgo/arxcovo/actions/runs/43152265161) |

## CI improvement prepared locally

The CI workflow has no concurrency group, so older PR builds can keep consuming runner capacity after a new head arrives. Group pull_request events by PR number; give every other event a unique run-ID group. Cancel-in-progress applies only to pull_request events. Separate PRs do not cancel each other. Main and manually dispatched measurements have separate unique groups, so a new run cannot evict a pending measurement. A manual dispatch on a PR branch runs independently of that PR's own event runs. This does not purport to fix the stuck GitHub records.

Validation: `node --test scripts/*.test.mjs`: 19/19 passed. Three workflow mutations (remove concurrency, allow cancelling main, share one global group) each failed the focused assertion and were restored. The test locks the workflow source contract; it does not emulate GitHub scheduling. No speedup or hosted verification of this local change is claimed. Required gates, matrix, permissions, pinned actions, 12-minute job limit and smoke checks are unchanged. No cache or retry change was added without measurements.

Independent different-provider review and hosted verification remain required before merge. No PR, push, merge, settings, owner/admin or native operations. Native/installed/dedicated M2 remains **unverified on real hardware**.

## Independent review corrections

Opus high review at `406b5ef` requested B1 (pending main runs still shared a group despite cancel-in-progress false) and B2 (historical Current-state evidence removed). Sol medium corrected B1 with unique non-PR run-ID groups, and added source fences against a job-level concurrency override and a changed job timeout. B2 restores the entire previous main Current-state section under a historical heading, preserving the two main durations, Linux E_PROTECTION experiment/cleared-SHA/no-retry, then-unreviewed E1 and then-open PR36 blockers. Those are dated historical claims, not assertions of their present status. The stale no-privileged-experiment wording is marked historical. The independent report is retained verbatim.

Hosted follow-up at review time: all five macOS jobs succeeded; main/PR36/PR26/PR34 Ubuntu jobs failed, PR9 Ubuntu and all Windows jobs were still running. No green overall run is claimed. These dispatches use the unchanged base workflow, not the local concurrency change.

Correction verification: all 19 Node tests pass. Four actual workflow mutants (the old shared-main grouping, cancel-main, job-level concurrency and 60-minute timeout) each failed compiled/runnable Node assertions and were restored. No hosted GitHub concurrency experiment was performed.

Main Ubuntu job `130963746682` in fresh run `43152187271` failed after `ubuntu-mirror-updated:1`: APT package downloads from mirrors.edge.kernel.org reported IPv6 connect error 101, Network is unreachable; Playwright dependency installation exited 100. This differs from the historical name-resolution symptom. It does not establish why IPv4/IPv6 connectivity failed or justify a network/config workaround. Other fresh Ubuntu failure causes have not been individually inspected. No retry or native experiment was dispatched after these failures.
