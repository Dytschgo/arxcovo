# M3 OpenRaft adapter and three-node proof: independent Opus review

Reviewer: Opus 5.5 high, 2026-11-15. Target `3fb24d280d60a8a83e6e34e8493c2b6c03932b84`
(`work/m3-openraft-adapter-boundary`), base main `38750cf1e179e14da6fa871972027b6d66028ec9` (PR #144).
Builder commits: `5cd8c6b` (ex-`7d60a76`), `93651ed` (ex-`7b4bf62`), `0a74643` (ex-`e39535f`), `40564a3`
(ex-`095504c`). Each rewritten commit has a tree identical to its original (checked with `git rev-parse <c>^{tree}`).
`40564a3..3fb24d2` changes only `docs/PROGRESS.md`. Contract: brief `docs/briefs/m3-openraft-adapter-first-slice.md`
(200 lines, startup correction `66a9bf8`), Sol review, ADR 0005 startup invariant, ADR 0023, and evidence lines 1–141.
The historical handbacks below line 141 were read as context only. They earn no credit here.

## Verdict

- **Standards: PASS.** No blocker. The low-severity notes are listed below.
- **Spec: PASS for the exact in-process slice.** T3 (snapshot catch-up) is not implemented. It is an explicit,
  uncredited **immediate follow-up**, as the brief allows: the source delta is 1752 lines, past the ~1800-line slice rule
  once the evidence is included. No RPC snapshot credit is given (`snapshots=0` in every fixture).
- **Security: PASS.** There are no concrete blockers.
- **Eligibility:** the slice can go to merge once the required hosted gates (three OSes, ci-time-budget, agent-review)
  are green and the coordinator merges. None of those gates have run for this head. This review gives no KICKOFF M3
  acceptance.

## Scope and dependency lock (verified)

- The only changed source paths are `cluster.rs` (`pub mod consensus`), `enrollment.rs` (static `Display`: "enrollment
  mint"/"enrollment redeem"), `state_store.rs` (`StoreError::Membership`, pure `peek`), and the new `consensus.rs`,
  `adapter.rs`, and the `#[cfg(test)]` files `mem_net.rs`, `tests.rs` and `three_node.rs`. `log_store.rs` is unchanged
  (diff 0 lines). The builder commits make no ADR, workflow or gate edits. The ADR 0005 line comes from Opus `66a9bf8`.
- There is no production caller. Nothing outside `cluster/consensus*` references the module. There is no listener,
  socket, transport, CA, signing or tracing subscriber.
- I parsed and compared `Cargo.lock` against main myself: 208 → 243 packages, 35 added, 0 removed. No existing
  package's version, source or checksum changed. The only changed dependency edges are `arxd +futures-util +openraft`
  and `chrono +js-sys +wasm-bindgen`. Running `cargo tree -e features` (locked, offline) shows only `openraft feature
  "tokio-rt"`, with no serde, clap, bt, single-threaded or type-alias. `rand 0.8.8` and `regex` come only through the
  `chrono-tz-build` build-dependency (via `display-more`), not as normal edges.
- File sizes: `consensus.rs` has 406 lines and `adapter.rs` has 309. The 380-line cap was superseded by the
  brief's current **≤ 420** cap (changed in `66a9bf8`), so 406 conforms. I found nothing concrete in production
  code that would justify cosmetic churn. Moving `Params::config` next to the adapters is a cohesion nit only.

## Contract conformance (source read against the pinned 0.10.0 source)

- **Types, mapping and codec.** `declare_raft_types!(Arx: D = Command, R = Applied)` keeps every other default.
  Mapping is total. The test compares vote order directly with `StoredVote` order. The membership codec allows 1–2
  configs, 1–16 strictly ascending nonzero voters each, and 1–16 nodes with addresses of 1–200 bytes from the allowed
  alphabet. Decode needs a canonical re-encode, and every error is `Membership`. Encode never truncates. The worst
  case is 3605 B and every truncation is rejected.
- **Debug/Display.** `Command`'s `Debug` exposes the code id, the verifier (hash of the proof), the key digest and
  ids. It never exposes the proof, the secret or the owner code text, because `RedeemProof`/`JoinCode` redact
  themselves. This is acceptable only because future ingress must accept proofs and never verifiers
  (documented). Re-review if a tracing subscriber is ever added.
- **C1 (decision 6).** `append` is a plain `fn` whose first statement is `Completion::new(callback)`. It runs before
  `into_iter().take(65)`, before conversion, and before the store clone. Errors found while preparing the batch finish
  synchronously. The closure that `spawn_blocking` runs finishes only after `VoteLogStore::append` returns.
  `Completion::Drop` completes `Err` only if the callback has not been consumed. That covers the unpolled-drop, outer
  panic, closure-dropped-before-start and closure-panic cases. Because `IOFlushed` is consumed by value, it completes
  at most once.
- **C2 (decision 12).** `try_acquire_owned` happens before any `.await`, and the owners mutex is never held across an
  `.await`. If admission is closed the call returns `Stopped`. The owner is spawned only into the `JoinSet`, takes
  `ProgressResponder::complete_only()`, and holds the permit until it gets the final result, a `Fatal`, or a closed
  receiver. It drops the permit before replying. The caller awaits only the reply receiver. I checked the
  classifications against the source:
  - `ForwardToLeader` is raised only before an append (`raft_core.rs` 490/543/718/1869), so `NotLeader` means "not
    appended".
  - `LogEntryDiscarded` (truncate/purge), a closed receiver and an owner panic map to `Unknown`.
  - A `client_write_ff` `Fatal` maps to `Unknown`. That is conservative, because the request was in fact never queued.
- **Shutdown.** It closes admission, then calls `try_shutdown` *before* joining owners. Owners are joined under one
  shared 5 s deadline, and on timeout it runs `abort_all`, joins the rest and returns `Orphaned`. It then drops its own
  `Raft`/`SmAdapter` and waits for both store `Weak`s to expire. If ownership is dropped without `shutdown`, dropping
  the `JoinSet` aborts the owners. Once the last `Raft` handle drops, `tx_shutdown` is dropped and the core exits
  (`Select4::First`). So nothing is leaked silently, though it is not joined either.
- **Locks.** Every lock and store call runs in `spawn_blocking`. A poisoned mutex maps to `Poisoned` and nothing calls
  `into_inner`. Errors are `io::Error::other(StoreError)` with static text.
- **Preflight (decision 9 / ADR 0005).** It runs in the same blocking closure that opens both stores, before any
  `Arc`, adapter or `Raft` exists. It reads A, P and L. It rejects an applied index above `i64::MAX`, uses a checked
  `a+1`, and reads at most one store-validated row. The first matching rule decides:
  - P past A: refuse.
  - P at A's index: accept only if P equals A.
  - A hole (L is `None` or below A's index): accept only if `L < Some(A)` in `LogPos` order.
  - Otherwise: accept only if there is exactly one row at A's index and it equals A.

  Refusals return `Corrupt`, wrapped as `StartError::Preflight`. The 19 fixtures match the R1–R4 list one for one. That
  is 12 refusals: higher, lower and other-leader rows, P = L = T2-N1.5, both interior mismatches, a missing row, both
  unordered hole tails, SM leader 2 over a leader-1 row, behind purged, and nothing applied with P set. It is 7 accepts:
  empty, log only, exact, retained under a newer tail, P = A with a tail, and both ordered holes. Every refusal leaves
  vote, log state, every retained row and the SM snapshot bytes unchanged.
- **Raw helper.** The raw-helper test is labelled as characterization only. It asserts exactly what was observed:
  `Err` for scenarios 1, 3, 6, 9, 10 and 11. Two of those observations differ from the brief's exploratory guess, and
  the evidence says so honestly. Only behind-purged is claimed as a raw `Raft::new` `Fatal::StorageError`. No upstream
  equal-index `Fatal` is invented.
- **Snapshots.** `get_current_snapshot`/`build_snapshot` read applied, membership and blob under one lock. Install
  runs `peek` (bounded at 16384 B before hashing in `decode`), requires an exact applied/canonical-membership match,
  then installs under the same lock, and returns `Corrupt` on a mismatch. I4 stays protocol-held. No
  cross-store-atomicity claim is made.
- **Suite.** I checked the partition against `testing/log/suite.rs`: 46 `run_test` cases, of which 27 are S1 and 19 are
  S2, plus `transfer_snapshot`. None are missing, extra or duplicated. Each case runs in its own spawned task, and a
  panic or an unexpected `Ok` fails the test. The SM consecutive-order rule is not weakened. This is **not** full
  conformance, and the evidence says so.
- **T1/T2/U6** match the brief:
  - T1: 2/3 continuation after a follower goes offline, reuse gives Consumed, competing keys give exactly one Enrolled
    {5} and one Consumed, logical expiry, BadProof, identical replay, and byte-identical convergence after restart. Blob
    equality also implies applied equality.
  - T2: election, continued writes, a ReadIndex read covering every ack, a follower read gets `NotLeader`, the old
    leader's durable SM reopens with applied ≥ every ack, and no-quorum writes and reads are refused. The unknown retry
    is shown at-most-once by node ids 5 and then 6.
  - U6: non-vacuous. It requires 64 actual entries past `local_committed`, then `Busy` with 0 permits, then shutdown
    without `Orphaned`, 64 permits, an empty set and `Stopped`, for both the isolated and the restored fixture.

## Checks I ran (Windows local)

All checks used my own target `D:/Temp/arxcovo-openraft-review-target`, with TEMP/TMP set to its `tmp`, `--locked
--offline`. Nothing was installed, fetched, deleted or cleaned.

| Check | Result | Time |
|---|---|---|
| `cargo nextest run -p arxd cluster::` (clean HEAD) | **98 passed**, 456 skipped: the old 36, both four-kill probes, Suite, R1–R4, T1/T2/U6 | 8.656 s run, 1 m 43 s cold build+run; T1 1.238 s, T2 7.857 s, U6 1.718 s |
| `cargo fmt --check` / `cargo clippy -p arxd --all-targets -- -D warnings` / `git diff --check 38750cf HEAD` | pass | 1 s / 32 s / – |
| Lock parse, `cargo tree` features, Suite name diff | as stated above | – |

I also ran counterfactuals on temporary source mutations. Each was reverted with `git checkout`, and I confirmed `git
diff --quiet HEAD` and an empty status afterwards:

- C1: moving `Completion::new` into the async body made `append_eager_unpolled_and_committed` fail with a closed
  callback (RecvError). The iterator-panic case in `append_spawned_cancellation…` failed too.
- C2: when the caller future holds the permit instead of the owner, U6 fails at `permits == 0`.
- Preflight: comparing the row by index only fails the refusal test at scenario 0.
- Shutdown order: joining owners before `try_shutdown` makes the isolated U6 fixture return `Orphaned` after the 5 s
  deadline.
- Privacy positive control: the same file scan run by `finish` found a public `[13;16]` value written through
  consensus in T2 (`seen=true`; T1 `false`, as expected). So the scan reads real store content, and the
  proof marker is the real `JoinCode::proof` derivation.
- Reopen-after-shutdown probe: started, shut down, and immediately reopened both stores 50 times (fixtures 13 and 17).
  There were 0 `Io` errors.

I did not run the hosted three-OS checks or measure full CI wall time. I re-ran no T3 work or broad workspace tests.
The builder's 8.788 s / T1 1.236 s / T2 7.943 s / U6 1.662 s figures are consistent with mine. They are the builder's
evidence, not mine.

## Non-blocking findings (fix in the T3 follow-up or before a production caller)

1. **The shutdown store-release postcondition is weaker than its name suggests (low/medium).** `Weak::upgrade`
   fails as soon as the strong count reaches 0. `Arc::drop` drops the inner `VoteLogStore`/`StateStore`, which
   closes the SQLite files and the lock, only *after* that. So `shutdown() == Ok` does not prove the files are closed
   if another thread dropped the last handle. My probe saw 0 of 50 failures. Fixture 17 still retries `Io`, and T2's
   `StateStore::open` straight after `stop` does not retry, which makes it a possible hosted flake (it cannot pass
   falsely). Before any in-process restart caller exists, add a release signal that fires after the store fields
   drop (for example a guard field declared after the store) and wait on that instead of `Weak`.
2. **ReadIndex apply wait is unbounded inside `linearizable_state` (low).** The 1500 ms `wait_timeout` bounds only the
   quorum acknowledgement. `ensure_linearizable` then calls `await_ready` with no timeout. Callers must keep an outer
   deadline (the tests do). Alternatively, use `get_read_linearizer` + `try_await_ready(Some(..))`.
3. **Missing retained negative controls (low).** There is no retained positive control for the marker scan; I ran one
   only temporarily. No forged-sender fixture proves the `mismatch` counter fires. Both are properties of the test
   double, not of production.
4. **Minor robustness (low).** If the owners lock is poisoned (unreachable today), `shutdown` returns `Orphaned` early,
   before `try_shutdown`. The `Weak` wait spins on `yield_now` for up to 5 s. The module-wide `#![allow(dead_code)]`
   and `pub permits` should be narrowed when the module is wired. U6's restored fixture asserts that all 64 owners
   resolve within 8 s, which is stronger than the brief's "recorded". It passes, but it depends on timing.
5. **T3 must cover what is still unexercised (risk).** No engine snapshot build, install or purge, no
   `max_in_snapshot_log_to_keep`/`purge_batch_size` sizing, and no `Full` liveness under a long follower outage has
   run through the engine (snapshots=0, and T1/T2 stay under 64 entries). T3 should also restart a follower around an
   install/purge boundary under the preflight. A legitimate refusal there must be handed back, not relaxed.

## Boundaries (unchanged)

The proof is real OpenRaft 0.10.0 over persistent local SQLite stores, run in-process with a test-only in-memory RPC
and graceful restarts only. Time is logical only.

Not claimed:

- physical 15-minute expiry
- key authentication
- crash during consensus, quorum disk loss or power loss
- separate processes or hosts
- TCP, mTLS, CA or signing
- pre-vote, learner or joint membership changes
- KICKOFF M3 acceptance

All are **unverified on real hardware**. M2 stays open: A7 is FINAL NO-GO, and A12's single attempt is consumed with
cleanup unconfirmed. No retry is authorized.

Retention: this file is the only change and the only commit. There are no source, PROGRESS or ADR edits, and no push,
PR, comment, status, merge or watch.
