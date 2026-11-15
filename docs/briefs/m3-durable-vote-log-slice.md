# Brief: M3 durable vote/log store (storage/crash probe)

Status: **proposed** (Opus 5.5 high, 2026-11-15) on `7883d27` (main `cd28bb9`, PR #143); Sol reviews
before code. Nothing built or tested. No M3 acceptance, power-loss, quorum or three-node claim. M2
stays incomplete; A7 FINAL NO-GO stays closed, and so does the consumed, unconfirmed, not-retried A12 cleanup.

## Why this slice

Vote/log half of ADR 0005 probe 3 beside the merged SM store: votes durable before return, append ack only after
sync, truncate/purge crash/reopen boundaries; a sync API a later OpenRaft 0.10 adapter wraps. No OpenRaft dep, transport or CA.

## Verified facts (OpenRaft `0.10.0` crate, sha256 `2628033834a6…7ad0`, read, not pinned)

- `RaftLogStorage` (`storage/v2/raft_log_storage.rs`): `get_log_state`, `get_log_reader`,
  `save_vote` ("persisted on disk before returning"), `append(entries, IOFlushed)` (readable on
  return; callback only once on disk), `truncate_after(Option<LogId>)` (exclusive),
  `purge(LogId)` (inclusive), optional `save_committed`/`read_committed` (default no-op/`None`). All write IO, vote
  included, is serialized. `LogState{last_purged_log_id, last_log_id}` (empty: last = purged). `IOFlushed::io_completed`.
- `RaftLogReader` (`raft_log_reader.rs`): `try_get_log_entries(RangeBounds<u64>)` returns every contained entry
  atomically. A gap is allowed only at the start (purged) or the end (not yet appended); holes are errors.
  It also has `read_vote`, and `limited_get_log_entries(start, end)` (non-empty prefix allowed).
- Defaults (`raft/mod.rs` `declare_raft_types`): `NodeId`/`Term` `u64`; `LeaderId` is
  `leader_id_adv::LeaderId{term, node_id}` (derived `Ord`); `LogId{leader_id, index}` derived `Ord`;
  `Vote{leader_id, committed}`. The `RefVote` order is leader id, then `committed` (false < true). `EntryPayload::{Blank, Normal(D), Membership}`.
- `update_vote` emits `SaveVote` only for a strictly greater vote. `initialize` appends index-0 membership
  `(term 0, self, 0)` **without** saving a vote, so a log entry may exceed the stored vote.
- Startup (`storage/helper.rs`): `committed = max(read_committed, last_applied)`. It errors if
  `last_log < last_purged`, or if `last_applied < last_purged` ("state machine is behind the purged logs").
  If applied is ahead of the log tail at a greater index, it calls `purge(last_applied)` past the end; at an equal or lower index it errors.
- `Command::PurgeLog{upto}` waits for `Condition::Snapshot{upto}` (`engine/command.rs`). On snapshot
  install, conflicting truncation runs before the SM install, which runs before the purge (`following_handler`).
- `testing::log::Suite` calls: a first append at index 1 or 2 on an empty log; `truncate_after((1,11))` with
  tail `(1,10)` as a no-op; `truncate_after(None)`; and `purge((1,20))` past the end, giving `last == purged`.
- Config (`config/config.rs`): `max_payload_entries`, `max_append_entries` (merges only), `api_batch_capacity`,
  `snapshot_policy: LogsSinceLast(n)`, `max_in_snapshot_log_to_keep`, `purge_batch_size`, `replication_lag_threshold`.

## Decisions

1. **Backend: the existing pinned SQLite**, in a separate file and lock in the same private cluster directory. It uses the
   pragma/readback/sidecar policy already passing hosted crash probes. No new deps.
2. **Atomicity model: two independent stores, consistent by ordering, with no cross-file transaction.**
   I1: SM apply is durable before it returns (merged). I2: appends are durable before return and callback.
   I3: the vote is durable before return. I4: `purged ≤ sm.applied` at every durable instant, because
   OpenRaft purges only after a snapshot ≥ `upto` and our snapshot is the durable applied blob. I5:
   `sm.applied` may exceed the log tail (install, then crash before purge); the helper purges past the end. I6: truncation
   never goes below purged (enforced) or below committed (an OpenRaft duty; not stored here).
   Startup checks catch I4/I5 breaks, so they fail closed.
3. **No `save_committed`.** The SM persists in `apply`, so restart never reverts below an observed
   state, and the documented re-apply is unnecessary. Reads still use OpenRaft's linearizable protocol.
4. **Suite-compatible fail-closed semantics.** The store accepts every log-store call pattern above and
   rejects holes, overlaps, regressions and ID mismatches without changing memory or disk.
5. **Bounds:** `MAX_APPEND = 64` entries per call, `MAX_LIVE = 512` unpurged entries, and an
   entry body of at most 4163 B. With 4096-byte pages, `max_page_count=2048` fits the maximal case (≤ 2 pages per row). `Full` writes
   nothing. The store never prunes on its own; only `purge`, driven by a snapshot, removes rows below the tail.
6. Retain the advisories: fresh-mint ingress (replicating a decoded `Mint` is not re-proposal) and SQL unknown outcome (N5).

## Files (exact; no `Cargo.toml`, lock, workflow or PROGRESS edits)

`cluster.rs` gains `pub mod log_store;`. In `state_store.rs`: `LogPos` derives `PartialOrd, Ord` (field
order equals `LogId` order); `put_pos`/`get_pos` become `pub(super)`; extract the dir/sidecar/lock/pragma code
into `pub(super) fn open_db(dir, stem, max_pages) -> Result<(File, Connection), StoreError>`
(`StateStore` keeps `cluster-state`, 64, byte-identical behavior); add `StoreError::{Full, Conflict, VoteRegressed}`
with static `Display`; `#[cfg(test)] pub(super) mod tests;` and `pub(super)` on `code`, `mint`, `redeem`,
`absent`, `privacy_markers`, `pause` and `KillOnDrop`. `enrollment/codec.rs` gains `encode_command`/`decode_command`.
New: `log_store.rs`, `log_store/tests.rs`, `docs/evidence/m3-durable-vote-log-store.md`. Commits: refactor (existing
23 cluster test assertions unchanged, green), then the store.

## Store contract (`log_store.rs`; sync, `&mut self`, no threads, clock, RNG or logging)

- Types: `StoredVote{term, node: u64, committed: bool}` derives `Ord` in field order (= `RefVote` order); `LogState{purged,
  last: Option<LogPos>}`; entries `(LogPos, state_store::Entry)`. API: `VoteLogStore::open(dir, cluster)`, `vote()`, `log_state()`, `live()`, `save_vote(StoredVote)`,
  `append(&[(LogPos, Entry)])`, `entries(start, end_excl, limit)`, `truncate_after(Option<LogPos>)`, `purge(LogPos)`.
- **Open:** `open_db(dir, "cluster-log", 2048)` validates `cluster-log.{lock,sqlite3}` and its `-journal/-wal/-shm`
  sidecars, holds the lock, and reads back TRUNCATE, `synchronous=2`, `fullfsync=1` and `page_size<=4096`.
  `user_version` 0 creates `meta(singleton CHECK=1, blob CHECK(length<=256))`, `log(idx INTEGER PRIMARY KEY
  CHECK(idx>=0), body BLOB NOT NULL CHECK(length(body)<=4163))` and the initial meta, in one transaction. Version 1 decodes meta and scans every row; any other version is `Unsupported`. A foreign cluster is `WrongCluster`.
- **Scan:** rows exactly `first..=last` from the meta tail, count ≤ `MAX_LIVE`, each body decodes with
  `idx == pos.index`, positions strictly increasing in `LogPos` order, and the last row equals the tail. If purged:
  `first == purged.index+1` and every row is `> purged`. Any failure is `Corrupt`.
- **Vote:** a lower vote is `VoteRegressed`, unchanged; an equal one is `Ok` without a write; a higher one is written to meta and committed,
  then returned. There is no vote ≥ entry check (`initialize` relies on that).
- **Append:** an empty batch is a no-op `Ok`. Over 64 entries, an index above `i64::MAX` or an oversized body is `TooLarge`. The expected first index is
  `tail.last+1`, else `purged+1`, else any index (Suite and `initialize`). Indexes must be consecutive and each `LogPos` greater
  than the previous one and than the tail or purged position; otherwise `OutOfOrder`. Overlap is never an implicit overwrite. Going over `MAX_LIVE` is `Full`.
  Rows and the meta tail go in one transaction; COMMIT (FULL sync) happens before memory changes and before return.
- **Truncate (exclusive):** `None` requires purged `None`, else `OutOfOrder`, and deletes all rows. `Some(a)`: below
  purged is `OutOfOrder`; equal to purged requires `a == purged` and deletes all; a row at `a.index` must equal `a`, else `Conflict`;
  past the tail it is a no-op; below the first row (sparse start) is `Conflict` unless `a.index+1 == first` (then delete all).
- **Purge (inclusive):** an index at or below purged is a no-op if equal, else `OutOfOrder`. In the log, the row must equal the id (else `Conflict`); delete rows up to it.
  Past the tail it requires `id > tail.last` (else purged), deletes all, and sets `last = purged = id`. Below the first row it is allowed only when
  `id.index+1 == first` and `id < first row`; otherwise `Conflict`. Meta and deletes go in one transaction.
- **Read:** one SELECT of `[max(start,first), min(end,last+1))` capped at `limit` (1..=512). It verifies each row and
  contiguity; any mismatch is `Corrupt`, which poisons the store.
- **Poison:** any SQLite or post-open decode error poisons the store, and every later call is `Poisoned`. The commit outcome is unknown, so
  memory stays at the pre-state until reopen reloads disk. All errors use static `Display` only.

## Canonical encodings (big-endian; SHA-256 trailers detect corruption, they do not authenticate)

Row: `"ARXLE1\0\0"`, `LogPos`, tag, payload, SHA-256. Tag 0 Blank. Tag 1 Mint: `cluster actor request code
verifier32 proposed u64`. Tag 2 Redeem: `cluster request code presented32 key32 proposed u64`. Tag 3 Membership:
`u16` length 1..=4096, then opaque bytes. Every encodable `u64` time is accepted, because the core rejects deterministically on apply; decode must never strand
a committed entry. The command codec holds only verifiers, never a proof. Meta: `"ARXLM1\0\0"`, `u16` version 1, `cluster16`,
vote flag + `term node` + committed flag, purged flag + `LogPos`, tail flag + `first u64` + last `LogPos`, SHA-256.
Decode rejects: header/version/digest errors; a flag not 0/1; unknown tags; truncated or trailing bytes; membership length 0
or over 4096; `first > last.index`; an index above `i64::MAX`; or a purged/tail mismatch. Canonical: `encode(decode(b)) == b`.

## Future adapter contract (next slice; recorded so this API is not throwaway)

`LogPos{term, leader, index}` ↔ `LogId{LeaderId{term, node_id}, index}`; `StoredVote` ↔ `Vote`;
`Entry::{Blank, Enrollment, Membership(blob)}` ↔ `EntryPayload::{Blank, Normal(Command), Membership}`.
The membership codec, at most 4096 bytes, is validated before `change_membership`. Wrap the store in `Arc<std::sync::Mutex<_>>`,
shared by the storage and every reader; each call runs one operation under `spawn_blocking`. `append` calls
`callback.io_completed` exactly once after the store returns (`Ok` only after COMMIT), then returns the same result.
Map errors to `io::Error::other(StoreError)`; OpenRaft then stops, the daemon reopens, and no client gets a final failure (N5).
Config: `max_payload_entries=64`, `max_append_entries=Some(64)`, `api_batch_capacity=Some(64)`,
`snapshot_policy=LogsSinceLast(64)`, `max_in_snapshot_log_to_keep=64`, `purge_batch_size=32`,
`replication_lag_threshold=128`. Refuse new proposals as retryable busy when `last.index − applied.index ≥ 256`.
`Full` is still possible (stale tails, lag). It is fatal for liveness, not safety, and never prunes unapplied entries. The SM store rejects
sparse applies, so the adapter slice chooses how to run the official Suite. These semantics are verified by reading the source, not by execution.

## Required tests (`cluster::log_store::tests`; `private_temp`; fill caps in 64-entry batches)

1. Codec: every variant round-trips canonically (membership of 1 and 4096 bytes); truncation at every length, a trailing byte,
   tags 4/0xff, membership 0/4097 and a flipped digest are all `Corrupt` without panic. The same holds for meta.
2. Append: sparse first index 1; then gap, overlap, duplicate, non-consecutive batch, or lower `LogPos` → `OutOfOrder`; 65 →
   `TooLarge`; `i64::MAX+1` → `TooLarge`. 512 max-size membership rows fit within 2048 pages, and the 513th is `Full`. All are unchanged on disk after reopen.
3. Reads: ranges `3..3`, `5..7`, `0..`, below purged, past the end, reversed, and `limit` 1/64 match the Suite expectations.
4. Truncate/purge: the Suite cases (`(1,11)` no-op, `None`, `purge (0,0)`, `(1,20)` past the end then append `(2,11)` →
   index 11). Mismatched ids give `Conflict`; going below purged gives `OutOfOrder`. A conflicting tail can be truncated and replaced.
5. Votes: start at `None`; lower term, same-term lower node, or committed→uncommitted → `VoteRegressed`; equal is a no-op;
   a vote survives reopen; index 0 appends with no vote.
6. Open fails closed: a flipped meta or row byte, a deleted middle/first/last row, an extra row, `user_version` 2, a foreign
   cluster, a held lock, or a directory or symlink sidecar.
7. Fault: a `RAISE(FAIL)` trigger on `log` insert, then separately on `meta` update, gives `Sql`; the store is poisoned; reads and writes then return `Poisoned`; reopen gives the pre-state.
8. Two stores in one dir: append 0..=5 (membership, mint, redeem), apply 0..=3 to the SM, purge `≤` applied, then reopen
   both. Check I4/I5, that 4..=5 re-apply to reference outcomes, and that an SM install past the log end, then `purge(id)`, makes the next append `id+1`.
9. Markers: no raw file and no store/error/entry `Debug` contains secret, proof or code text, raw or hex.
10. Process kill (four spawns of `current_exe`, 10 s deadline each, `KillOnDrop`; the child test is ignored;
    `ARX_LOG_CRASH_{DIR,MODE}`). The child builds entries 0..=3 with a vote, then a `#[cfg(test)]` hook pauses:
    `append-before` (after `cache_flush`; assert the hot-journal magic and page count > 0): reopen tail 3, and the same batch re-appends to the reference result.
    `append-after` (after COMMIT, before return; journal absent or length 0): rows 4..=5 equal the reference, re-append → `OutOfOrder`.
    `truncate-before` (hot journal asserted): 0..=3 intact with no hole, then truncate and replace succeed.
    `vote-after`: the new vote is reopened, and a lower vote gives `VoteRegressed`. All runs end with a clean marker scan.

## Acceptance

Hosted Windows/macOS/Ubuntu green: `cargo fmt --check`, `cargo clippy -p arxd --all-targets --locked -- -D warnings`,
`cargo nextest run -p arxd --locked cluster::` (cluster < 60 s per OS; full CI < 15 min). Only listed paths; no dependency,
`Serialize` or credential accessor. Claim: hosted process-kill consistency only. Power loss, quorum, OpenRaft integration and
three-node availability stay open, **unverified on real hardware**. Next: OpenRaft 0.10 adapter (pinned after review), in-process three-node test.
