# U6 shutdown diagnosis (Sol high, 2026-11-16)

Base: `382a82ae115e031d96d422a6ef709cc1d9e1f531`, local branch
`t3/diagnose-u6-shutdown`, containing PR149 head `8eaed30`.

Status: **blocked on exact local reproduction**; no source fix established. No hypothesis or
source/API investigation started without the exact local shutdown failure.
No fix attempts, source edits, test weakening, dependency changes, CI reruns,
pushes or PR operations. The five-second shutdown contract and C1/C2 ownership
requirements remain unchanged. No claim that A0 caused the hosted failure.

## Target signal

Coordinator-supplied verified hosted evidence: Windows job `133021152770`, run
`43782024400`, U6 failed in 20.678 s at `three_node.rs:78:28`,
`shutdown().await.unwrap()` in `Cluster::stop`, with `Err(Orphaned)`.
Restored routing observed 43/64 resolutions, all within eight seconds false;
that observation is permitted by the reviewed PR145 contract. Job duration
287 s, not job timeout; 113 passed, one failed, 453 unrun. Other R1–R5/T2
passed. This investigation did not independently fetch that hosted job.

Exact reproduction requires U6 itself to panic at shutdown with `Orphaned`.
A marker-scan file lock, append deadline, R4 timeout, or the deliberately gated
R5 `Err(Orphaned)` is a different signal and cannot qualify.

## Local commands and bounds

All binaries and temporary cluster stores use the unique directory
`D:/arxcovo-u6-20261116-sol`; existing caches were not deleted and nothing
was installed. Build:

```powershell
$env:CARGO_TARGET_DIR='D:/arxcovo-u6-20261116-sol/target'
$env:TEMP='D:/arxcovo-u6-20261116-sol/tmp'
$env:TMP=$env:TEMP
New-Item -ItemType Directory -Force $env:TEMP | Out-Null
cargo test -p arxd --locked --lib u6_cancelled_callers_keep_engine_permits_until_resolution --no-run
```

Build passed in 57.53 s. Test executable:
`D:/arxcovo-u6-20261116-sol/target/debug/deps/arxd-c184af5585f22530.exe`.
The exact real test invocation (unchanged source):

```powershell
& $exe cluster::consensus::three_node::u6_cancelled_callers_keep_engine_permits_until_resolution --exact --nocapture
```

Harnesses use `Start-Process -WindowStyle Hidden`, separate stdout/stderr files,
and `WaitForExit(55000)` (60 s for the initial full-cluster experiment), killing
only their own test process on harness timeout. No test retry was added to the
product or CI; these are diagnostic repeat loops.

1. Sequential: at most 24 invocations, stop at first failure; **24/24 passed**.
   Files `baseline-{1..24}.log[.err]`. Example: one passed, 531 filtered,
   0.90 s. Both isolated and restored shutdown assertions ran each time.
2. Concurrent U6: eight batches of eight independent test processes, stop on a
   failed batch; **64/64 passed**. Files `concurrent-{1..8}-{1..8}.log[.err]`.
   Example batch-first durations 1.78–1.87 s. All 64 restored observations were
   64/64 within the window.
3. Real cluster contention: four processes per batch, each running
   `$exe cluster:: --nocapture --test-threads=8`, each process assigned affinity
   mask 3 (two logical processors shared across the four processes). Bound:
   four batches, stop at first failed batch. Stopped after two batches/eight
   processes: six passed (114 tests, two ignored each); two failed as below.
   Durations 17.03–19.13 s. Seven U6 tests passed; the eighth failed in the
   marker scan after shutdown, before reaching the second fixture. No U6
   shutdown returned `Orphaned`.
4. Follow-up exact-signal load loop: up to eight batches of four processes,
   same affinity and concurrency, using
   `$exe cluster:: --nocapture --test-threads=8 --skip r4_read_admission --skip r5_shutdown`.
   This avoids confusing known R4/gated-R5 signals with U6. Stops only on a
   failed U6 with `three_node.rs:78:28` and `Orphaned`. Files
   `exactload-{batch}-{process}.log[.err]`. **32 processes completed: 29 passed,
   three failed**. U6 passed 31 times and failed once at the marker scan
   (`exactload-7-4`); the other two failures were T1 marker scans (`7-2`, `8-2`),
   also Windows error 33 at `three_node.rs:167:65`. No exact U6 `Orphaned`.
   Durations 15.18–17.92 s. All 31 restored-routing observations were 64/64
   within the window; the marker-scan failure prevented the other restored leg.
5. Final concentrated-load bound: two batches of eight processes, same cluster
   filter/skips and test concurrency as step 4, affinity mask 1 (all eight
   processes share one logical processor). Files
   `onecpu-{batch}-{process}.log[.err]`. First batch: six processes passed,
   two failed in other tests (T4 startup `Open(Io)` at `three_node.rs:63:10`,
   R3 `Elapsed(())` at `three_node.rs:117:10`). All eight U6 tests passed.
   The first batch lasted about 48 s per process; T3 reached 20.7078 s, close
   to the supplied hosted T3 duration of 22.551 s, without U6 `Orphaned`.
   This does not establish or exclude a hosted load cause.
   Second batch: seven processes passed; one T4 startup failed with `Open(Io)`
   at the same line (`onecpu-2-4`, 47.68 s). All eight U6 tests passed.
   Overall: **13/16 cluster processes passed, 16/16 U6 passed**.

Across all five experiments: **144 actual U6 test executions, 142 passed,
two marker-scan failures, zero U6 shutdown `Orphaned`**. All 142 executions
that reached the restored-routing observation reported 64/64 within eight
seconds. No harness timeout occurred. The full cluster suites were not all
green: eight process-level failures are described here, including the two U6
marker-scan failures. These repeats provide no red-before/green-after fix proof.

## Separate failures, not the target reproduction

`cluster-2-2.log`: R4 failed at `three_node.rs:965:10`, `Elapsed(())`;
113 passed, one failed. U6 passed. The injected append/iterator panic messages
are expected tests, not U6 failures.

`cluster-2-4.log`: U6 failed at `three_node.rs:167:65` with Windows error 33,
"The process cannot access the file because another process has locked a portion
of the file." This occurred in the marker scan after successful shutdown, with
113 passed and one failed. It is consistent with the previously recorded
Weak-expiry/file-close advisory but does not identify the cause of the hosted
`Orphaned`. No fix for this separate failure was attempted.

## Handoff boundary

No exact local `Orphaned` reproduction yet means no supported failed shutdown
phase or pending-task identity. No temporary instrumentation has been added;
there are no debug tags to clean. Do not infer a load cause or an A0 regression
from the nearby failures. Main `6e9edd5` was not compared because an exact
red signal was not established on this head.

Debug phases 1/2 remain incomplete for the requested symptom; phases 3–6 were
not entered. Zero genuinely different fixes were attempted, so the three-fix
handoff threshold was not consumed. This note is intentionally uncommitted:
the requested source/test fix commit requires correct-seam red-before/green-after
evidence, which is absent. No agent children were created. All ordinary test
subprocesses have exited.

The next useful investigation needs a repeatable U6 shutdown `Orphaned` under
the actual interleaving, followed by tagged phase/owner/store-retention probes
without changing the five-second deadline. Preserve C1/C2 and A0 caller
semantics; distinguish core shutdown, proposal owners, read owners, and store
release rather than accepting `Orphaned`. Root retains PR149 watch ownership;
fresh independent Opus review remains required before any push.

No fmt/Clippy or fresh hosted checks claimed for a fix: no source fix exists.
Separate hosts, VM paths and transport are **unverified on real hardware**.
