# CI headless-shell candidate

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12. Base is main after
#52, 1604db1. This is a separate CI performance candidate, not a Stage C
feature change and not a claim that hosted macOS capacity is fixed.

Main run 43240797525 attempt 1 failed at 906s. Its macOS job 131260319040
had no runner name or steps; GitHub annotated hosted runner acquisition failure
and macOS arm64 capacity constraints. Ubuntu and Windows passed. One failed-job
retry completed successfully, but policy deliberately measures attempt 1.
That failed over-budget baseline is retained; no status is manually overridden.

The only workflow change adds --only-shell to the existing signed-dependency
Chromium installation step. Matrix, names, arm64 macOS coverage, action pins,
permissions, concurrency, timeouts, dependencies and smoke steps are unchanged.
No new action, cache, trust surface or policy change is introduced.

[Playwright's browser documentation](https://playwright.dev/docs/browsers#chromium-headless-shell)
supports omitting regular Chromium when no channel is selected and tests use
headless shell. The pinned installed 1.65 CLI advertises --only-shell, and a
dry run selects headless shell and existing support binaries without regular
Chromium. This dry run downloaded or installed nothing. Our root config has
no channel or headed override; the UI smoke uses the default page fixture.
Desktop smoke uses _electron.launch and the application's Electron binary.
The config and all smoke test source remain unchanged.

## Baseline measurements

Coordinator read the Actions API for attempt 1 of these six main runs.
All listed Windows install steps passed. These are step durations, not build
execution time alone or claims about runner queueing.

| Main run | Windows job | Install start UTC | Install end UTC | Seconds |
| --- | --- | --- | --- | --- |
| 43240797525 | 131260319775 | 14:52:59 | 14:55:43 | 164 |
| 43233144276 | 131233422108 | 14:08:50 | 14:12:12 | 202 |
| 43231164086 | 131226519288 | 13:56:26 | 13:59:45 | 199 |
| 43221533019 | 131193987500 | 12:54:43 | 12:57:04 | 141 |
| 43213333692 | 131166213791 | 12:02:29 | 12:05:59 | 210 |
| 43204089423 | 131134713331 | 10:57:40 | 11:00:54 | 194 |

All timestamps are 2026-11-12. This explicitly pinned sample has minimum 141s
and median 196.5s; it is not the reviewer's differently selected sample.
Network variability prevents attributing every second to browser selection.

## Validation and acceptance

- Node script tests: 20 passed.
- Four workflow/config mutants were caught by runtime assertions and restored:
  remove --only-shell; remove macOS; add channel; set headless false.
- npm run lint and npm run build passed.
- Local Windows UI smoke 1/1 and Electron smoke 1/1 passed using existing
  installed binaries. No browser install was performed for these checks.
- No Rust source changed; no cargo or native service tests were run here.

Hosted evidence is pending. Before applying ci-speed, the exact candidate's
FIRST attempt must succeed on all three OS jobs, finish below both 900s and
the 906s baseline, retain UI smoke 1/1 and desktop smoke 1/1 on every OS, and
show a Windows install step below this sample's 141s minimum. The independent
reviewer must verify the unchanged coverage and the measured saving. No speed
claim or label is justified by this local run alone. A capacity or functional
failure stays red; no attempt-count reset or empty commit is a measurement.

Only then may the existing ci-speed exception evaluate the separate genuine
performance PR. No policy code changes or manual success statuses. Stage C
remains unlabelled. After this PR merges, main itself must pass a fresh run
inside 15 minutes before the feature queue can drain. If the candidate fails
these criteria, keep it blocked and record the evidence rather than relabel.

Capacity can fail again; this change does not fix GitHub runner allocation.
ExecutionFailed and E_PROTECTION remain unresolved. Native, installed and
dedicated paths remain **unverified on real hardware**; M2 is incomplete.
No owner/admin/account/service, SCM/LSA, VM/Lab Manager, experiment, private
state, ignored fixture or babysitter operation occurred. Rollback is the
one-line workflow revert and its associated guard update; no installed state.
