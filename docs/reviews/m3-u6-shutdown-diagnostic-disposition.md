# M3 U6 shutdown diagnostic disposition

2026-11-16. Opus 5.5 high design handoff on `affc85c` (contains PR149 head `8eaed30`). This is a docs-only decision for Sol to implement and for review. No source fix, push, PR operation, CI rerun or PROGRESS change. The three-fix rule is not consumed (0 fixes). PR149 stays blocked. Root keeps the watch. M2 hardware paths are unchanged and **unverified on real hardware**.

## Target and prior evidence

Hosted Windows run `43782024400` job `133021152770`: U6 panicked at `three_node.rs:78:28` with `Err(Orphaned)` after `resolved=43/64` (the permitted restored observation). The test took 20.678 s and the job 287 s, so this was not a job timeout. Sol's 144 unmodified local U6 runs under CPU contention (`docs/evidence/m3-u6-shutdown-diagnosis.md`) produced zero exact `Orphaned`. The error-33 marker-scan failures and the R3/R4/T4 failures are separate signals and are not addressed here.

## What the shutdown actually waits on (code + OpenRaft 0.10.0)

`ConsensusNode::shutdown` runs three phases sequentially under one 5 s deadline and merges them into a single bit: (1) `raft.try_shutdown()`; (2) join the `owners` JoinSet (proposal owners and read owners are mixed); (3) poll `log_weak`/`sm_weak` until no strong reference remains. The unit `Orphaned` discards which phase failed.

OpenRaft documents that `try_shutdown` success covers only the core and tick joins, "independently of … other background task lifetimes" (`raft/mod.rs:2136-2143`). After the core exits, the state-machine worker keeps draining its queue (`core/sm/worker.rs` loop ends only when `cmd_rx` closes). It applies queued entries one at a time and only then sends each `EntryResponder` that a proposal owner awaits. Snapshot builds run in detached tasks (`worker.rs:245`), and replication joins are detached too (`raft_core.rs:2469`). Phase 2 can therefore outlast the engine. Its duration is bounded by the apply drain, not by the engine.

## Bounded different-strategy reproduction (storage latency, not CPU)

Throwaway copy `git archive affc85c` in `D:/arxcovo-u6-20261116-opus` (source/target since deleted; logs and the 93-line probe diff `runs/debug-u6o-probe.diff`, sha256 prefix `8a1f376c2f649838`, retained). Tagged `[DEBUG-u6o]` probes printed each phase outcome and its elapsed time. Test-only env `ARX_DEBUG_U6O_DELAY_MS` slept inside each store `spawn_blocking` closure. Exact invocation: `arxd-c184af5585f22530.exe cluster::consensus::three_node::u6_cancelled_callers_keep_engine_permits_until_resolution --exact --nocapture`, 60 s harness cap, one process at a time.

| delay/op | restored | leader engine | owners at 5 s | store at end | U6 |
|---|---|---|---|---|---|
| 0 / 50 / 100 ms | 64/64 | ok <1 ms | 0 left (≤0.1 s) | released | pass ×3 |
| 150 ms (×2) | 49, 48/64 | ok <1 ms | 0 left, drain 2.29–2.33 s | released | pass ×2 |
| 200 ms (×2) | 36/64 | ok <1 ms | **4 left**, timeout | log 1, sm 2 held | **`three_node.rs:78:28` Orphaned ×2** |
| 250 ms (×3) | 28/64 | ok <1 ms | **16 left**, timeout | log 1, sm 2 held | **exact ×3** |
| 300 ms | 23/64 | ok <1 ms | **25 left**, timeout | log 1, sm 2 held | **exact** |
| 250 ms, sm ops only | 31/64 | ok <1 ms | **14 left** | log 1, sm 2 held | **exact** |
| 250 ms, log ops only | 64/64 | ok <1 ms | 0 left, 0.26 s | released | pass |

The counts are deterministic: same delay, same counts. They follow `left ≈ unresolved − 5 s / latency`: 36−20=16 and 41−17≈25. The engine phase never failed. The owner phase consumed the whole budget, and store release then failed only because the deadline was already gone. Latency on state-machine ops alone is sufficient; log-op latency alone is not. R5 on the same build: gated leg owners 1 left, sm held, log released → `Err(Orphaned)` as designed; ungated `Ok`.

**Disposition:** this is a reproducible *sufficient mechanism*: with induced latency on state-machine ops, U6 goes red with the exact symptom via the proposal-owner apply drain. It is **not a supported hosted cause**. Hosted 43/64 falls between the 150 ms (pass) and 200 ms (fail) rows. A uniform-latency model predicts about 21 × 170 ms ≈ 3.6 s, which passes. The hosted failure would therefore need latency spikes or a different phase. No clear code bug was reproduced: with this much unwithdrawable apply work, `Orphaned` is the contract's intended refusal (compare R5 gated). The hosted phase must be observed before any fix is chosen. No deadline change, detach, quarantine, or accepting `Orphaned`.

