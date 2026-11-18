# PR185 original first attempt: all jobs passed, budget hold

Published head `59800c7632b0d91a09c6643ffe455a0898328571`,
[CI44389444133](https://github.com/Dytschgo/arxcovo/actions/runs/44389444133),
normal attempt 1, completed/success. 2026-11-18 20:17:23–20:33:12 UTC:
**949 seconds wall**. All five required checks succeeded, but ADR0031's initial
900-second wall requirement failed. No merge, rerun or dispatch occurred.

| Job | ID | Execution seconds | Result |
| --- | ---: | ---: | --- |
| Rust Windows | 135102767918 | 441 | success; 709 Rust passed, 36 skipped |
| Rust macOS | 135102768173 | 327 | success; 663 passed, 8 skipped |
| Rust Ubuntu | 135102769060 | 192 | success; 695 passed, 22 skipped |
| UI Windows | 135102768428 | 69 | success; Node50, UI/browser/Electron passed |
| UI macOS | 135102768791 | 56 | success; Node50, UI/browser/Electron passed |
| UI Ubuntu | 135102768523 | 112 | success; Node50, UI/browser/Electron passed |
| Verify Windows name, Ubuntu runner | 135112455869 | 7 | success |
| Verify macOS name, Ubuntu runner | 135112455932 | 5 | success |
| Verify Ubuntu | 135112455907 | 14 | success |

Each OS ran the existing 14 pure clock cases and anchor case exactly once.
Issuer i6 passed on Ubuntu/macOS; its existing Windows source quarantine remains.
The automatic Windows sample reported eight passes, zero orphans and 31.688s.
It grants no stability, repair, re-enable, native or exhausted-attempt credit.

Mac Rust was created at 20:17:24, started at 20:27:28 and finished at 20:32:55.
Mac UI ran 20:22:28–20:23:24. Gates were created only at 20:32:55. These facts
show idle pending time; they do not establish a universal runner limit, outage or
the cause of PR182's earlier slowdown. The split's worker timings do not establish
a charged-plus-execution improvement over the 606-wall/603-charged base.

The coordinator executed the unchanged published policy on the retained exact
metadata: `unavailable:jobs-invalid`, wall949, charged/queue/longest null and empty
job measurements. This is the **actual recorded policy result**. It is not replaced
by calculations under a proposed validator.

Fresh independent Sol6.1/high disposition confirmed HOLD and recommended a
prospective exact-nine-job charged-v1 adaptation under the existing accepted
budget definition. Report: `D:/Temp/arx-ci-split-budget-disposition/review.md`.
Its counterfactual arithmetic is 165 idle seconds, 784 charged and 441 longest;
784 is above the base's 603 charged. This is design/regression-fixture reasoning,
not live credit, retrospective acceptance or a speedup claim. The adaptation
requires source review and a normal NEW-head first attempt before adoption.

All nine logs are retained at `D:/Temp/arx-ci-split/{job-id}.log`, each capped at
8 MiB and a 45-second fetch deadline. Every fetch exited zero, without truncation.
The manifest records exact byte sizes and SHA256 for all nine files; its SHA256 is
`14acdae4a9286ca516b560d9e261d96e010527df7e5da4d664cf0325836cd629`.
The same directory retains attempt1-run.json, attempt1-jobs.json, execution-proof.json
and the original policy-measurement.json. No history row was rewritten.

PR182 remains held. Engine Sol3/3 and historical Opus0/2 are unchanged. M2 OPEN /
M3 UNCHECKED; relevant paths remain **unverified on real hardware**. OAuth input,
owner setup and the babysitter are untouched.
