# M3 A0 bounded fresh read: refusal-contract decision and independent Opus review

Reviewer: Opus 5.5 high, 2026-11-16. Target `32bce78d60f587a587ee71542ca8bcc8ca7b17fe`, base `6dab2e4`.
Sol builder commits: `87e80dc` (ex-`0e48f21`, source) and `71e3849` (ex-`24a147b`, evidence). `32bce78` changes only
`docs/PROGRESS.md`. Against the base, the source delta is limited to the three allowlisted files `consensus.rs` (+111 net),
`consensus/adapter.rs` (+25) and `consensus/three_node.rs` (+283), plus `docs/evidence/m3-bounded-fresh-read.md`. No
dependency, Cargo, workflow, ADR or brief edits are included.
Contract: brief `docs/briefs/m3-certificate-issuance-first-slice.md`, decision 1 and "PR A0 files and tests", together with
the C1–C3 Sol review it closes, AGENTS, KICKOFF and CONTRIBUTING. Primary source: the cached `openraft-0.10.0` and
`openraft-rt-tokio-0.10.0`.

**Not run by this reviewer:** build, tests, clippy and fmt. I read the source, and its behavior is clear without them. All run
results quoted here are the builder's, from the evidence file. Hosted CI has not run. CA/TLS/PKI (A and B), M2 native work,
grants, VMs and admin steps are out of scope and were not touched.

## Decision: approve the coordinator's minimal refusal policy

**Approved.** An isolated-leader read may refuse with `Unavailable`, which is the truthful caller-deadline refusal.
`NotLeader` and `NoQuorum` keep their mapping, so they are returned whenever the engine actually replies before the
deadline. The 2.5 s caller bound, non-success, and permit drain all stay strict. No sub-budget is taken, and no
deadline is relabeled.

### Why the brief conflicts internally (verified against primary source)

- `raft_core.rs:405–461` (`handle_get_linearizer`) takes `now = C::now()` **when the request is dequeued**. It then
  computes `wait_timeout = effective_wait_timeout(..)`, which is our `Some(remaining)` (`linearizer_option.rs:116–117`).
  It stores `PendingRead { deadline: now + wait_timeout }`.
- `pending_read_queue.rs:51–66` (`drain_expired`) sends `QuorumNotEnough` only once `deadline <= now`.
- `raft_core.rs:1053–1080` (`process_pending_reads`) is the only other producer. On leadership loss it sends
  `ForwardToLeader` through `drain_all_with_error`. Otherwise the only other `QuorumNotEnough` producer is the
  `wait_timeout.is_zero()` fast path at line 449. That path is reached only when `remaining` has already saturated to zero,
  which means the caller deadline has already passed.
- C::now is `tokio::time::Instant` (`openraft-rt-tokio lib.rs:45`), the same clock as `fresh_read`. `remaining` is computed
  before the send, and dequeue happens at or after the send, so **engine deadline ≥ caller deadline**. Queue delay only
  widens that gap.
- Decision 1 then requires `timeout_at(deadline, rx)` → `Unavailable`. It also requires that no result is delivered once the
  deadline has passed. The owner enforces this with `now < deadline` before `tx.send`.

As a result, with the pinned OpenRaft and decision 1, `NoQuorum` **cannot be observed** through `fresh_read`. `NotLeader` is
observable only if the isolated leader steps down, or is already a follower, before the caller deadline. OpenRaft 0.10
gives no such guarantee. R2 and the T2 read assertion, which require exactly `NotLeader | NoQuorum`, therefore test an
outcome that the contract itself makes unreachable. The builder's 1.515 s `Unavailable` is the expected behavior, not a bug.

### Rejected alternatives

- A shorter quorum sub-budget (`wait_timeout = remaining − margin`): forbidden by the coordinator. It would also only move
  the race, because a dequeue delay larger than the margin brings it back.
- Relabeling a deadline as `NoQuorum`: this claims an engine verdict that never arrived. It is not truthful.
- Waiting past the caller deadline for the engine verdict: this breaks the absolute 1500 ms caller bound, which is the
  whole point of advisory 2.
- Removing `ReadError::NoQuorum`: this is API churn outside A0's single concern. Keeping it costs nothing and remains correct
  if OpenRaft behavior changes. PR A folds it into `Unavailable`/`RetryLater` anyway (see advisory A3).

### What "truthful deadline refusal" must mean in the test

`Unavailable` is not accepted from every source. It counts as the deadline refusal only when it arrives **at or after**
`READ_BUDGET`. `started` is sampled before the call, and `deadline` is computed inside `fresh_read` at or after it. Every
deadline path then has `now ≥ deadline`: the `timeout_at` elapse, the post-receive `now < deadline` guard, and an owner that
withholds its send. So `elapsed ≥ READ_BUDGET` follows for those paths. An `Unavailable` that arrives early is a different
failure (Fatal, poisoned owners, `try_await_ready` error, `Corrupt`, or an owner that panicked or dropped `tx`), and R2 must
still fail on it. This keeps R2 discriminating instead of merely widening it.

