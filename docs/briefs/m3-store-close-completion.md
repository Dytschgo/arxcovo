# M3 store-file advisory 1: destructor-completion signal before the first restart caller

2026-11-16. Anthropic / Claude Opus 5.5 / high, research sub-agent. Docs only. Base: prepared PR B docs `51706df`, on pending PR A source `c1cdecd` (main
`f1c399d` at the time of writing). Does not touch PR157 or the PR B docs. No source, test, dependency, PROGRESS or ADR edit. No build, test, mutation, VM, admin, native M2,
CI, push, PR, status, merge or watch. M2 stays open. M3 stays unchecked. The eight Windows three-node ignores are unchanged. Hardware paths are **unverified on real hardware**.

**Decision: settled as a contract, not yet implemented.** Each store gets a final-field drop-ack guard. Its `Drop` stores `true` with `Release` into a shared
`Arc<AtomicBool>`. `ConsensusNode::shutdown` waits, with an `Acquire` load and the **unchanged** 5 s deadline, for both flags to be set. It no longer stops at
`Weak` expiry. There is no new dependency, no `unsafe`, no `ManuallyDrop`, and no new timeout, retry, scheduling or quarantine. The fix does not need a production
restart caller. It must land before the first one (`m3-enrollment-certificate-next-slice.md` advisory 1).

## Defect (from source)

- `shutdown` (`consensus.rs`, StoreRelease phase) loops `while log_weak.upgrade().is_some() || sm_weak.upgrade().is_some()` and then returns `Ok(())`.
- `Arc::drop` decrements `strong` (`fetch_sub(1, Release)`). Only the thread that saw the count at 1 runs `drop_slow`, which calls `drop_in_place(T)`. So `strong == 0`, and
  with it `Weak::upgrade() == None`, becomes observable **before** `T`'s destructor has started. Source: `library/alloc/src/sync.rs` (`impl Drop for Arc`, `drop_slow`). This was
  not re-read at the 1.95.0 tag in this session because no local rust-src is installed. The implementer must cite that tag (see Prerequisite).
- The last strong `Arc<Mutex<StateStore|VoteLogStore>>` is often dropped on a `spawn_blocking` thread. Examples are `LogAdapter::run`, the apply/append closures in
  `adapter.rs`, and `fresh_read`'s blocking closure, which drops `sm` before its permit (#147). `self.sm.take()` in `shutdown` covers only the node's own clone. The
  destructor that closes `db: Connection` and `_lock: File` can therefore still be running on another thread when `shutdown` returns `Ok(())`. An immediate reopen
  then fails `lock.try_lock()` in `open_db` with `StoreError::Io`.
- Existing symptom: `startup_accepts_exact_empty_and_ordered_hole` (scenario 17) already retries `VoteLogStore::open` on `Io` for 2 s after `shutdown().await.unwrap()`.
  That loop is left unchanged here. Removing it is a later, separately reviewed cleanup.

## Why a last-field guard proves destructor completion

- The Reference (Destructors) says struct fields are dropped **in declaration order**. Current order is `StateStore { db, image, blob, poisoned, _lock, #[cfg(test)] hook }`
  and `VoteLogStore { db, meta, poisoned, _lock, #[cfg(test)] hook }`. A new field declared **last** drops after `db` (rusqlite `Connection::drop` flushes the statement cache.
  `InnerConnection::drop` then calls `sqlite3_close`) and after `_lock` (std closes the handle/fd).
- `Release` on store paired with `Acquire` on load means everything the field destructors did happens-before `shutdown` observes `true`.
- The Rustonomicon calls relying on field order fragile. The mitigation is a comment on the field ("must stay last") plus the store-level test below. `ManuallyDrop` is
  rejected because the brief excludes it and it would need `unsafe` or explicit drop code.

## Exact meaning, and what it does not claim

- `true` means the destructors of every store field have **returned or unwound**. Drop glue keeps dropping the remaining fields if one field destructor panics.
- It does **not** prove `sqlite3_close` succeeded. rusqlite 0.37.0 `InnerConnection::drop` ignores `close()`'s `Result` (`#[expect(unused_must_use)]`), and a failed
  close leaves the handle open. Every `Statement`/`Transaction` borrows the connection, so none can outlive the store, and the cache is flushed first. A failure would come from inside SQLite,
  and nothing reports it. File close errors are likewise ignored by std. The real proof stays the next open, which fails closed (`Io`/`Sql`). No retry is added.
