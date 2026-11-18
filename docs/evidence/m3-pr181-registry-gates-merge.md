# PR181 registry gates and merge

Coordinator, 2026-11-18. Reviewed head
`8e5091b9796ec8b4b2fb1f1d813007dae9d246b6` passed
[CI44290836331](https://github.com/Dytschgo/arxcovo/actions/runs/44290836331)
on attempt 1. All five required checks were successful. Independent Sol6.1/high
[source review](../reviews/m3-peer-registry-source-sol61-review.md) applied to the
exact restored source; the exact-head review comment was 6976624630.

| Job | Duration | Rust passed / skipped | Registry tests | Node tests |
| --- | ---: | ---: | ---: | ---: |
| Windows 134755297808 | 594 s | 674 / 36 | 4 / 4 | 48 / 48 |
| Ubuntu 134755297264 | 304 s | 666 / 22 | 4 / 4 | 48 / 48 |
| macOS 134755297720 | 429 s | 634 / 8 | 4 / 4 | 48 / 48 |

Wall time: 598 seconds, 11:01:28–11:11:26Z. Windows satisfied the feature
660-second gate. Each of the four named registry tests passed on every platform.
The optional Windows sample was `skipped_budget`, e_s=604; no diagnostic case
executed and no stability, re-enable or fix credit is claimed.

Per-job logs were fetched with an 8 MiB read cap to `D:/Temp/arx-pr181-gates`:

| File | Bytes | SHA256 |
| --- | ---: | --- |
| windows.log | 247019 | 06070a0fb588d0fb66446bd40d14d6e27c3e04abd0ac59a963bc78bb5b03c92d |
| ubuntu.log | 252455 | 7fd246cbfa1909cc8ce1b2f8714951b17c0e52d3932bea8b5086ee644298ae0c |
| macos.log | 232495 | 084648a94d50f7ff5f4e5b6923da988138e73f96e46a340e84bf2cd522f9adc0 |

Immediately before merge, live main was
`8b36405e932ef2921a3c8d71dbaf7ed001314de7`, its exact CI44286225997 attempt 1
was successful, and PR181 was CLEAN with all five checks green. Coordinator
performed a normal expected-head guarded squash merge as
`cd8907500dcd89fddc0c659590f962cbf03c7ed5`. No bypass or admin action.
PR181 was then unwatched. Resulting main's own first CI44304025778 was queued;
no next feature merge until that exact-main result is verified.

Local unpublished documentation checkpoints are preserved. New cleanup/scratch
rules will be delivered with the next appropriate change rather than triggering
another diagnostic run solely for documentation. This callback is a transport
prerequisite only: M2 OPEN / M3 UNCHECKED; hardware **unverified on real hardware**.
Sol3/3 exhausted and historical Opus0/2 unchanged. No engine fix, diagnostic
rerun, dispatch, timeout/quarantine change or native owner-PC test.

## Exact-main first-attempt confirmation

Exact main `cd8907500dcd89fddc0c659590f962cbf03c7ed5` passed
[CI44304025778](https://github.com/Dytschgo/arxcovo/actions/runs/44304025778)
on attempt 1. 12:27:04–12:37:30Z: 626 seconds wall, 624 charged and two seconds
validated idle queue credit under the existing recorder. Windows 622 seconds,
Ubuntu 236 seconds, macOS 439 seconds. The history record was appended through
the unchanged `scripts/ci-budget.mjs --record`; no policy override.

Each OS ran all four named registry tests successfully and Node 48/48. Full Rust:
Windows 674 passed / 36 skipped, Ubuntu 666 / 22, macOS 634 / 8. Optional Windows
sample was `skipped_budget`, e_s=631: no cases or diagnostic credit. No rerun or
dispatch. Bounded per-job logs at `D:/Temp/arx-pr181-main-cd890750`:

| File | Job | Bytes | SHA256 |
| --- | ---: | ---: | --- |
| windows.log | 134800790689 | 246329 | 12042828966d6fcfec5c1398f74d0c3d2f468518d0300e058c875138db52f067 |
| ubuntu.log | 134800790871 | 251386 | b63050da082fa3ed8fd27cc7f38637f74fdc8b4248e3aef774f251a17597219e |
| macos.log | 134800790196 | 232568 | 8c91c99b2d2d23cd6175a6062f1c3dff2c2faed1d9ceb986c1969ee829dd6051 |

This closes the one-time PR181 exact-main followup. Continue the independently
reviewed owned-TLS prerequisite; M2 OPEN / M3 UNCHECKED and all prior stop rules
remain. No live transport, container or hardware acceptance follows from callback
CI success.