## Exact coordinator delta (small; the coordinator may apply it)

Sol must check the brief change before the test change lands, because it is a new policy decision from Opus.
Both changes then need a fresh delta recheck from Opus.

### D1. `three_node.rs`, `r2_isolated_read_refusal`

Replace the final `matches!` assertion with this one, and measure the permit drain from `started`:

```rust
        let result = c.node(l).linearizable_state().await;
        let elapsed = started.elapsed();
        read_units_return(
            c.node(l),
            Duration::from_millis(2500).saturating_sub(started.elapsed()),
        )
        .await;
        // ...eprintln, isolation clear and finish unchanged...
        // OpenRaft 0.10 starts its quorum timer at dequeue with `remaining` computed at
        // submission, so its QuorumNotEnough cannot precede the caller deadline.
        assert!(
            matches!(result, Err(ReadError::NotLeader | ReadError::NoQuorum))
                || (result == Err(ReadError::Unavailable) && elapsed >= READ_BUDGET),
            "R2 refusal={result:?} elapsed={elapsed:?}"
        );
        assert!(elapsed <= Duration::from_millis(2500));
```

The drain budget change implements the brief's "all permits return in 2.5 s" literally. Today the 2.5 s window restarts
after the caller returns, which allows up to about 4 s from the start. The builder's run returned all units by 1.515 s,
so this leaves about 1 s of margin. `READ_BUDGET` is in scope through `use super::*`, the same way `READ_PERMITS` is.

### D2. `three_node.rs`, `t2_leader_offline_reads_and_unknown_retry`: one pattern only

```rust
        assert!(matches!(
            read,
            Err(ReadError::NoQuorum | ReadError::NotLeader | ReadError::Unavailable)
        ));
```

Every other T2 line stays unchanged: the 3 s `timeout(..).unwrap()`, the follower `NotLeader`, propose, replay and
convergence. R2 carries the deadline discriminant, so T2 does not need its own timing code.

### D3. Brief, decision 1 and the A0 test list (Opus owns the brief; Sol checks it)

- Decision 1, **Honest limits**: append "OpenRaft 0.10 starts the quorum `wait_timeout` at dequeue, and `remaining` is
  computed before submission, so its `QuorumNotEnough` never precedes the caller deadline. An isolated leader's caller
  therefore normally gets `Unavailable`, the deadline refusal. `NoQuorum` or `NotLeader` is returned only when the engine
  replies first, for example `ForwardToLeader` after step-down. No sub-budget is taken, and a deadline is never relabeled."
- R2 line: replace "an isolated leader gives `NotLeader`/`NoQuorum` in ≤ 2.5 s" with "an isolated leader refuses in ≤ 2.5 s:
  `NotLeader`/`NoQuorum` if the engine replies before the deadline, otherwise `Unavailable` at or after `READ_BUDGET`, and
  all permits return within 2.5 s of the call".
- R5 tail ("All existing tests pass unchanged") and the PR A line "existing T1-T4 … pass unmodified": add the exception "except
  T2's isolated-read pattern, which also accepts `Unavailable` (A0 decision 1 deadline)".

The coordinator should not change anything else. Source code needs no change for this decision.

## Standards axis: PASS (no blocker)

- The change is limited to the three allowlisted files plus evidence. It adds no dependency and uses no `unsafe`. The new
  error variants carry static data only. Test `eprintln!` lines print only enums and durations. There is no new
  suppression; the builder fixed the clippy helper-scope note lexically.
- Each new test proves a behavior on actual OpenRaft nodes. The R1 and R3 counterfactuals failed at the old 4 s detector,
  and the permit-early mutant failed with 8 versus 7. These were genuine reds before green, per the evidence, and the
  builder ran them. I did not rerun them.
- Nit, no action: `consensus.rs` is now 526 lines, up from 440. The A0 brief sets no size cap, and the production delta is
  about 110 lines.

## Spec axis: conforms to decision 1, except the conflict decided above

I traced each step:

- **Caller.** `deadline` is computed first, then `try_acquire_owned` → `Busy` before any lock or spawn. Under the `owners` std
  mutex, the caller checks `closed` → `Unavailable`, reaps, clones, and spawns into `owners.set`, with **no await under the
  lock**. Then `timeout_at(deadline, rx)`. The `rx` is moved into the timeout and dropped with it, so a late send sees
  `is_closed()`. Accepting only `Ok(Ok(r)) if now < deadline` is stricter than the brief, and correct.
