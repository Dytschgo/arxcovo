# M3 recurring cluster deadline: Opus diagnosis and repair disposition

Provider/effort: Anthropic / Claude Opus 5.5 / high (design and diagnosis only; no source change).
Status: **design for Sol independent review**. No source fix may be committed before that review.

## Decision

1. The four exact signals in main CI 43788629298 (T2 and U6 `leader deadline: Elapsed(())` at
   `three_node.rs:100:10`; T3 and T4 write helper `Elapsed(())` at `three_node.rs:117:10`) are
   reproduced locally on exact main `01a42d5` by a bounded storage-latency feedback loop. They go red
   **only when several three-node cluster tests run at the same time and share one storage flush queue**.
   The same storage model with three-node tests limited to one or two at a time does not produce them.
2. CPU starvation, including the `yield_now` spin in the leader/write helpers, is **falsified** as a sufficient
   cause: four parallel three-node tests pinned to a single logical CPU stay green at normal speed.
3. Repair (separate small PR, Sol implements after review of this design): add a repository
   `.config/nextest.toml` test group that runs `arxd` `cluster::consensus::three_node::*` tests at most one
   at a time. All other tests keep the default four-way parallelism. No test, assertion, 5 s helper deadline,
   45 s test budget or production timeout changes. No skip, retry or quarantine.
4. Hosted cause status: **supported, not proven on the hosted runner**. Hosted flush latency cannot be
   instrumented from here. The calibration (below) ties the local model to hosted timings. The repair PR's own
   hosted Windows CI plus the next main runs are the hosted test. If they go red with a three-node deadline,
   the hypothesis fails and the next step is the escalation in "Attempt accounting".

## Pinned inputs