## Decided seam: failure-only phase report carried by `Orphaned`

Change `pub(super) struct Orphaned;` to `pub(super) struct Orphaned(pub ShutdownReport);`. Keep `Debug, PartialEq, Eq`. The hosted panic at `three_node.rs:78` then prints the report without changing the test plumbing. Every field is a bounded `Copy` integer or enum. There are no paths, SQL, owner/command data, node addresses or error strings.

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EngineEnd { Absent, Joined, JoinFailed, Deadline }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase { Lock, Engine, Owners, StoreRelease }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShutdownReport {
    pub first: Phase,              // first phase that set `failed`
    pub engine: EngineEnd,
    pub engine_ms: u16,            // elapsed since start at phase end, saturating
    pub owners_done_at_start: u8,  // reaped by try_join_next before waiting (≤72)
    pub owners_waited: u8,         // set.len() when the wait began
    pub owners_left: u8,           // set.len() at deadline (0 if joined)
    pub owners_panicked: u8,       // JoinError count
    pub last_owner_join_ms: u16,   // progress-vs-hang discriminator
    pub propose_held: u8,          // 64 - permits.available() at owner-phase end
    pub read_held: u8,             // READ_PERMITS - read_permits.available() then
    pub log_strong: u8,            // Weak::strong_count at end, saturating
    pub sm_strong: u8,
    pub total_ms: u16,
}
```

Rules: `Lock` means a poisoned owners mutex (currently `map_err(|_| Orphaned)`). Always reap completed owners with `try_join_next` and count them before the timed wait; this preserves `failed |= is_err()`. Build the report only for the return value and record no global state. The success path stays `Ok(())` and allocates nothing. Ordering, deadline, `abort_all` and weak polling stay semantically unchanged; only counters are added.

Reading on hosted: `first=Owners, engine=Joined, propose_held>0, read_held=0, last_owner_join_ms≈5000` means the slow apply drain (the induced mechanism). The same with `last_owner_join_ms≪5000` or `owners_left==owners_waited` means stuck proposal owners, not a drain. `read_held>0` means read owners. `first=Engine` means the core did not join in time. `first=StoreRelease` with `owners_left=0` means a detached holder (snapshot build, replication, blocking closure) outlived the owners.

## Tests (must fail before the seam and pass after)

1. **R5 gated** (existing, `three_node.rs` ~line 1019): replace `Err(Orphaned)` equality with a match asserting `first==Owners, engine==Joined, read_held==1, propose_held==0, owners_left==1, sm_strong>=1, log_strong==0`. Ungated still `Ok(())`.
2. **New proposal-apply leg** (add to the R5 loop or a sibling `r6_shutdown_report_phases`; ≤45 s cap): arm `adapter::APPLY_PAUSE` on the leader store (the R1 pattern), spawn one `propose`, await `entered`, remove the leader from the router and `try_unwrap`, then `shutdown()`. Assert `first==Owners, engine==Joined, propose_held==1, read_held==0, owners_left==1`, elapsed ≤5.5 s. Then send release; within 2 s weak counts reach 0 and permits return to 64; the caller gets `Err(Unknown)` or a result, never a hang. This is the deterministic, env-free form of the induced mechanism.
3. **New store-only leg**: with no owners, keep a test clone of `n.sm` alive across `shutdown()`. Assert `first==StoreRelease, owners_waited==0, owners_left==0, sm_strong>=1`, then drop the clone and see release. Proves phase 3 attribution.
4. `Cluster::stop` keeps `.unwrap()` so the report reaches the CI log; add no retry and no tolerance. `EngineEnd::{JoinFailed, Deadline}` have no deterministic red seam today. State that in the PR rather than faking one.

## Exact implementation brief (Sol)

Files: `crates/arxd/src/cluster/consensus.rs` (types plus report construction in `shutdown`) and `crates/arxd/src/cluster/consensus/three_node.rs` (tests 1–3). No other file, Cargo, workflow, ADR or PROGRESS change in that PR; PROGRESS follows the existing process. One conventional commit, `test(consensus): attribute shutdown Orphaned to a bounded phase report`, on its own branch from `main`, as a one-concern PR. Opus independent review required. Local evidence: the three tests red before / green after, fmt, Clippy, plus 8 sequential unmodified U6 runs green (sanity, not proof). Stopping budget: at most 2 implementation attempts on the seam. If the tests cannot go red-before, stop and report. This is not a fix, so it does not count toward the three-fix rule.

## Hosted attempt and stopping rule

A changed-diagnostic CI attempt is justified because the code under test differs and the output becomes informative: the seam PR's own required checks (attempt 1), then PR149 rebased onto it after it merges (attempt 2). There is no manual rerun of either. If U6 fails, the report names the phase and the next step is a design decision on that evidence. If both pass, record U6 `Orphaned` as an **unresolved natural flake, not fixed**, with the induced mechanism above as the leading but unconfirmed explanation. The seam stays in place to catch the next occurrence. PR149 unblocking then follows normal gate policy, not this note.
