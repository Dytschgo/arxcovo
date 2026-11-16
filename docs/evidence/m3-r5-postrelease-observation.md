# R5 post-release observation: Opus implementation evidence

Test-only attribution for the main R5 post-release red (CI43880904887, Windows
job133346204427, `three_node.rs:1195` `Elapsed`). **Not a fix.** Contract:
[brief](../briefs/m3-r5-postrelease-observation.md) on prepared `c761aed`, which
settles Opus disposition `9075daa` ([review](../reviews/m3-main-r5-postrelease-opus-disposition.md)).
Main `c612152` stays red on Windows. This PR must not merge while main is red
without separately recorded steering, and a green PR is not green main.

## Source pins

Branch `t3/m3-r5-observation-test`, implementation head `c49f297`. Commits:
`df878ab` stamps, `847b352` R5 line, `fbc929b` nextest config and guard, `c49f297`
log-holder unit case.

| Path | Blob at `c49f297` |
|---|---|
| `crates/arxd/src/cluster/consensus.rs` | `e635e61c8ccffa4152b13c87163e7476b0f04718` |
| `crates/arxd/src/cluster/consensus/three_node.rs` | `ecc78ce9de34d58108ec9b0946f216fdbd13f12d` |
| `.config/nextest.toml` (new) | `d97d8b6643f42135b5a6007af7a1c15337724244` |
| `tools/ci/test_windows_cluster_quarantine.py` | `1e6e74a5671f17cc2df6e683f3c825669f2649d7` |
| retained brief / Opus disposition | `5ef5438e…` / `27130a0b…` (unchanged) |

`git diff c761aed c49f297` touches only those four paths (+496/-3). Cargo files,
`Cargo.lock` and `.github` are unchanged. The only removed lines are the three
mechanical ones below. The test file still contains exactly eight `ignore`
attributes.

## What changed

- `consensus.rs`, cfg(test) only: a per-node `read_stages: OnceLock<Arc<ReadStages>>`.
  Each node has its own slot, so there is no global slot and concurrent fixtures
  stay separate. `fresh_read` does a single `get().cloned()` lookup. In the
  blocking closure, the first read that enters after arming claims the stamps.
  Each stage then records its first `std::time::Instant`: entered, lock
  acquired, returned (after the read function and its guard scope), `drop(sm)`
  returned, `drop(permit)` returned. Stamps are inserted between the existing
  statements. The #147 guard, store, permit, completion order and all work are
  unmoved. A stamp holds no store Arc, permit or ticket, and the closure's
  `Arc<ReadStages>` is released when the closure ends. Unarmed reads pay only
  the lookup plus `None` branches.
- `three_node.rs`, R5 only: the gated iteration arms its own node and records
  `read_blocking` before shutdown.
  - `R5Post::open` starts a std-thread sampler. The sampler holds `Weak`
    clones, the read `Arc<Semaphore>` and both `Released` flags. It never
    upgrades and sleeps 1 ms between samples.
  - The anchor is taken immediately before `gate.open()`, followed by the open
    snapshot.
  - After the unchanged wait, the end snapshot is taken. The sampler is then
    stopped and joined, and one `[R5-post]` line is emitted before the
    unchanged `unwrap()`, on both pass and fail.
  - If the sampler panicked, its panic is resumed after the line rather than
    lost. `Drop` also stops and joins the sampler while unwinding.
- Mechanical necessity: the wait result had to be bound so the line could be
  emitted before `unwrap()`. As a result, `timeout(Duration::from_secs(2), async {`
  became `let r5_wait = timeout(...`, and `.await` + `.unwrap();` became
  `.await;` … `r5_wait.unwrap();`. The predicate loop, the 2 s budget, the 5.5 s
  shutdown bound, the report assertions, the gate-release order and the final
  assertions are byte-identical.
- Line fields: wait, end time, rb counts, open/end snapshots (`sm`/`log` strong
  counts, read permits, release flags, rb), the first observed clear time of
  each of the five predicates, the five stage marks, the sample count, the
  observer state and `store_holder`.
  - Times are µs after the anchor. `pre` means before the anchor, `none` means
    missing, and `ovf` means above 9,999,999.
  - Counts above 9,999,999 render as `ovf`.
  - `store_holder` is `none` only when both end strong counts are 0, and
    `unknown` otherwise. Per the replacement for C4, nothing is subtracted to
    produce an exact detached-holder count, and no causal label is emitted.
  - The line is ASCII, has no paths, errors, IDs, addresses or wall-clock
    times, and is capped at 1024 bytes (the maximum-width line fits without
    truncation).
- `.config/nextest.toml`: there is a single default-profile override.
  - Filter: `package(=arxd) & kind(lib) & test(=cluster::consensus::three_node::r5_shutdown_read_owners)`.
  - It sets `success-output = "immediate"`. Scheduling, retry and timeout
    settings keep the defaults.
  - The Windows sampler passes `--config-file tools/ci/windows-diag-sample.nextest.toml`,
    so it does not read this file, and its exact-eight config is unchanged.
  - The e3/e4/s2b drivers pass explicit output flags and select other tests.
- Guard: pins the whole file byte-for-byte and checks its parsed TOML
  structure. It rejects 18 mutants, each through both checks: broadened or
  other filter, second override, override/profile scheduling (`threads-required`,
  `test-group`, `priority`, `test-threads`), retries, slow/leak/global timeout,
  failure/final output, and a non-default profile.