| Item | Value |
| --- | --- |
| Exact main | `01a42d5adfedbdbd3d6e6d35648b8adb0d76cad9` (after #150), obtained by `git archive` into an owned scratch directory with a unique `CARGO_TARGET_DIR`; no checkout or worktree binding |
| Record parent | `b460a01` (contains unmerged PR149 history; **not** the experiment baseline) |
| Red run | main CI 43788629298, Windows job 133041791518, 01:50:53Z–01:55:32Z, read once |
| Last green main | CI 43777637580 on `6e9edd5`, Windows job 133007458523, read once |
| Toolchain | rustc 1.95.0 and cargo-nextest 0.9.151, both identical to CI; `CARGO_PROFILE_DEV_DEBUG=0` as CI |
| Repo nextest config | none on main (default profile, `test-threads` = CPU count; `windows-latest` = 4 vCPU) |
| Local host | 16 logical CPUs, NVMe/SATA SSDs (flushes much faster than hosted) |

### What the red run shows

- Nextest started about 01:54:39Z. From the failure durations, T2 started ≈01:55:12.0, T3 ≈13.3, T4 ≈15.3,
  U6 ≈18.4. When U6 started, **all four test slots held three-node cluster tests**.
- Summary: 86/565 run, 82 passed, 4 failed, 5 skipped, 479 not run (fail-fast).
- #150 added three more three-node tests (`r6_shutdown_report_*`, each builds a real three-node cluster)
  directly before T1–U6 in run order. They passed at 3.4–8.9 s right before the failures. This is consistent
  with more queue pressure in that window; it is **not proven** to be the trigger.
- Storage-heavy, CPU-light consensus tests are 13–30× slower on hosted Windows than locally, while CPU
  pinning locally changes almost nothing:

| Test (red run, 4-way parallel) | Hosted | Local, 4 CPUs, 4 threads |
| --- | ---: | ---: |
| `suite_last_membership_in_log_multi_step` | 13.478 s | 0.455 s |
| `raw_helper_asymmetry_and_raw_behind_purged_fatal` | 15.778 s | 0.995 s |
| `startup_preflight_refuses_without_changing_existing_stores` | 15.512 s | 1.187 s |
| `startup_accepts_exact_empty_and_ordered_hole` | 8.019 s | 0.605 s |
| `three_node::t1_follower_offline_enrollment_and_reopen` | 10.298 s | 0.525 s |

Both stores open SQLite with `journal_mode=TRUNCATE`, `synchronous=FULL` and `fullfsync=ON`
(`crates/arxd/src/cluster/state_store.rs` `open_db`), so every commit issues several file flushes.

## Feedback loop

### Harness

- Scratch copy only: a throwaway SQLite VFS shim tagged `[DEBUG-cdl7]` registered as the default VFS
  immediately before `Connection::open` in `open_db`. It wraps only `xSync`: optional cross-process
  exclusive lock on one shared file (single-queue device model), then a fixed sleep, then the real sync.
  Source is in the appendix. It was never placed in this repository; the scratch tree was restored and
  verified identical to `git archive 01a42d5` (see Cleanup).
- Launcher: PowerShell `Start-Process cargo nextest run -p arxd --lib --locked --no-fail-fast -j N -E <filter>`
  with `ProcessorAffinity` set immediately; a watcher recorded each `arxd-*` test process's affinity.
  Child inheritance was verified (`affinities observed: 1 count=8` on the single-CPU run).
- Environment: `ARX_DEBUG_SYNC_US=<µs>`, optional `ARX_DEBUG_SYNC_QUEUE=<scratch>\tmp\syncq.lock`,
  `TMP/TEMP=<scratch>\tmp`, `CARGO_TARGET_DIR=<scratch>\target`.
- Red-capable signal: nextest `FAIL` with stderr `leader deadline: Elapsed(())` at `three_node.rs:100:10`
  (and write helper at `:117:10`); U6 `Orphaned(ShutdownReport …)` at `:78:28` recorded separately.
- Build: 42 s first build, 5 s incremental. Each loop run is 9–142 s.

### Calibration (storage-only tests, 4 threads, compare with hosted times above)

| Model | multi_step | preflight | raw_helper | accepts | append_to_log |
| --- | ---: | ---: | ---: | ---: | ---: |
| no delay | 0.455 | 1.187 | 0.995 | 0.605 | 1.159 |
| parallel sleep 5 ms | 2.507 | 3.238 | 3.264 | 1.717 | 1.447 |
| parallel sleep 15 ms | 6.579 | 8.008 | 8.334 | 4.291 | 2.111 |
| single queue 10 ms | 15.020 | 18.469 | 19.001 | 11.086 | 3.931 |

Linear fit: all five tests reach their hosted time at ≈29–32 ms per flush with parallel sleeps, or ≈8 ms
per flush with the single-queue model. The agreement across five different tests is the evidence that the
hosted slowdown is flush latency, not CPU.

### Results on exact main

| # | Filter | Model | Threads | Result |
| --- | --- | --- | --- | --- |
| 0 | three_node (8) | none | 16 | 8/8 pass, 7.8 s |
| 1 | three_node | none, affinity 4 CPUs | 4 | 8/8 pass |
| 2 | three_node | none, affinity **1 CPU** (verified) | 4 | 8/8 pass, 8.6 s: **CPU starvation falsified** |
| 3 | three_node | parallel sleep 30 ms | 4 | 8/8 pass; r6/T1 times match hosted (r6 store_only 7.5 s vs 7.9 s) |
| 4 | three_node | queue 8 ms | 4 | 7 pass, **U6 Orphaned** (`owners_left: 6`, total_ms 5010) |
| 5 | three_node | queue 12 ms | 4 | 7 pass, U6 Orphaned |
| 6 | three_node | queue 16 ms | 4 | **0/8 pass: 7× `leader deadline: Elapsed(())` at `three_node.rs:100:10`** + U6 Orphaned (exact hosted signature) |
| 7 | three_node | queue 16 ms | 2 | no deadline failures; U6 Orphaned only |
| 8 | three_node | queue 16 ms | 1 | no deadline failures; U6 Orphaned only |
| A | three_node | queue 8 ms | 1 | 8/8 pass (U6 9.9 s) |
| B | all `cluster::consensus` (68) + candidate group `max-threads=1` | queue 16 ms | 4 | 67/68; **no deadline failures**; U6 Orphaned only |
| C | all `cluster::consensus` (68) + candidate group `max-threads=1` | queue 8 ms (calibrated) | 4 | **68/68 pass**, 86.4 s (U6 10.0 s) |

Differential (6 vs 7/8, same flush service time): the leader/write deadline family needs cross-test flush
contention among parallel three-node tests. Run 6 is harsher than hosted (7 failures vs 4), which matches
hosted being between the 12 ms and 16 ms points once concurrent storage-heavy `consensus::tests` add load.
The write-helper (`:117`) signature was not separately isolated locally; it is the same 5 s per-call helper
pattern waiting on proposals that need the same flushed commits. It is grouped with the leader signal as
**inferred, not independently reproduced**.

Stopped on the exact signal; no broad workspace fixtures run (49 known unrelated Windows AccessDenied
fixtures untouched).

## Ranked hypotheses (after loop)

1. **Cross-test flush-queue contention among parallel three-node tests** stretches elections and commits
   past the 5 s per-call helper deadlines. Prediction: limiting three-node concurrency removes the deadline
   signals at the same latency. **Supported** (runs 6/7/8, B, C).
2. Per-test slow storage alone. Prediction: serial runs at the same latency also fail on deadlines.
   **Falsified for the deadline signals** (runs 7/8); **supported for U6 Orphaned at 2× calibrated latency**
   (run 8), consistent with the earlier Opus 8eea512 storage-latency proposal-owner apply-drain finding.
3. CPU starvation or `yield_now` busy-spin in helpers. Prediction: a 1-CPU pin fails. **Falsified** (run 2).
4. PR149 bounded fresh reads. Not on this path: T2/U6 fail in `leader()`, T3/T4 in `write()`. Unchanged.

## Repair design (for Sol review; Sol implements)

### Files (exact main based, one concern)

1. `.config/nextest.toml` (new):

   ```toml
   [test-groups]
   arxd-three-node-cluster = { max-threads = 1 }

   [[profile.default.overrides]]
   filter = 'package(arxd) & kind(lib) & test(/^cluster::consensus::three_node::/)'
   test-group = 'arxd-three-node-cluster'
   ```

   Keep `profile.default` so CI and local runs match; the CI command stays `cargo nextest run --workspace --locked`.
2. `tools/ci/test_nextest_groups.py` (new, stdlib `tomllib`, run in the existing tools/ci unittest pattern):
   asserts the group exists with `max-threads = 1`, the override filter is exactly the three-node module,
   no `retries`, `slow-timeout` kill, `threads-required` or skip/`default-filter` entries were added.
   Wire it into the existing CI python-guard step without adding a new job.
3. Behavior evidence in the PR (not committed code): `cargo nextest show-config test-groups --workspace`
   output listing all eight three-node tests in the group and no others, plus a local rerun of runs 6 (red,
   no config) and B/C (green, with config) using the appendix shim from a scratch copy.
4. `docs/PROGRESS.md` entry and an ADR in `docs/adr/` recording the test-concurrency constraint (heavy
   real-storage three-node tests share one flush device; serialize them rather than lengthen deadlines).

Not allowed in this PR: changing `three_node.rs`, helper deadlines, `Params`, SQLite pragmas, production
timeouts, CI `timeout-minutes`, retries, or quarantine.

### CI budget

- Last green Windows job (`6e9edd5`) took **708 s against the 720 s cap**; nextest 192.5 s. This tightness
  exists on main today and is independent of this repair.
- Three-node tests on that green run (4-way contended): T1 9.2, T2 15.3, T3 25.0, U6 20.1, T4 32.5 s.
  Serialized and uncontended they should be shorter (local calibrated chain ≈57–59 s in runs A/C) while the
  other ≈557 tests use the remaining three slots. Expected nextest wall change is small but **unmeasured on
  hosted**.
- Stop rule: the repair PR must report the hosted Windows nextest wall and job duration. If the job exceeds
  the 12-minute cap, do not raise the cap or bypass `ci-time-budget`; return to design (candidate:
  `max-threads = 2`, which run 7 showed is deadline-clean three-node-only at 16 ms but was **not** validated
  with other suites concurrent).

### Residual risk (state in the PR)

- U6 `Orphaned` remains possible if hosted flush latency exceeds ≈2× the calibrated value (run 8). That is
  the separate shutdown proposal-owner drain seam (8eea512, PR149 run 43782024400). If it recurs after the
  group lands, the next diagnostic is run 8's model with #150's `ShutdownReport` to separate apply drain from
  owner join, not a deadline increase.

## Attempt accounting (suite-level three-attempt oversight; no reset by renaming)

Genuinely different **fix** attempts on the consensus suite:

1. PR147 `a318ae1`: release append store before completion callback (appendIo, run 43767515928). Controlled
   race 20/20 fixed; natural recurrence unproven. Its signature has not recurred.
2. `6eb5ad8` (PR149 branch, unmerged): serialize shared apply-pause fixtures (R1/R6 hook isolation).
3. **This repair** (three-node test group) would be suite-level fix attempt 3.

Diagnostics (not fixes): 43757400254 Linux shim unknown (main flake outside cluster), PR150 shutdown phase
report, 8eea512 induced storage latency for U6, A0 refusal-contract work, this disposition.

If the repair goes red on hosted Windows with any three-node deadline or Orphaned signal, the suite has reached
three failed fixes: stop, record evidence, and escalate to the owner for an explicit quarantine decision
(honestly marked, uncredited, **unverified** until a hosted fix lands, and independently reviewed) rather than
a fourth blind change.

## Merge-block standing

- Main `01a42d5` is **RED** (43788629298). No further merge until latest exact-main CI is green; no manual
  rerun to fish for green.
- The repair PR is the only candidate to restore green. It still needs Sol review of this design, Sol
  implementation, a different-provider independent review, green required checks and coordinator merge.
- PR149 (watch/root persistence, local hook-race recheck) is owned by a separate Opus reviewer; this work did
  not touch its files.

## Cleanup

- Scratch: shim file, `open_db` hook and scratch `.config/nextest.toml` removed; `state_store.rs` restored from
  `git show 01a42d5:…`; `grep -rln DEBUG-cdl7` → none; `diff -rq` against a fresh `git archive 01a42d5` →
  identical. Worktree: `grep DEBUG-cdl7` → none.
- No installs, cache deletion, child agents, push, PR status change, merge or watch. Only scratch target/temp
  on the D: drive and the session scratchpad were used.

## Appendix: scratch-only shim and launcher (not for commit to source)

Insert `debug_sync_shim::install();` immediately before `let db = Connection::open(&path)?;` in
`open_db`, and append `#[path = "debug_sync_shim.rs"] mod debug_sync_shim;` at the end of `state_store.rs`.

```rust
// [DEBUG-cdl7] crates/arxd/src/cluster/debug_sync_shim.rs (scratch only)
use rusqlite::ffi;
use std::os::raw::{c_char, c_int};
use std::sync::Once;
#[repr(C)]
struct ShimMethods { m: ffi::sqlite3_io_methods, orig_sync: unsafe extern "C" fn(*mut ffi::sqlite3_file, c_int) -> c_int }
static mut ORIG: *mut ffi::sqlite3_vfs = std::ptr::null_mut();
static mut DELAY_US: u64 = 0;
unsafe extern "C" fn shim_sync(f: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    unsafe {
        let s = (*f).pMethods as *const ShimMethods;
        let q = std::env::var_os("ARX_DEBUG_SYNC_QUEUE").map(|p| {
            let f = std::fs::OpenOptions::new().create(true).write(true).open(p).unwrap();
            f.lock().unwrap();
            f
        });
        if DELAY_US > 0 { std::thread::sleep(std::time::Duration::from_micros(DELAY_US)); }
        let rc = ((*s).orig_sync)(f, flags);
        drop(q);
        rc
    }
}
unsafe extern "C" fn shim_open(_v: *mut ffi::sqlite3_vfs, name: *const c_char, f: *mut ffi::sqlite3_file, flags: c_int, out: *mut c_int) -> c_int {
    unsafe {
        let o = ORIG;
        let rc = ((*o).xOpen.unwrap())(o, name, f, flags, out);
        if rc == 0 && !(*f).pMethods.is_null() {
            let orig_m = *(*f).pMethods;
            if let Some(os) = orig_m.xSync {
                let mut m = orig_m;
                m.xSync = Some(shim_sync);
                let b = Box::leak(Box::new(ShimMethods { m, orig_sync: os }));
                (*f).pMethods = &b.m;
            }
        }
        rc
    }
}
pub(crate) fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        DELAY_US = std::env::var("ARX_DEBUG_SYNC_US").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        if DELAY_US == 0 { return; }
        ORIG = ffi::sqlite3_vfs_find(std::ptr::null());
        let mut v: ffi::sqlite3_vfs = *ORIG;
        v.zName = c"dbgsync".as_ptr();
        v.xOpen = Some(shim_open);
        let v = Box::leak(Box::new(v));
        assert_eq!(ffi::sqlite3_vfs_register(v, 1), 0);
    });
}
```

Launcher (PowerShell, scratch paths shown as `<scratch>`):

```powershell
param([int]$Aff=15,[int]$J=4,[string]$Filter='test(/cluster::consensus::three_node::/)',[string]$Out)
$env:CARGO_TARGET_DIR='<scratch>\target'; $env:CARGO_PROFILE_DEV_DEBUG='0'
$env:TMP='<scratch>\tmp'; $env:TEMP='<scratch>\tmp'; $env:NEXTEST_HIDE_PROGRESS_BAR='1'; $env:NO_COLOR='1'
$p = Start-Process cargo -ArgumentList @('nextest','run','-p','arxd','--lib','--locked','--no-fail-fast','-j',"$J",'-E',"`"$Filter`"") `
  -WorkingDirectory '<scratch>\src' -NoNewWindow -PassThru -RedirectStandardError $Out -RedirectStandardOutput "$Out.stdout"
$p.ProcessorAffinity = [IntPtr]$Aff
# watcher: record (Get-Process arxd-*).ProcessorAffinity until $p exits
```

Red reproduction (run 6): `ARX_DEBUG_SYNC_QUEUE=<scratch>\tmp\syncq.lock ARX_DEBUG_SYNC_US=16000`, `-J 4`.
Candidate green (run C): same at `ARX_DEBUG_SYNC_US=8000` with the proposed `.config/nextest.toml`, filter
`test(/cluster::consensus::/)`.