- Windows: std `try_lock` uses `LockFileEx`. Microsoft documents that locks are released on close, but "the time it takes … depends upon available system resources".
  The ack proves the handle was closed. It does not prove the OS released the lock. An explicit `File::unlock()` before close is a **separate Windows-stability candidate**,
  not part of this fix.
- Mutex poisoning does not stop `Drop` (dropping a poisoned `Mutex<T>` still drops `T`). The guard gives no in-process recovery or reuse guarantee for a store or a node.
- Open and constructor failure paths need no signal. Each `open` builds the guard only in its final `Ok(Self { .. })`. Earlier `?` returns drop `lock`/`db` locals
  synchronously in the caller's frame. In `start`, both opens and `preflight` run inside one `spawn_blocking` closure, which finishes (or unwinds) before `.await` returns. A
  `params.config()` failure drops the sole `Arc`s synchronously on return. `Engine::new` failure (`StartError::Fatal`) has unknown openraft ownership, and this fix makes
  **no release claim** for it. A future restart caller must not treat `Fatal` as released.

## Scope, types and API (one isolated PR; files: `state_store.rs`, `log_store.rs`, `consensus.rs`, tests)

```rust
// state_store.rs, beside open_db (shared opener). std::sync::{Arc, atomic::{AtomicBool, Ordering}}.
#[derive(Clone)] pub(super) struct Released(Arc<AtomicBool>);
impl Released { pub(super) fn is_set(&self) -> bool { self.0.load(Ordering::Acquire) } }
pub(super) struct ReleaseAck(Arc<AtomicBool>);           // constructor private to the module family
impl Drop for ReleaseAck { fn drop(&mut self) { self.0.store(true, Ordering::Release); } }
pub(super) fn release_pair() -> (ReleaseAck, Released);
// In both stores, declared LAST, after `_lock` and the cfg(test) hook:  release: ReleaseAck,  // must stay last
// Both stores: pub(super) fn released(&self) -> Released
#[cfg(test)] pub(in crate::cluster) struct DropGate { entered: std::sync::mpsc::Sender<()>, release: std::sync::mpsc::Receiver<()> }
// Drop: let _ = entered.send(()); let _ = release.recv();   (a dropped test sender unblocks it, so it cannot hang)
// In both stores, cfg(test), declared IMMEDIATELY BEFORE `_lock`: gate: Option<DropGate>;  + cfg(test) fn hold_drop(&mut self, g: DropGate)
```

- `ConsensusNode` gains `log_released: Released` and `sm_released: Released`. They are taken in `start` from the opened stores before the `Arc::new` wrapping. `log_weak`
  and `sm_weak` stay: they feed `log_strong`/`sm_strong`, the diagnostics, the test helpers and `peer_registry`'s per-call upgrade.
- `shutdown`: only the StoreRelease wait condition changes, to `while !(self.log_released.is_set() && self.sm_released.is_set()) { yield_now }` inside the same
  `timeout_at(deadline, ..)`. A set flag implies `strong == 0`, because the guard drops only inside `T`'s destructor, so the old `Weak` condition is subsumed.
