# Main R5 post-release cleanup: Opus diagnosis disposition

Diagnosis only. No fix, PR, rerun, dispatch, merge, status or quarantine change.
Reviewer of record for any follow-up must be Sol (different provider).

## Identity and attempt accounting

- Worktree HEAD `113bb8a` (evidence commit); `crates/` byte-identical to main
  `c6121523417089bc0afaf307470183dd4a7f47ba` (`git diff --quiet c612152 HEAD -- crates`).
- Red under study: CI43880904887, Windows job133346204427,
  `cluster::consensus::three_node::r5_shutdown_read_owners`, `three_node.rs:1195`
  `Elapsed(())` in the 2 s wait after `gate.open()` (gated=true iteration).
- Attempts: Sol shared fixes 3/3 exhausted (unchanged). Opus fixes 0/2 unchanged:
  this round made **no fix attempt**. One focused R5 control at source was spent
  (authorized for this new symptom). PR159 natural loop not repeated.
  No timeout, deadline, retry, helper, assertion, ignore or quarantine change.

## Commands and results (local Windows 11, toolchain 1.95.0)

Isolated target `D:/Temp/arx-opus-r5post-113bb8a/target`, env
`CARGO_PROFILE_DEV_DEBUG=0` (matches CI), all `--locked --offline`, sequential.

1. Build: `cargo test -p arxd --lib --locked --offline --no-run` -> exit 0, 50.35 s.
2. **Control at exact source** (14:15:52Z):
   `cargo test -p arxd --lib --locked --offline
   cluster::consensus::three_node::r5_shutdown_read_owners -- --exact --nocapture`
   -> **PASS**, 6.85 s. gated=false `Ok(())` 1.41 ms; gated=true `Orphaned`
   first=Owners, engine=Joined, owners_waited=1, owners_left=1, read_held=1,
   propose_held=0, log_strong=0, sm_strong=2, total_ms=5009. Same report shape as CI.
   Local storage maxima Vote6/Append8/Apply9 ms vs CI Vote126/Append145/Apply192 ms.
3. Temporary tagged probe `[DEBUG-r5p]` (test-only, `three_node.rs` R5 body only,
   19 tagged lines; patch retained locally at `D:/Temp/arx-opus-r5post-113bb8a/probe.patch`,
   not committed). It keeps the original combined wait and `unwrap` unchanged and adds:
   `read_blocking` before/after shutdown; `(sm_strong, log_strong, read_permits)` before
   open, right after open and at wait end; and a std-thread observer holding only
   `Weak` clones and the `Arc<Semaphore>` (never `upgrade`) that records the first
   clear time of each predicate **separately**. Rebuild 14.64 s. Round 14:20:23-14:20:55Z.

| run (`ARX_DEBUG_R5P_CF`) | verdict | gated=true observations |
|---|---|---|
| `none` (natural) | PASS 6.86 s | rb 1->2 (read closure entered before shutdown); pre (2,0,7); after open 0.30 ms (0,0,7); sm and log first clear 0.24 ms; permits=8 by wait end 1.90 ms |
| `sm` (extra `SmAdapter` clone held across the wait) | **RED** `Elapsed` at the same wait | report sm_strong=3; log/permits clear 0.16 ms; sm stuck at 1 to 2.012 s |
| `permit` (one extra read permit held across the wait) | **RED** `Elapsed` at the same wait | sm/log clear 0.14 ms; permits stuck at 7 to 2.010 s |

Counterfactual reading: the probe goes red on each held predicate and names it,
so it is red-capable for *attribution*. The natural run is green: the actual CI red
was **not reproduced** locally. Per contract, no hypothesis ranking or source fix follows.

## Established facts (no causal claim)

- At the 5 s report the only live owner is inside the unbounded `spawn_blocking`
  read closure: locally `read_blocking` advanced 1->2 before shutdown, and the
  linearizer waits before it are deadline- or shutdown-bounded (openraft 0.10
  `try_await_ready` uses `wait(timeout)`; core is Joined). CI lacks this counter.
- sm_strong=2 at report = gate thread + one other holder after owner abort; locally
  that holder is the read closure (sm cleared as soon as the gate released).
