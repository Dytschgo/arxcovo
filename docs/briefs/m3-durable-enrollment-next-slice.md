# Brief: M3 durable enrollment state store (storage/crash probe)

Status: **proposed** (Opus 5.5 high, 2026-11-15) on `35ec694` (main `4f323c3`, PR #142); Sol reviews
before code. Nothing built or tested; no M3 acceptance, commit, three-node or M2 claim. Steering
permits M3 with M2 gaps open; A7 FINAL NO-GO and unconfirmed, not-retried A12 D7l cleanup stay closed.

## Why this slice

Before OpenRaft wiring, prove on hosted Windows/macOS/Linux that one local store atomically persists
and, after a process kill, recovers the whole `EnrollmentState` (consumed codes, dedup records, node
counter, `committed_ms`) with applied position and membership: ADR 0005 probe 3's state-machine half
and the backend probe. Local apply or persistence never proves quorum commit.

## Verified facts (primary sources, exact versions)

- OpenRaft `0.10.0` crate source (2026-11-10, sha256 `2628033834a6…7ad0`), read, **not pinned**
  (five days old, unneeded here): `apply` stores each log id as last-applied plus membership and may
  persist before returning (`storage/v2/raft_state_machine.rs`); startup installs a snapshot only if
  `last_log_id > last_applied` (`storage/helper.rs`); followers skip snapshots ≤ committed
  (`following_handler`); `initialize` writes index 0, term 0; default `LogId` is
  `{leader_id: {term, node_id}, index}` (`leader_id_adv.rs`).
- Pinned `rusqlite =0.37.0` bundles SQLite 3.50.2 (`libsqlite3-sys 0.35.0`): a `TRUNCATE`-mode commit
  syncs the truncated journal when `synchronous>=FULL`; `fullfsync=ON` → `F_FULLFSYNC` (macOS);
  Windows uses `FlushFileBuffers`. sqlite.org/pragma: `EXTRA` differs from `FULL` only in DELETE mode.

## Decisions

1. **Backend: SQLite, provisional on the pass criteria.** Pinned, bundled, no dependency/CI cost,
   existing Windows hot-journal evidence; separate file from activity. A probe failing on any hosted
   OS after three genuinely different fixes → hand off with evidence; another backend needs an ADR.
2. **Persistent state machine:** one SQLite transaction per entry rewrites one canonical blob
   (applied position, membership, full state), which is also the current snapshot.
3. **Credential boundary (ADR 0005 narrowly updated).** `RedeemProof` is a bearer command
   credential. `Command::Redeem.proof` becomes `presented: CodeVerifier`, built only by
   `Command::redeem(cluster, request, code, &RedeemProof, key, proposed_at_ms)` on the receiving
   node, which then drops the proof. Entries, forwards, state, snapshots, DB files, errors, `Debug`
   and evidence carry only verifiers. `apply` keeps the `subtle` comparison as a consistency check
   under Raft's trusted-member authority, not authentication against members (whoever can commit
   entries already has full cluster authority). A copied state or log never holds a code
   redeemable through the unauthenticated enrollment path.
4. **Fresh-mint-only (review A2).** A `Mint` comes only from an in-memory `JoinCode::generate` on
   the proposing host, retried only from that value while `now < first_proposal + CODE_TTL_MS -
   4*SKEW_MS`, then discarded for a new code. Never re-propose a `Mint` decoded from log, snapshot
   or wire; never accept a client or peer verifier. Applying committed entries in index order is
   not re-proposal; the store guard applies each index once.
5. **Exactly-once recovery ≠ retry dedup.** Recovery: each committed index changes durable state at
   most once, in order, and the outcome returns only after commit. Retry dedup exists only via
   retained records, up to `TOMBSTONE_MS` after code expiry; a pruned code's redeem is
   `UnknownCode`. No dedup is claimed for unknown or pruned requests.
6. **Out of scope:** OpenRaft, log/vote storage, transport, mTLS, CA key, signing, encryption at
   rest, MCP, endpoints, physical-expiry guards. A3 (one key, two codes) is not load-bearing here
   (the store keeps core semantics); the issuance slice decides it.

## Files (exact; no `Cargo.toml`, lock, workflow or PROGRESS edits)

`cluster.rs` gains `pub mod state_store;`. Under `crates/arxd/src/cluster/`: `enrollment.rs` (the
`Redeem` change, `Command::redeem`, `mod codec;`); `enrollment/code.rs` (only `pub(super)
CodeVerifier::from_bytes`); new `enrollment/codec.rs` (state section); `enrollment/tests.rs`
(mechanical updates plus A1: wrong proof on an expired unconsumed code is `BadProof`); new
`state_store.rs` and `state_store/tests.rs`. Evidence: `docs/evidence/m3-durable-state-store.md`.
Existing deps only (`rusqlite`, `sha2`, `hex`; dev `tempfile`). Commits: boundary + A1, then store.

## Store contract

- Types: `LogPos { term: u64, leader: u64, index: u64 }` mirrors OpenRaft's default `LogId` but is
  not an OpenRaft type. `Entry::{Blank, Enrollment(Command), Membership(MembershipBlob)}`; a blob is
  1–4096 opaque, non-credential bytes.
- API (`&mut self`, no threads): `StateStore::open(dir, Init{cluster, first_free})`, where `Init`
  is used only at creation and a stored cluster differing from it is `WrongCluster`; `applied()`,
  `membership()`, `state()`; `apply(pos, &Entry) -> Result<Option<Outcome>, StoreError>`;
  `snapshot() -> Vec<u8>` (equals the stored blob); `install(&[u8])`.