## Commands and results (local Windows 11, rustc 1.95.0, nextest 0.9.151)

Separate target `D:/Temp/arx-r5obs-c761aed/target`, `CARGO_PROFILE_DEV_DEBUG=0`,
`--locked --offline`, run sequentially. Logs are in `D:/Temp/arx-r5obs-c761aed/logs`.
No previous target was used or deleted.

1. Build: `cargo test -p arxd --lib --locked --offline --no-run` exited 0 in 61.9 s.
2. `cargo nextest list --workspace --locked --offline -E '<override filter>'`
   lists exactly one test: `arxd cluster::consensus::three_node::r5_shutdown_read_owners`.
3. Units (`cargo test … cluster::consensus::three_node::r5_ -- --skip r5_shutdown_read_owners`):
   4/4 pass before and after `c49f297`. They cover missing, before-open,
   observed clear (0 µs, 240 µs, cap boundary), overflow, count overflow,
   fixed-field rendering, holder `unknown` for sm>0 and for log-only>0,
   bounded ASCII at maximum width, and claim-once/first-instant stamps.
4. **One focused natural R5 run** (14:34:34Z),
   `cargo nextest run -p arxd --lib --locked --offline -E 'test(=…r5_shutdown_read_owners)'`
   with no output flags: **PASS** in 6.883 s, and nextest printed its passing
   stderr through the config override. The gated report is unchanged in shape
   (`Owners`/`Joined`/read_held 1/sm_strong 2/total_ms 5000). Line (281 B):
   `[R5-post] v1 wait=ok end_at=1490 rb_shutdown=2 open=sm:2,log:0,rp:7,smrel:0,logrel:1,rb:2 end=sm:0,log:0,rp:8,smrel:1,logrel:1,rb:2 seen_clear=sm:96,log:96,rp:none,smrel:none,logrel:96 stage=enter:pre,lock:39,fn:46,store:1485,permit:1488 samples=1 observer=joined store_holder=none`.
   Locally, inline store teardown on the closure thread took ≈1.44 ms
   (`fn` 46 → `store` 1485 µs). Windows 1 ms sleep granularity allowed only one
   sample before the wait ended. The run's working tree equalled `fbc929b`
   except one comment line in `R5Sampler::drop`, which was edited after the run.
   `c49f297` adds only a `Clone, Copy` derive and unit-test lines. The run was
   not repeated (no loop). The controls below used exact `fbc929b` source plus
   the throwaway patch.
5. Throwaway controls: one env-switched patch (`ARX_R5OBS_CF`), never committed,
   one rebuild (11.8 s), one run each with `cargo test … -- --exact --nocapture`.
   All three are **RED at the unchanged `r5_wait.unwrap()` with `Elapsed(())`**
   and have distinguishable raw fields:

| control | end snapshot | stages | holder |
|---|---|---|---|
| held store (extra `SmAdapter` clone) | sm:1 rp:8 smrel:0 | all reached, `store:65` (not the last holder) | unknown |
| held permit (extra read permit) | sm:0 rp:7 smrel:1 | all reached, `store:1487` | none |
| store-drop hold (existing cfg(test) `DropGate` seam via `hold_drop`) | sm:0 rp:7 smrel:0 | `fn:44 store:none permit:none` | none |

   After the patch, `three_node.rs` was restored with `git checkout`. Its
   SHA-256 matched the committed `e5efa05f…` and `CF-r5obs` occurs 0 times.
6. Rendering/stamp mutants: seven, each applied, built, tested and restored.
   Every one fails the units (rc 101): before-open→missing, overflow boundary
   `>=`, holder ignores log, release flags swapped, line cap 200, claim always
   true, stamp wrong slot. After the mutants, the hashes match the committed
   ones (consensus.rs `ab420962…`, three_node.rs `fdbeb25d…`).
7. Guard: the quarantine guard passes (3 tests). Appending `retries = 2` to the
   real `.config/nextest.toml` makes it fail with "byte-exact". The file was
   restored and its SHA-256 matches `8373eb3b…`.
8. Run once: the sampler guard (58 OK), the e3/e4/s2b driver/workflow guards
   (25 with 1 skip / 22 / 11 OK), `cargo fmt --all --check`, and
   `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
   (exit 0, 35.8 s).

## Not run / limitations

- No push, PR, status, comment, merge, watch, CI dispatch or rerun. No hosted
  reproduction is claimed, and the CI red remains unattributed until a natural
  hosted occurrence emits the line.
- No broad cluster suite was run (there was no integration concern beyond R5).
  No natural repeat loop, timing stress or Windows sampler run.
- Single local machine. The first-clear values are sampled observations only.
  An end snapshot is not atomic.
- No owned cargo, arxd, rustc or nextest processes remain. The isolated target
  and logs stay under `D:/Temp/arx-r5obs-c761aed`.
- Sol must independently review the actual final source before publication.
  All five exact-head gates are still required.
- Attempt counters are unchanged: Sol shared fixes 3/3, Opus Windows fixes 0/2.
  No implementation correction was spent: `c49f297` is a pre-review test
  addition. M2 open, M3 unchecked, eight Windows quarantines unchanged.
  Hardware paths are **unverified on real hardware**.
