# Brief: M3 OpenRaft 0.10.0 adapter and in-process three-node proof

Status: **proposed** (Opus 5.5 high, 2026-11-15) on `7431327` (main `38750cf`, PR #144). Sol reviews before code.
Nothing built or run. No M3 acceptance, transport, mTLS, CA, signing, power-loss or real-host claim. M2 stays open:
A7 FINAL NO-GO and the consumed, cleanup-unconfirmed A12 attempt get no retry. ADR 0005 stays unchanged; this brief
implements its invariants and adopts none of its open policy choices.

## Goal

Run the real OpenRaft engine over the merged `VoteLogStore` and `StateStore` stores. Prove an election, commit/apply,
2-of-3 continuation, restart convergence, ReadIndex reads, no-quorum refusal and enrollment outcomes from committed
entries. Use three persistent in-process nodes and a test-only in-memory network: no sockets, listeners or certificates.

## Verified source facts (crate `openraft-0.10.0`, sha256 `2628033834a6…7ad0`; deps fetched from static.crates.io)

- Features (`Cargo.toml`): default `tokio-rt`+`clap`; `serde` is optional. Without `serde`, `OptionalSerde` is an
  empty blanket trait (`base/mod.rs`), so `D`/`R`/`Node` need no serde. `openraft-rt-tokio 0.10.0` (sha `ad29…b1a4`)
  needs tokio `1.39` with `rt,sync,time,rt-multi-thread`, which the pinned `=1.48.0` satisfies. Already in our lock:
  `futures-util 0.3.34`, `rand 0.10.3`, `syn 3.0.10`, `chrono 0.4.45`, `thiserror 2.0.21`, `tracing 0.1.44`.
- `declare_raft_types!` (`raft/mod.rs`) defaults: `NodeId`/`Term` `u64`, `Node` `impls::BasicNode{pub addr: String}`,
  `LeaderId` `leader_id_adv::LeaderId{pub term, pub node_id}` (Committed = Self), `Vote{pub leader_id, pub committed}`,
  `Membership<u64,BasicNode>`, `Entry{pub log_id, pub payload}`, `EntryPayload::{Blank, Normal(D), Membership(M)}`, and
  `AsyncRuntime` `impls::TokioRuntime`. `AppData` needs `Debug + Display`; `AppDataResponse` needs only Send/Sync.
- `Membership::new(Vec<BTreeSet<NID>>, nodes)` checks voters ⊆ nodes; it has `get_joint_config()` and `nodes()`. Its fields
  are crate-private. `StoredMembership::new(Option<LogId>, M)`. `SnapshotMeta{last_log_id, last_membership}` has no
  snapshot id in 0.10. `Snapshot{meta, snapshot: SD}`.
- `RaftLogStorage` (`storage/v2/raft_log_storage.rs`) returns `io::Error`. It must serialize all write IO; `save_vote` is
  durable before return; `append(entries, IOFlushed)` must be readable on return, and the callback means durable (before or
  after return). `IOFlushed` (`storage/callback.rs`) has no completing `Drop`, so a dropped callback never completes the IO.
  `IOFlushed::signal(oneshot)` is public. `RaftLogReader` requires `try_get_log_entries(RangeBounds)` and `read_vote`.
- `RaftStateMachine` (`raft_state_machine.rs`): `type SnapshotData`/`SnapshotBuilder`; `applied_state`; `apply(Stream<Item =
  io::Result<(Entry, Option<ApplyResponder>)>>)` (`ApplyResponder::send(R)` must run after each apply, or `client_write`
  never returns); a required `get_snapshot_builder`; `install_snapshot(&meta, SD)`; `get_current_snapshot`.
- Startup (`storage/helper.rs`): `committed = max(read_committed, applied)`. It installs `get_current_snapshot` only if
  newer than applied. It errors if `applied < purged` ("state machine is behind the purged logs") or if the log is inverted.
  If applied is ahead of the log at a greater index, it calls `purge(applied)`; an equal or lower index is an error. It
  rebuilds the snapshot if it is below purged. `Raft::build` maps the error to `Fatal::StorageError` and does not validate `Config`.
- Install (`engine/handler/following_handler`) is ignored unless snapshot > `local_committed`. Then it truncates
  conflicts, installs the SM, then purges. `Command::PurgeLog` waits for `Condition::Snapshot{upto}`.
- `Raft::new(id, Arc<Config>, N, LS, SM)` needs `N::Network: NetSnapshot<SnapshotData = SM::SnapshotData>`. `RaftNetworkV2`
  (`network/v2/network.rs`) blanket-implements all `Net*`. It needs `append_entries`, `vote` and `full_snapshot`; the
  default `pre_vote` grants. `Config::enable_pre_vote` `None` resolves to false. `RPCError::Unreachable(Unreachable::new(&e))`.
- Reads: `ensure_linearizable(LinearizerOption)` waits for leadership and for apply ≥ `read_log_id`. With
  `LinearizerOption::new(Some(ZERO), true).with_wait_timeout(d)` a missing quorum ends in
  `LinearizableReadError::{QuorumNotEnough, ForwardToLeader}`. `client_write` has no timeout. `try_shutdown` joins only
  the core and tick tasks ("independently of ... other background task lifetimes").
- `testing::log::Suite` runs 46 `run_test` cases plus `transfer_snapshot`. Each case is a `pub async fn(LS, SM)`, so cases
  can be called one by one. 19 cases and `transfer_snapshot` start SM apply at index > 0 or leave a gap.

## Decisions

1. **Pins.** In `crates/arxd/Cargo.toml`: `openraft = { version = "=0.10.0", default-features = false, features = ["tokio-rt"] }`,
   `futures-util = "=0.3.34"`, and tokio gains `"sync"`. No `serde`, `clap`, `bt`, `single-threaded` or `type-alias`. Cargo.lock may only
   **add** packages; existing entries stay byte-identical. Record `cargo tree -e features -i openraft` in the evidence file.
2. **Types.** `declare_raft_types!(pub Arx: D = enrollment::Command, R = Applied)`, with all other defaults. `Applied(pub
   Option<Outcome>)`. `Command` gains a static `Display` ("enrollment mint"/"enrollment redeem"). No tracing subscriber
   is added; if one is added later, Debug/Display exposure must be reviewed again.
3. **Mapping (exact, total).** `LogPos{term,leader,index}` ↔ `LogId{LeaderId{term,node_id:leader},index}`.
   `StoredVote{term,node,committed}` ↔ `Vote{LeaderId{term,node_id:node},committed}`; the derived `Ord`s agree (tested).
   `Blank`↔`Blank`, `Normal(c)`↔`Enrollment(c)`, and `Membership(m)`↔`Membership(encode(m))`.
4. **Membership codec** (`MembershipBlob`, ≤ 4096 B). Layout: `u8` version 1, then `u8` config count 1..=2. Each config:
   `u8` 1..=16 voters as strictly ascending nonzero `u64`. Then `u8` 1..=16 nodes: ascending nonzero id, `u8` addr length
   1..=200, then addr bytes from `[A-Za-z0-9._:\[\]-]`. The worst case is 3605 B. Decode reads with `Reader`, builds
   `Membership::new`, and requires a canonical re-encode. Every error is the new `StoreError::Membership` ("membership out of
   bounds"); encode never truncates. Any `initialize`/`change_membership` input is pre-encoded, including the joint form,
   so a leader cannot create an unencodable entry.
5. **Adapters share stores.** Use `Arc<std::sync::Mutex<VoteLogStore>>` (cloned into every `LogReader`) and
   `Arc<Mutex<StateStore>>`. Every lock and store call runs inside `tokio::task::spawn_blocking`; async code never locks
   and never holds a guard across `.await`. A `JoinError` (panic, or cancelled before start) maps to `StoreError::Poisoned`.
   A mutex poisoned by a panic fails closed with `Poisoned` (no `into_inner`) until the process restarts and reopens.
   Errors are `io::Error::other(StoreError)` with static text. OpenRaft then goes fatal and the outcome is unknown (N5).
6. **Append callback exactly once.** `append` collects `entries.into_iter().take(65)`; more than 64 is `TooLarge`, never
   truncated. It wraps the callback in `Completion(Option<IOFlushed>)` and moves that into the blocking closure. After
   `VoteLogStore::append` returns (`Ok` only after COMMIT), it calls `finish(result)`. `Drop` completes with
   `Err(Poisoned)` if the callback is still unset: panic, cancelled-before-start, or a dropped closure. A conversion error
   completes `Err` before any spawn. `append` then returns the same result. The callback fires even if OpenRaft drops the append future.
7. **State-machine semantics stay as they are.** `apply` takes items in order. On an item `Err` it returns that error. Each
   entry is one blocking `StateStore::apply` (durable before return). `responder.send(Applied(outcome))` happens only after
   `Ok`; on error the responder drops and the call returns `Err`. The consecutive-from-0 rule and term rule stay; a real
   cluster applies index 0 (the `initialize` membership) and installs snapshots for everything else. `StateStore::apply` is called only from
   `SmAdapter::apply` and tests. Grants reach callers only through `ClientWriteResponse` (committed+applied on the leader).
8. **Snapshots are the durable SM image.** `get_current_snapshot`/`build_snapshot` read `(applied, membership, blob)` under one
   lock: meta `last_log_id = applied`, `last_membership` decoded. They return `None` when nothing is applied (forced builder:
   meta `None` with the blob). Purge waits for a snapshot ≥ `upto`, and every snapshot is durable state ≥ applied, so
   `purged ≤ durable applied` holds across stores (I4). `install_snapshot` rejects data over 16384 B. A new pure
   `StateStore::peek(blob)` must match meta exactly (applied and canonical membership) before `install`; otherwise
   `Corrupt` with nothing changed. `try_create_snapshot_builder` keeps its default.
9. **Startup** is OpenRaft's helper unchanged. It repairs SM-ahead (purge past the tail is supported) and fails closed when the
   SM is behind purged or on an equal-index conflict. No `save_committed`: apply is durable, so restart never reverts.
   Reads go through the leader's ReadIndex only.
10. **Config and backpressure** (`Params`, validated, `check()` asserted in tests). Production: heartbeat 100 ms, election
    500..1000 ms, `max_payload_entries=64`, `max_append_entries=Some(64)`, `api_batch_capacity=Some(64)`,
    `api_channel_size=Some(1024)`, `snapshot_policy=LogsSinceLast(64)`, `max_in_snapshot_log_to_keep=32`, `purge_batch_size=32`,
    `replication_lag_threshold=128`, and `enable_pre_vote=Some(false)` (no pre-vote RPC; ADR 0005). Proposals hold one permit
    of a 64-permit `Semaphore` until `client_write` returns; no permit means retryable `Busy`. `check()` requires
    `snapshot_logs + keep + purge_batch + 64 + 8 ≤ MAX_LIVE (512)` and `lag > snapshot_logs`. This is sizing, not proof:
    `Full` stays a fail-closed liveness error. The snapshot test params (8/0/1/16) pass the same `check()`.
11. **Suite honesty.** Run the 27 compatible official cases by name, unmodified. Run the 19 sparse-SM cases plus `transfer_snapshot`
    and require each to return `Err` displaying "out of order". Run each in `tokio::spawn` so a panic or `Ok` fails and forces
    reclassification. The evidence names both lists. This is never reported as full Suite conformance; Arxcovo startup tests cover the gaps.
    S2 = `last_membership_in_log`, `get_membership_from_{empty_log_and_sm, log_le_sm_last_applied, log_gt_sm_last_applied_1, _2}`, `get_initial_state_{with_state, membership_from_empty_log_and_sm, membership_from_sm_inlog_is_smaller, membership_from_log_insm_is_smaller, last_log_gt_sm, last_log_lt_sm, last_log_lt_sm_past_snapshot, sm_lt_purged, inverted_log_order, conflict_at_same_index, log_ids, re_apply_committed}`, `last_id_in_log`, `last_applied_state`, `transfer_snapshot`. S1 = every other `test_store` case.
12. **Command service** (`pub(super)`, no ingress). `propose(Command)` gives `(LogPos, Option<Outcome>)` or `ProposeError::{Busy,
    NotLeader(Option<u64>), Unavailable}`; it never retries. `linearizable_state()` gives `(Option<LogPos>, Vec<u8>)` or
    `ReadError::{NotLeader, NoQuorum, Unavailable}`. It uses the option from the facts with `wait_timeout` 1500 ms, and checks the
    returned applied value is ≥ `read_log_id`. Fresh-mint-only ingress (A2) and fresh signing guards stay future boundaries.

## Files (only these, plus Cargo.toml/Cargo.lock as in decision 1)

- `cluster.rs`: `pub mod consensus;`. `enrollment.rs`: `Display for Command`. `state_store.rs`: `StoreError::Membership`, `pub(super) fn peek`.
- `cluster/consensus.rs` (≤ 350 lines): type config, `Applied`, `Params`/config, mapping, membership codec, and `ConsensusNode::{start,
  propose, linearizable_state, shutdown}`. `shutdown` runs `try_shutdown`, drops the `Raft`, then polls ≤ 5 s until `Weak`s of
  both stores are gone; otherwise `Orphaned`.
- `cluster/consensus/adapter.rs` (≤ 320 lines): `LogAdapter` (also the reader), `SmAdapter`, `SnapBuilder`, `Completion`, and the `blocking` helper.
- `#[cfg(test)]` `cluster/consensus/{mem_net.rs, tests.rs, three_node.rs}`, plus `docs/evidence/m3-openraft-three-node.md`.
- Commits: (1) deps, types, mapping, codec and unit tests; (2) adapters, Suite and startup; (3) service, mem_net and T1/T2; (4) T3 and evidence.
  If the diff exceeds ~1800 lines, (4) becomes an immediate follow-up PR. T1/T2 are the first usable three-node proof.

## In-memory network (test double, not a transport claim)

`MemRouter{nodes: Mutex<BTreeMap<u64, Raft>>, isolated: Mutex<BTreeSet<u64>>, counters}`. `MemNetwork{source, target}`
calls the target's `append_entries`/`vote`/`install_full_snapshot`. If either side is absent or isolated, or the remote returns
`Fatal`, it returns `Unreachable`. It checks the claimed sender (`rpc.vote.leader_id.node_id`) equals `source`; on a mismatch it
returns Unreachable and increments a counter, and the test requires 0. It records max entries per append (must be ≤ 64) and
snapshot bytes (must be ≤ 16384). Nodes are removed from the map before shutdown, which breaks the Raft↔router reference cycle.

## Tests (`cluster::consensus::*`; `private_temp`; each three-node test runs ≤ 45 s inside `timeout` with owned shutdown)

- U1 mapping round trips, plus a vote-order table (lower term, same term lower node, committed flag) matching `StoredVote` order.
- U2 codec: the Suite default node; the maximal joint 2×16 voters with 16×200-B addrs round-trips canonically. Rejected: id 0, unsorted or
  duplicate ids, a voter not in nodes, 17 nodes, addr 0/201/bad byte, 3 configs, version 2, a trailing byte, truncation at every length. No panics.
- U3 `Params::check`/`Config::validate` for both profiles; mutated params are rejected.
- U4 completion via `IOFlushed::signal`: exactly one value on success (after COMMIT, the row is visible on reopen), on store error,
  on a 65-entry batch (disk unchanged), after a panic in a blocking op (then every later call returns `Poisoned`), and for a dropped unrun `Completion`.
- U5 SM sparse stream (first index 1) gives `Err` "out of order" with the store unchanged. Install with mismatched meta is `Corrupt`, unchanged.
- S1/S2 Suite as in decision 11. R1–R4 startup through the real `StorageHelper::get_initial_state`: SM 0..=5 over log 0..=3 → purged = last = 5,
  next append 6 (4 rejected); SM 0..=1 under log purge(3) → error, and `Raft::new` → `Fatal::StorageError`; equal-index term conflict → error; normal.
- Fixture: ids 1..3, node 1 `initialize({1,2,3} → BasicNode::new("mem:N"))`, `first_free = 4`, deterministic code RNG, explicit
  logical times. Writes go to the discovered leader, re-routing on `NotLeader` within the deadline. Every ack is recorded.
- T1 follower: Mint A, Redeem A(K1, R1) → `Enrolled{4}`; an identical retry gives replay 4. Isolate and shut down a follower. Then 2/3 writes:
  reuse A(R2) → `Consumed`; Mint B with concurrent Redeem B(R3, K2) and (R4, K3) → exactly one `Enrolled{5}`, the other `Consumed`;
  Redeem C at mint time + `CODE_TTL_MS` → `Expired`; a wrong proof → `BadProof`. Restart the follower: applied equals the leader's, and the blobs are equal on all three.
- T2 leader: acked writes; isolate and shut down the leader; a new leader appears within 5 s; writes continue. A linearizable read sees every ack;
  a follower read gets `NotLeader`. Reopen the old leader's SM directly: applied ≥ each write it acked. Restart it and it converges. Shut down
  two nodes: a lone-node propose gets no `Ok` within 2 s, and a read gets `NoQuorum`/`NotLeader` within 3 s. Restart one node: an identical retry
  of the unacked command resolves deterministically, with the node counter advanced at most once. All converge. Marker scans run on all directories.
- T3 snapshot: small params. Isolate and shut down a follower; commit 40 Redeem-unknown entries until the leader has purged beyond the follower;
  restart. Expect ≥ 1 `full_snapshot` to it, applied converged with blobs equal, then consecutive applies afterwards.
- No test sleeps blindly. Use the `Raft::wait(Some(..))` conditions and `tokio::time::timeout`, with real time, multi-thread runtime and 4 workers.

## Acceptance

Hosted Windows/macOS/Ubuntu: `cargo fmt --check`, `cargo clippy -p arxd --all-targets --locked -- -D warnings`, and
`cargo nextest run -p arxd --locked cluster::`. Target: all cluster tests < 60 s per OS, T1–T3 each ≤ 20 s typical, full CI < 15 min;
report measured times. Existing 36 cluster tests and eight crash probes stay unchanged and green. The evidence records the lock diff, the
`cargo tree` output, the Suite partition, counters and timings.

**Claim:** the actual OpenRaft 0.10.0 engine over the durable SQLite stores, in-process, gives election, commit/apply, 2/3 continuation
after the follower or the leader goes offline, restart convergence of acknowledged state, ReadIndex reads with bounded no-quorum refusal,
and enrollment outcomes from committed entries.

**Not claimed:** separate processes or hosts, TCP/mTLS/CA/certificate binding, pre-vote, learner, promotion and joint changes, crashes
during consensus (only graceful restarts; store kill probes exist separately), power loss, physical 15-minute expiry (logical times only),
key authentication, signing, MCP ingress, KICKOFF M3 acceptance. All are **unverified on real hardware**.

## Sol handoff

Sol high implements in commit order. Do not edit `log_store.rs`, or the codecs and semantics of the merged stores. If a compatible Suite case
fails, show which documented OpenRaft contract the store breaks before changing anything. Otherwise move the case to S2 with a reason and
flag it for Opus review. Never weaken the SM order to pass a case. Three failed fixes trigger a handoff with evidence. No subagents beyond
the routing rules, no native or M2 actions, no admin steps. A different-provider review and green gates are required before merge.