- `ShutdownReport` is unchanged in shape and meaning. `log_strong`/`sm_strong` are still the true strong counts. `first: StoreRelease` with both counts at 0 now
  truthfully means "destructor still running at the deadline". Lock, Engine and Owners phases, the 5 s first phase, `propose_held`/`read_held` capture-before-abort,
  owner/abort ordering and read/proposal ownership (#147 guard → Arc → permit → completion) are untouched.
- Test hooks compile out (`#[cfg(test)]`). No production behavior other than the wait condition changes. No restart caller is added.

## Deterministic regressions (each must be red on the old wait, recorded before the fix)

1. **Store level** (`state_store/tests.rs`, `log_store/tests.rs`, one per store). Open, then take `released()` and `hold_drop(gate)`. Drop the store on a `std::thread`.
   Wait for `entered` (bounded recv). Assert `!is_set()` and that a reopen of the same dir returns `Err(Io)`, which proves the gate sits before the lock release. Release the gate
   and join. Assert `is_set()` and that an **immediate** reopen succeeds with no retry. Mutation: moving `release` before `gate` must turn this red.
2. **Shutdown waits** (`consensus/tests.rs`, single node, mem_net, not three-node, so it gates all three OS). Start, install a gate on the `StateStore`, and keep a test
   clone `held` plus its own `Weak`. Spawn `shutdown`, wait (bounded) until `Arc::strong_count(&held) == 1`, then drop `held` on a `std::thread` and wait for `entered`.
   Assert `weak.upgrade().is_none()`, so `Weak` has expired while the lock is held. Then `timeout(500 ms, &mut handle)` **must elapse**. Old code returns `Ok(())` here: red,
   "shutdown returned while store destructor held its lock". Release the gate. `shutdown` returns `Ok(())`, and an immediate `ConsensusNode::start` on the same dir
   succeeds and shuts down `Ok(())`. Assert that total elapsed time is under 5 s before the 500 ms window.
3. **Deadline truth.** As in 2, but the gate is held past the 5 s deadline. Expect `Err(Orphaned(r))` with `r.first == Phase::StoreRelease`, `r.engine == Joined`,
   `r.sm_strong == 0` and `r.log_strong == 0`. The old code returns `Ok(())`: red. Release the gate, then join, then an immediate reopen succeeds. This adds about 5 s of test time, within the CI budget.

Test-only bounded waits are fixture bounds, not production timeouts. Existing tests (T1–T4, U6, R1–R6, crash probes, the scenario 17 retry) run unmodified. The red
runs on the old wait and the mutation are recorded as evidence. They are not claimed until run.

## Accounting boundary

- This brief addresses **source-correctness advisory 1**. It is not a Windows three-node recovery attempt. It charges no Opus recovery attempt and launders none, and it
  claims no historical flake fixed. The U6 hosted error 33 (`m3-u6-shutdown-diagnosis.md`) remains "consistent with" this advisory, with no cause shown. The eight
  ignores, the sampler and ADR 0024 are unchanged. A green does not earn re-enable credit. Windows remains **unverified on hosted Windows** for three-node and
  **unverified on real hardware**.

## Risks

- Field order is a convention. The comment, regression 1 and review guard it, but a reorder between `gate` and `_lock` without moving `release` is not detected.
- Ignored close errors (SQLite, std) and asynchronous Windows unlock can still fail a reopen after the ack. That failure is fail-closed and observable, never silent.
- Regression 2's elapse window is a negative timing check. The old code fails it deterministically. The fixed code can only pass it early if the 5 s deadline expires, which the
  elapsed assertion excludes.

## Prerequisite before implementation (actual main pin recheck)

On the then-current main SHA (recorded in the PR), the implementer must reconfirm: the store field lists and `open_db` sharing above; that `start` is the sole
`ConsensusNode` constructor; the StoreRelease loop text; rusqlite `=0.37.0` drop behavior and toolchain `1.95.0`, with the `sync.rs` drop sequence cited at that tag;
and that no production restart caller exists. Pins at writing: `state_store.rs` blob `90474881230e`, `log_store.rs` `02db51fc2409`, `adapter.rs` `181e9dce6765`, and `consensus.rs` `b7d1f350afb6` (main) /
`c93c58f624e9` (`c1cdecd`, which adds only the `issue` module). On any drift, stop and re-settle.

## Review route

Root Sol reviews this contract. The future isolated main-based PR is implemented separately, gets an independent different-provider review (Opus 5.5 high for a Sol-built change), and merges only with the five required
gates green and green main. It contains no PR157 or PR B content.

Sources: Rust Reference, Destructors (doc.rust-lang.org/reference/destructors.html). Rustonomicon, Drop Check. rust-lang/rust `library/alloc/src/sync.rs`. Microsoft
LockFileEx (learn.microsoft.com/…/nf-fileapi-lockfileex). rusqlite 0.37.0 `src/inner_connection.rs:140-165,402-408` and `src/lib.rs:353-358` (local registry copy).
