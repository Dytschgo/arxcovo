# PR182 refreshed first attempt: macOS runner never acquired

Exact head `834ca7d9a51ea6ce4eeac9fecfbe0861e7cec19c`, normal first
[CI44404523355 attempt1](https://github.com/Dytschgo/arxcovo/actions/runs/44404523355/attempts/1)
completed **FAILURE, 912 seconds wall**. PR182 remains HOLD; no rerun/dispatch,
source/settings change, budget exception or counter reset occurred.

| Worker | Result | Execution evidence |
| --- | --- | --- |
| Rust Windows | success,435s | 741 passed/36 skipped; all32 transport cases once PASS |
| Rust Ubuntu | success,165s | 727 passed/22 skipped; all32 once PASS |
| Rust macOS | success,330s | 695 passed/8 skipped; all32 once PASS |
| UI Windows | success,73s | Node60, browser/Electron passed |
| UI Ubuntu | success,85s | Node60, browser/Electron passed |
| UI macOS135156078165 | cancelled,zero steps | runner_id0; no runner/group name; no observed execution |
| All3 verify gates | failed closed | dependency guard saw UI abandoned; Ubuntu6s/Windows4s/macOS4s |

The [macOS UI check](https://github.com/Dytschgo/arxcovo/actions/runs/44404523355/job/135156078165)
states that a hosted runner never acquired the job after multiple allocation
attempts. Three notices identify macOS arm64 capacity constraints/longer queues.
Metadata sets created_at=started_at21:44:13 and completed_at21:59:15. That902-second
timestamp span is **not UI execution** and does not establish a12-minute worker
timeout. No UI, browser or Electron step ran on macOS in this attempt.

The source-policy result is preserved as wall912, computedcharged910/idle2/
longestspan902, `withheld:not-green`; no credit applies and acceptance uses wall912,
which fails900. Those computed spans cannot be described as measured execution
for the unacquired job. Green ci-time-budget evaluates the healthy main baseline,
not this failed candidate. No failed candidate was added to duration history.

All96 named transport PASS rows match the exact32-source inventory, with no
duplicates. Source GO and unchanged blob proofs remain valid. The automatic
Windows diagnostic sample reported8PASS/orphans0/29.657s; it grants no stability,
repair, re-enable or exhausted-attempt credit. Neither this attempt nor the old
2e4e0d3f execution-limit cancellations establish a transport regression. The old
slowdown's underlying cause remains UNKNOWN; current capacity annotations cannot
retroactively explain it.

Evidence retained at `D:/Temp/arx-pr182-ci-refresh/first-ci`: terminal run/jobs,
full failed-job checks/annotations, final-execution-proof.json, policy-measurement,
source-name inventory matching and all-log-fetch-manifest.json. Eight real job
logs are available, each fetched under8MiB/45s with no truncation. The ninth fetch
returned exit1/215-byte BlobNotFound XML; its .log file is explicitly labelled an
API-error response, **not a job log**. No unavailable log is counted as inspected.

Fresh independent Astra6.2/high reviewed retained evidence and Git pins only,
confirmed allocation failure and HOLD, and preserved independent Sol source GO.
Disposition/pins: `D:/Temp/arx-pr182-runner-disposition/{review.md,pin-proof.json}`.
No tests/builds/probes/remote writes occurred in that disposition.

No currently authorized action can qualify this terminal attempt. The owner's
no-rerun/dispatch, first-attempt and budget rules require separate authorization
and independent review of a prospective recovery contract, or a separately
authorized genuine change with an eligible normal first attempt. Repeating only
the UI job would not by itself resolve first-attempt/budget eligibility. Restored
capacity alone cannot complete this failed run. Do not manufacture source changes
or waive any Mac/verify check. PR body records HOLD; requested work returns to the
owner inbox after unwatch, rather than a passive polling loop.

Main last verifiede11b0767/first44399330997 remains qualified920wall/827charged/
93idle. EngineSol3/3 exhausted, historicalOpus0/2 unchanged. M2 OPEN/M3 UNCHECKED;
v1 incomplete and hardware **unverified on real hardware**. OAuth, owner setup,
Lab Manager/VM/cache/user data and existing babysitter untouched.
