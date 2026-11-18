# Parallel CI preflight

Coordinator, 2026-11-18. This is local configuration evidence, not a hosted
speed result or M2/M3 acceptance. [ADR0031](../adr/0031-parallel-ci-validation.md)
records the decision and limits.

## Before

Exact base `ff599a2433bc5a34967d965d0f0da2b3a4517cc8`, first normal main CI
[44379630740](https://github.com/Dytschgo/arxcovo/actions/runs/44379630740)
attempt 1 succeeded in 606 seconds wall, with Windows 602 seconds, Ubuntu 280
and macOS 265. The old three-job policy measured 603 charged seconds and 3
validated idle queue seconds. Rust and UI validation ran sequentially in each
job. Those observations establish a comparison baseline, not a slowdown cause.

PR182's separate failed first attempt remains retained in
[its original evidence](m3-pr182-first-ci-held.md). All 32 transport cases passed
per OS; cancelled required jobs still prevented acceptance. No failure was
reclassified as a pass and no code fix is claimed.

## Prepared change

- Independent Rust and UI matrices each cover all three existing OS labels.
- Existing protected verify names run as gates requiring both complete matrices
  to succeed, including when a dependency fails, cancels or skips.
- All existing commands, tool pins and OS predicates stay in their owning phase;
  checkout is repeated for the independent worker. No artifacts are transferred.
- Every job keeps 12 minutes. Nine jobs receive no charged-v1 credit; the original
  900-second wall rule applies, with unchanged trusted policy code.
- The unchanged automatic Windows sample remains anchored in its Rust worker;
  its admission timing can differ after UI moves out of the sequential path.
  Sample results provide no stability, repair or re-enable credit.

## Executed local checks

Builder Sol6.1/high: focused workflow, policy, evaluator and budget suites passed
45/45, zero failures/skips/cancellations, 6.298 seconds. The extracted actual
PowerShell gate command was exercised for all 25 pairs of success, failure,
cancelled, skipped and missing results; only success/success returned zero.
One earlier test run was 44/45 because the new test parser included trigger
blocks as jobs. Its bounded correction limited parsing to the jobs section;
no workflow runtime failure occurred.

Coordinator Sol6.1/high: `node --test scripts/*.test.mjs` passed **50/50**, zero
failures/skips/cancellations, 6.032 seconds on Windows with Node 24.18.0 and
PowerShell 7.6.9. `git diff --check` passed. These checks exercise configuration
and policy guards, not the application or browser. No Rust/native/engine/client
test, install, rerun or dispatch was performed locally.

Application sources, tools, nextest configuration, trusted policy implementation,
policy workflow and auto-merge implementation match the exact base. Carried
unpublished clock CI/progress/history documentation is preserved separately from
this source-free change. Existing owner-client branch remains intact.

Evidence is retained under `D:/Temp/arx-ci-split` (baseline metadata, scope proof,
full Node log/result) and `D:/Temp/arx-ci-split-builder` (focused logs, phase
mapping and source identities). All scratch/cache roots were explicitly on D:;
C: had more than 689 GB free. No owner data, cache, VM or installation was changed.

## Remaining gates

Independent exact-head review and first-attempt hosted measurements are pending.
Before merge, require every existing protected check, all workers successful,
wall time at most 900 seconds and the existing Windows worker feature limit.
Then verify the resulting exact-main first attempt before a normal PR182 refresh.
No hosted speedup or release is claimed here. Engine Sol3/3 and historical
Opus0/2 remain unchanged. M2 OPEN/M3 UNCHECKED; applicable paths are **unverified on
real hardware**. The existing secure Claude Code OAuth request is untouched.