- Ordering in `consensus.rs` closure: guard, `drop(sm)`, then `drop(permit)` (#147).
  When the closure is the last strong holder, `StateStore` teardown (rusqlite
  `Connection` close, lock `File` close, `ReleaseAck`) runs inline **between** the sm
  predicate clearing and the permit predicate clearing. Locally that gap was
  <=1.7 ms. Its CI duration is unobserved.
- openraft 0.10 `try_shutdown` joins core and tick only; the state-machine worker
  and any `build_snapshot` task are detached `C::spawn` tasks that may hold an
  `SmAdapter` after Joined. Whether one existed in CI is unobserved.
- Retained Windows logs with R5 (main149, main157, pr149, pr157, pr160, pr161):
  6 PASS at 9.27-11.67 s, 1 FAIL at 12.32 s (~typical + the full 2 s wait). Nextest
  hides passing stderr, so no passing run records post-release latency or margin.
- The CI dump is truncated (8120 bytes) and its helper attribution is `none`; D1
  values are bounded observations only. PR161 changed no cluster source.

## Why no hypothesis or repair is proposed

The CI log cannot say which of the three predicates (sm strong, log strong, read
permits) was true at the deadline, whether the read closure ever acquired the store
lock after release, whether `StateStore` teardown had begun or finished, or whether
a detached engine task held the store. Local hardware is ~20x faster on storage and
did not go red. A repeated natural loop or a timing stress harness would be the
spent PR159 pattern or tuning toward green, both excluded.

## Recommended next action: observation contract (test-only)

Goal: make the *next natural* Windows occurrence self-attributing, without changing
any budget, assertion, ignore, retry or helper semantics. Predicates stay separate.

C1. At R5 post-release, always emit one bounded line (pass and fail) tagged
    `[R5-post]`: `rb_pre`, `rb_post_shutdown`, and at open and at wait end
    `sm_strong`, `log_strong`, `read_permits`, `sm_released`, `log_released`.
    Use `strong_count`/flags only; never `upgrade` in the observer.
C2. First-clear time of each predicate from an observer that holds only `Weak`
    clones and the permit `Arc`, sampled at <=1 ms; report `None` explicitly.
C3. `cfg(test)` read-closure stage stamps next to `read_blocking` in
    `consensus.rs` (entered, lock acquired, `f` returned, `drop(sm)` returned,
    `drop(permit)` returned) as ms since gate open. This touches product source
    under `cfg(test)` only and needs Sol review; it must not reorder #147 drops.
C4. Detached-holder count: `sm_strong` minus known holders (gate, read closure
    stage) at deadline, so an engine-side holder (SM worker or snapshot builder)
    is distinguishable from the closure.
C5. Visibility on pass: either a per-test nextest `success-output` override for R5
    or the line written to the existing bounded diag dump, so margin data exists
    for green runs. Must stay under the CI time budget; no ci-time-budget bypass.

Decision table for the next red (or for green-run margins):
- sm>0, closure stage < lock acquired -> another store holder or lock wait; use C4.
- sm=0, `sm_released`=false, permit held, stage = in `drop(sm)` -> store teardown
  on the closure thread exceeds 2 s on hosted Windows; repair is a product
  ordering/design question about #147 for Opus architecture plus Sol review.
- sm>0 with closure finished -> detached engine holder; repair targets join scope.
- log>0 -> log-store holder; separate investigation.
- all clear late but <2 s on passes with shrinking margin -> report margin data;
  any budget change still requires an explicit owner decision, not this contract.

Delivery constraint: main is RED and diagnostic merges are held. The contract is a
proposal for the coordinator; landing it needs an explicit steering exception or a
reviewed repair path. Not a blanket new quarantine.

## Cleanup and limitations

- Probe reverted with `git checkout`; SHA-256 of all 113 `crates/**/*.rs` files
  identical before/after; `grep DEBUG-r5p` = 0; `three_node.rs`
  `2e7c817a...0d`, `consensus.rs` `aad13eba...da30`. No owned cargo/arxd processes
  left. Isolated target and logs remain under `D:/Temp/arx-opus-r5post-113bb8a`;
  no cache deleted.
- Single local machine, one control, one probe round; counterfactuals prove probe
  attribution, not the CI cause. Hosted-runner timing is not modeled.
- M2 open, M3 unchecked, eight Windows quarantines unchanged. Hardware paths are
  **unverified on real hardware**.
