# PR185 exact-main first attempt: qualified green

Exact main `e11b0767f1d108313824a5fd2600516561d70391`, normal push
[CI44399330997](https://github.com/Dytschgo/arxcovo/actions/runs/44399330997),
attempt1 completed/success. **920 seconds wall / 827 charged / 93 validated idle
queue seconds; longest job515 seconds.** All9 jobs succeeded. The reviewed
exact-nine validator applied the existing charged-v1 budget; the full wall value
is retained. No raw-wall speedup, charged reduction or slowdown cause is claimed.

| Phase | Windows | Ubuntu | macOS |
| --- | ---: | ---: | ---: |
| Rust execution seconds | 515 | 192 | 304 |
| Rust passed / skipped | 709 / 36 | 695 / 22 | 663 / 8 |
| Rust test body seconds | 209.647 | 51.455 | 80.453 |
| UI execution seconds | 72 | 107 | 57 |
| Node script tests passed | 60 | 60 | 60 |
| Verify seconds, all Ubuntu runners | 6 | 4 | 4 |

UI/browser/Electron passed each OS. Existing14pureclock+anchor cases passed once
per OS; issuer i6 passed Ubuntu/macOS, existing Windows source quarantine stayed
unchanged. The automatic Windows sample reported8PASS/orphans0/31.906s, granting
no stability, repair, re-enable or exhausted-attempt credit. Each job stayed below
12minutes, and Windows workers below660seconds. No native/hardware acceptance.

The macOS Rust job was created at21:14:09, started21:24:14, and completed21:29:18.
Credit includes only created pending intervals with no job executing. Each gate
was created at21:29:18, after all workers completed, and started21:29:21. Queue
cause is unproven; no overlap or execution was subtracted. The existing evaluator
and metadata bounds are unchanged apart from the independently reviewed exact
layout validation already merged in PR185.

All9 logs and metadata/count/case/timing proof retained at
`D:/Temp/arx-ci-split-main-e11`, each log bounded8MiB/45s, zero fetch errors or
truncation. policy-measurement.json is the actual source-policy result, and
history-append-proof.json verifies insertion of record37 with every prior byte
preserved. No candidate or failed run was reclassified or inserted as main.

A single bounded foreground observer followed this exact push run; it exited0
and retained45,425 stdout bytes without truncation. No rerun/dispatch, workflow
cancellation, settings/timeout/quarantine change or counter reset occurred.
PR185 remains merged/unwatched. The owner-authorized normal PR182 refresh may now
proceed, still requiring independent integrated review and its normal first CI.
Original PR182 cancellation evidence and exhausted engineSol3/3/historicalOpus0/2
remain unchanged. M2 OPEN/M3 UNCHECKED; relevant paths **unverified on real hardware**.