- **Open:** validate the dir and existing paths (including `-journal/-wal/-shm`) with
  `config::checked_path` and `security::private`; hold `cluster-state.lock` via `try_lock` (Unix
  `0600`); open `cluster-state.sqlite3` with 250 ms busy timeout; set and read back
  `journal_mode=TRUNCATE` (`truncate`) and `synchronous=FULL` (2); `fullfsync=ON`;
  `page_size<=4096`, `max_page_count=64`. `user_version` 0 creates `sm(singleton INTEGER PRIMARY
  KEY CHECK(singleton=1), snapshot BLOB NOT NULL CHECK(length(snapshot)<=16384))` plus the initial
  blob in one transaction; 1 decodes; else `Unsupported`.
- **Apply:** poisoned → `Poisoned`. With `applied` `None` only index 0 is accepted; otherwise index
  must be `applied.index+1` and term not below `applied.term`, else `OutOfOrder` with memory and
  disk unchanged. Apply to a clone; a rejected command is still a committed entry (advances the
  position, stores the core's deterministic result); `Membership` stores blob plus position. Write
  and commit, then swap memory and return. Any SQLite error poisons; memory stays at the pre-state
  until reopen reloads disk.
- **Install:** over 16 KiB → `TooLarge`; failed full decode → `Corrupt`; other cluster →
  `WrongCluster`; applied `None` or not strictly above current → `StaleSnapshot`; all unchanged.
  Otherwise one transaction, then swap.
- `StoreError` `Display` is static: no IDs, paths, SQLite text or bytes. No logging.

## Canonical blob (big-endian; `codec.rs` owns the state section)

`"ARXSM1\0\0"`, `u16` version 1; applied flag + `LogPos`; membership flag + `LogPos` + `u16` len +
bytes; `cluster16`, `next_node u64`, `committed u64`, `u16` count, records sorted by `code16`, each
`actor16 request16 verifier32 expires u64` + flag + (`request16 key32 node u64` if consumed);
trailer SHA-256 of all preceding bytes (corruption detection, not authentication).

Decode rejects: wrong header/version or digest; flag not 0/1; trailing or missing bytes;
membership without applied, membership `index > applied.index`, or blob length outside 1–4096;
over `MAX_RECORDS` or unsorted/duplicate code IDs; `next_node` 0; consumed node 0, not below
`next_node`, or duplicated; `committed > MAX_TIME_MS`; `expires` outside
`CODE_TTL_MS..=MAX_TIME_MS+CODE_TTL_MS` or `expires+TOMBSTONE_MS <= committed`; over `MAX_PENDING`
usable pending at `committed`. Canonical: `encode(decode(b)) == b`.

## Required tests (`cluster::state_store::tests`; reuse `private_temp`)

1. Round trip of a rich state (pending, consumed, expired-retained, nonzero time, membership,
   applied): `Eq`, canonical bytes, `snapshot()` equals the DB blob. Each decode reject, and
   truncation at every length, gives `Corrupt` without panic.
2. Open fails closed on a flipped blob byte, `user_version` 2, a foreign `Init` cluster, or a held
   lock. `None` accepts only index 0; gap, duplicate or lower term → `OutOfOrder`, nothing changed.
3. Write fault: an existing-pattern `RAISE(FAIL)` trigger on `sm` fails apply; the store is
   poisoned, memory unchanged, later apply `Poisoned`; reopen yields the pre-state.
4. Reopen keeps every boundary: stale proposal → `ClockBehind`; node IDs continue, never reused
   after pruning; same-request redeem retry replays without moving the counter; after
   `TOMBSTONE_MS` that retry is `UnknownCode`.
5. Install refuses stale, foreign, oversized and corrupt blobs unchanged; a valid install equals the
   source; two stores fed the same entries give byte-identical snapshots.
6. Markers: after mint, redeem, retry, snapshot and install with a marker secret, no dir file's raw
   bytes and no store/error/command `Debug` contain secret, proof or exposed text, raw or hex.
7. Crash child, cross-platform, using the `store/crash_tests.rs` spawn and `KillOnDrop` pattern: the
   child applies indexes 0..k, then a `#[cfg(test)]` fault hook pauses inside k+1 (a consuming
   `Redeem`) or an install, and the parent kills it (four spawns, 10 s deadline each). Before
   commit (after `cache_flush`; assert hot-journal magic and page count): reopen is at k, equals the
   reference replay, re-applying k+1 gives the reference result. After commit: at k+1, re-applying
   k+1 → `OutOfOrder`. Install killed before commit keeps the old state; after, the new one.

## Acceptance and probe pass criteria

Hosted Windows, macOS and Ubuntu green: `cargo fmt --check`, `cargo clippy -p arxd --all-targets
--locked -- -D warnings`, `cargo nextest run -p arxd --locked cluster::`, cluster tests under 60 s
per OS (CI stays under 15 min). SQLite stays the state-machine backend only if all three OSes pass
the crash tests with a genuine hot journal, a clean marker scan, and fail-closed decode/open.
Review: only listed paths; no new dependency; no `Serialize`, `Display` or byte accessor on
credentials; no clock, RNG or logging in the store. The claim is process-kill crash consistency on
hosted runners only. Power-loss durability, quorum commit, three-node availability, physical
expiry, signing and mTLS stay open and **unverified on real hardware**.

**Next, not this slice:** (1) durable vote/log storage (ack only after sync; truncate/purge crash
tests); (2) OpenRaft 0.10 adapter over this store, pinned once two weeks old, in-process three-node
test; (3) transport, mTLS and issuance design, deciding A3.