- **Owned task.** Steps 1–4 match decision 1: the pre-submit deadline and `is_closed` check; `get_read_linearizer(..)`
  awaited with no timeout and no select, which is the only proof that the engine request has ended; the
  ForwardToLeader/QuorumNotEnough/other mapping; the re-check; `try_await_ready(Some(remaining))`, where anything other than
  `Ok(Ok)` gives `Unavailable`; the re-check; and the blocking closure.
- **Queued request after caller cancel.** The owner task is owned by the JoinSet, not by the caller. Aborting or dropping
  the caller only closes `tx`. If the request was already submitted, the owner still awaits it to completion, so the
  engine reply always arrives before the permit drops on that path. R4 shows that an aborted caller keeps its unit, and R5a
  shows that a cancelled in-flight unit resolves through `try_shutdown` → Fatal.
- **The eight-unit bound and release order.** `permit` and the `sm` clone move into the async block. On an early return they
  drop when the engine or ready wait ends, which is after the engine reply. On the closure path they move into the
  `spawn_blocking` closure. Inside it, the inner closure's guard drops first, then explicit `drop(sm)`, then
  `drop(permit)`, and only then is `result` returned and the deadline-gated send made (#147). The proposal semaphore (64)
  is untouched and is asserted in R4. One clarification: a read unit's permit leaves the pool only through
  `try_acquire_owned`, and every acquired permit is either dropped before spawn (closed or lock-poisoned) or owned by the
  task or closure.
- **Shutdown.** `closed` is set before `try_shutdown`, so no new unit can be admitted after closure (`Unavailable`, asserted
  in R5 after shutdown). The 5 s join, then `abort_all`. An aborted owner that is awaiting `spawn_blocking` leaves the
  closure running with its `sm` Arc and permit. `sm_weak` is still live, so `Orphaned` is reported, which is honest and not
  a hard-termination claim. After release, the closure drops the Arc and then the permit. R5b asserts both.
- **Future panic.** If the owner's async code panics, the task fails. `tx` and any held permit or Arc drop during unwind.
  The caller sees `RecvError` → `Unavailable`. Shutdown's `join_next` records `is_err()` → `Orphaned`, so nothing is hidden.
  A panic inside `f` gives a `JoinError` → `Unavailable`, and the closure's captures drop during unwind before the
  `JoinError` is observable, so ownership stays correct. See advisory A1 for the poisoning side effect.
- **Test hooks.** `APPLY_PAUSE` is awaited before `run`, so it holds no store lock. It is keyed by the Arc pointer and
  `take()`n on first match. If R1 panics, the dropped `release` sender makes a stale hook resume immediately, so it cannot
  wedge another test. `StoreGate` holds the mutex on a dedicated `std::thread` with RAII release, never on a Tokio worker.
  `read_blocking` is `#[cfg(test)]`.

## Security axis: PASS

The change adds no new input surface, network listener, secret, owner data, log content or filesystem path. Admission is
bounded, so a wedged RaftCore yields `Busy` instead of growing without limit, which closes the unbounded-wait half of
advisory 2. No mutex is held across an await, and the store mutex is acquired only on blocking threads. A late or
cross-deadline read result can never be observed by the caller.

## Advisories (non-blocking; none needs a fix before merge)

- **A1. Panic in `f` poisons the store mutex.** The guard drops during unwind, so later `apply` calls get `Poisoned`, and the
  state machine becomes Fatal for that node. This is the same property the existing `run` apply path has. A0's `f` is
  `(applied, snapshot)`. PR A's `f` must stay panic-free (no `unwrap`/indexing on store data), and its review should
  check for this.
- **A2. Release order on the panic path is unspecified.** During unwind, the closure's `sm` and `permit` captures drop in
  capture-field order, not in the explicit #147 order. Both are released before the `JoinError` is returned, so only a
  concurrent observer polling permits and weak references could see the permit first. Nothing depends on this today.
- **A3. PR A mapping.** Decision 6 should state `ReadError::NoQuorum → IssueError::Unavailable` explicitly, next to
  `Busy → Busy`. This belongs to PR A brief scope, not A0.
- **Carried forward unchanged:** store-file advisory 1 (Weak expiry does not prove the file is closed) stays open. The 49
  non-cluster workspace failures reproduce on the exact base (per the builder's name-set comparison). They are unrelated
  to A0 and need no admin fix here.

## Status: not green

- R2 and T2 fail on `32bce78` as committed. This is an **expected contract conflict**, resolved by D1–D3, not a source bug.
  No other blocker was found in the A0 source.
- Required before merge: Sol checks D3; the coordinator applies D1–D3; the builder reruns the cluster tests, fmt and
  workspace clippy and records them; Opus does a fresh delta recheck of the correction; then a different-provider
  review, green hosted checks (three OSes, ci-time-budget, agent-review), and a coordinator-only merge.
- This review gives no M3 acceptance. Real-host, VM and hardware paths are **unverified on real hardware**.
