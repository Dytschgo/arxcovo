# M3 durable vote/log store: independent Opus source/security review

Reviewer: Claude Opus 5.5, high effort, 2026-11-15. Target: parent
`2d88842b1e6c828d5c8262e5d179b1bea533207e` on `work/m3-durable-log-boundary`, base actual main
`cd28bb91fb7a42307e6c48e360c96adbf36128fd` (PR #143, an ancestor of the target). Sol builder commits:
`cad7f08c5e72ff780c759f78c6da91942e87e6e3` → retained `189bbc9` (shared helper refactor) and
`07da6a539564c757b0bbe546ee9294c38ef5b6ba` → retained `077e121` (store, codec, tests, evidence).
Both pairs have equal trees (`0c6dfd3`, `b9bb6db`) and equal stable patch IDs (`9efb6e32dfea`,
`151f956f9489`). Inputs read: AGENTS, KICKOFF, the brief (Opus `cf5e1da` → retained `48b8185`,
with the Sol `88b68db` clarifications), the Sol slice review, the evidence document, the PROGRESS
delta and all six source paths. I also read the retained OpenRaft 0.10.0 source under `D:/Temp`
without running it.

**Verdict: Security PASS. Spec PASS for production semantics. Standards BLOCK on one test-coverage
finding (R1).** No production change is required. R1 needs only added tests. Merge also needs a
different-provider recheck of the R1 fix and fresh hosted Windows/macOS/Ubuntu gates.

## Reviewed blobs at `2d88842`

| Path | Blob |
| --- | --- |
| `crates/arxd/src/cluster.rs` | `87e9e1cf1a8328fab9bce2a6763e39b21ce4e98a` |
| `crates/arxd/src/cluster/enrollment/codec.rs` | `0d3b58c3e7b0b7767dabf8795abb57bec060e9da` |
| `crates/arxd/src/cluster/log_store.rs` | `1c50a296af1cf21289859e2fb1400491b232c8ae` |
| `crates/arxd/src/cluster/log_store/tests.rs` | `45ceb0517a5e83e6d0e0d63fcd8adff9497c3156` |
| `crates/arxd/src/cluster/state_store.rs` | `35640b8fc591a01d73d91374857a0965cdc5310a` |
| `crates/arxd/src/cluster/state_store/tests.rs` | `45333a876dc9b41e2de2ff2a91d4877578bdee12` |
| `docs/evidence/m3-durable-vote-log-store.md` | `5112daa603f76cbf71818b25706818851470efe2` |

Full tree: `9c4b4a33d623bfdee55e252af7a694a6d5c818aa`.

## Scope and hygiene

The builder commits touch only the paths the brief lists. They change no `Cargo.toml`, lockfile,
workflow, gate, dependency, ADR or PROGRESS file. The PROGRESS change is in the coordinator's commit
`2d88842`. The only crates used are existing pins (`rusqlite =0.37.0` bundled, `sha2`). There is no
OpenRaft dependency, transport, endpoint, signing, MCP or logging code, and no clock, RNG or thread
in the store. `git diff --check cd28bb9 2d88842` is clean.

## Standards

- **Helper refactor keeps StateStore behavior.** `open_db` is the old open sequence moved as-is:
  directory privacy, the five sidecar `symlink_metadata`/`private`/`is_file` checks, the held
  `try_lock`, private creation, the same pragma batch, and the same readback table. Only the stem
  and page cap are parameters now. For `cluster-state`, it still uses 64 pages with `page_size`
  `≤4096`, so the bound is unchanged. For `cluster-log`, the page size must be exactly 4096 and
  the cap is 2048 pages. Any other stem returns `Unsupported` (tested with `"../escape"`). Because
  `max_pages` is a `u64`, the formatted pragma cannot inject SQL. The `state_store/tests.rs` diff
  only widens visibility, so all 23 baseline assertions are byte-identical. They pass at HEAD
  (13 enrollment + 10 state store). `189bbc9..077e121` does not touch `state_store.rs`.
- **Visibility judgment: accepted.** The brief's `pub(super)` on helpers inside
  `state_store::tests` would expose them only to `state_store`, so the sibling
  `log_store::tests` could not use them. `pub(in crate::cluster)` is the narrowest scope that
  compiles. All of it is `#[cfg(test)]`. The non-test items `open_db`, `put_pos` and `get_pos` are
  `pub(super)`, which is cluster scope. This deviation is mechanical and needs no ADR.
- Errors are static: three new `Display` arms, and `From<rusqlite::Error>` maps to `Sql`.
  `VoteLogStore`'s `Debug` shows only the meta (cluster ID, vote, positions) and the poison flag.
  No input makes the code panic: `prev.unwrap()` runs only after a non-empty check, and each `[0]`
  index follows an `entries` call whose `Ok` path guarantees exactly one row. Every `u64`→`i64`
  cast is bounded by an `i64::MAX` gate or by the tail invariant. The exclusive bound `MAX+1`
  becomes `≤ end-1`, which the `i64::MAX` tail truncate/read/reopen probe covers. The
  `#[cfg(test)]` hook is not compiled into production.
- The crash fixture follows the existing pattern: an ignored child that re-runs `current_exe`,
  `KillOnDrop`, a 10 s deadline per child and a 60 s total, with environment-scoped paths only.

## Spec (brief contract)

- **Open and scan.** Fixed stems and sidecars, a held lock, and readback of TRUNCATE, FULL,
  fullfsync, cap 2048 and an exact 4096-byte page. `user_version` 0 creates the schema and meta in
  one transaction. Any version other than 0 or 1 returns `Unsupported`. The meta read checks for a
  singleton, a blob of at most 256 bytes, no second row, and `WrongCluster`. Negative indexes are
  refused. The scan reads at most 513 rows and decodes each with SQL `idx == pos.index`. It requires
  contiguous indexes, strictly increasing `LogPos`, first row = tail first, last row = tail last,
  the right count, and a first row above purged. Rows without a tail return `Corrupt`.
- **Codec.** The row is magic, `LogPos`, tag, payload, then SHA-256. The command codec stores only
  `CodeVerifier`/`KeyDigest`/IDs, never a proof or code text. It accepts every `u64` time (apply
  rejects `u64::MAX` deterministically, and a test covers this). Membership is 1..=4096 bytes with
  a `u16` length. Decoding rejects truncation, trailing bytes and unknown tags, and re-encodes to
  check canonical form. Meta flags must be 0 or 1. It checks `first ≤ last.index`, a tail of at
  most 512 rows, `first == purged+1`, `last > purged`, and indexes `≤ i64::MAX`. The trailer
  detects corruption but does not authenticate, and the module doc says so.
- **Capacity.** Max row = 8+24+1+2+4096+32 = 4163, which the test asserts. The test fills eight
  64-entry batches of maximal membership rows, checks `page_count ≤ 2048`, verifies all 512 payloads
  after reopen, and gets an atomic `Full` on row 513 with meta unchanged. Over 64 entries,
  `>i64::MAX` and an oversized body each return `TooLarge` before SQL. Nothing is pruned
  automatically.
- **Vote.** `StoredVote{term,node,committed}` uses derived field order. This matches OpenRaft's
  `leader_id_adv::LeaderId{term,node_id}` (derived `Ord`) followed by `RefVote`'s committed
  tie-break (`vote/ref_vote.rs:36-57`, adv mode is total). A lower vote returns `VoteRegressed` and
  leaves state unchanged. An equal vote writes nothing (asserted with `total_changes`). A higher
  vote, including a higher-term uncommitted one, is committed before return. There is no rule
  that the vote must be at least the entry, and index 0 appends without a vote.
- **Append.** The expected first index is tail+1, else purged+1, else any index (sparse start for
  Suite and `initialize`). The batch must be consecutive, strictly above the previous `LogPos`,
  and must not overlap. `Full` is checked after order validation. Rows and the meta tail go in one
  transaction. COMMIT happens before memory changes, so the call returns only after the data is
  durable.
- **Truncate and purge.** Truncate is exclusive and purge is inclusive. Both handle mismatched IDs
  as `Conflict`, below-purged as `OutOfOrder`, past-tail truncate as a no-op, past-tail purge as
  last = purged, and sparse-start rules, all as the corrected brief specifies. The test runs the
  Suite cases independently, not chained.
- **Reads.** Ranges are clipped to `[max(start,first), min(end,last+1))`, and `limit` must be in
  1..=512. Invalid arguments return before SQL and do not poison. Row count, the first index,
  `≤ last`, `> purged` and the tail ID are re-verified on every read. A mismatch poisons the store.
- **Poison and unknown outcome.** Any error inside `persist` or a read poisons the store. Memory
  changes only after `checked(result)?`. `vote`, `log_state`, `live`, `entries`, empty `append`,
  `truncate_after`, `purge` and `save_vote` all return `Poisoned`. Trigger faults on INSERT log and
  UPDATE meta reopen to the exact pre-state. Mutant M2 below confirms the tests detect a
  memory-before-durability swap.
- **Cross-store I4/I5.** `purge` documents that the caller must durably apply or install through
  `id` first. The store does not and cannot enforce `purged ≤ sm.applied`, and neither the
  evidence nor the code claims it does. The two-store test shows ordered use and install-ahead
  followed by `purge(applied)` and append `applied+1`. StateStore still refuses sparse apply. Both
  remain obligations for the future adapter and its startup.
- OpenRaft alignment was checked by reading the source only. `LogId{leader_id, index}` uses
  derived `Ord`, so `LogPos` field order matches it. `truncate_after` is exclusive and `purge` is
  inclusive (`storage/v2/raft_log_storage.rs:133-148`). The OpenRaft Suite was not run.

## Security

The store uses only private fixed paths: the directory, lock and DB, plus sidecars checked before
and after opening (`checked_path` + `private` refuse reparse points). Directory sidecars are refused
(tested), and Unix symlink refusal is compiled only under `cfg(unix)`. Every read is bounded: meta
≤256 bytes, rows ≤4163 bytes checked before `to_vec`, at most 513 rows scanned, and read limit ≤512.
SQL CHECK constraints add a second bound. The files hold no secret or proof bytes. Privacy scans
cover every raw file in the directory, raw and hex markers, `Debug` and `Display`, with a positive
control. The Windows fix moved scans after the store is dropped and its lock released. Every file
is still scanned and no assertion was weakened. No credentials, network, accounts or host admin
are involved.

## Findings

### R1 (Standards, blocking): spec'd fail-closed branches have no detecting test

AGENTS requires a meaningful test for every behavior. The brief specifies each guard below, but
removing it leaves all ten log-store tests green. I applied each mutant to HEAD one at a time, ran
`cluster::log_store`, then restored the file with `git checkout`. The file's SHA-256 matched
afterwards and the worktree was clean.

| Mutant | Guard removed (`log_store.rs`) | Result |
| --- | --- | --- |
| M1 | purge past tail with `id <= last` → `OutOfOrder` (504) | survives |
| M5 | scan `first row > purged` in `verify` (308) | survives |
| M6 | truncate `a.index == purged.index && a != purged` → `Conflict` (450) | survives |
| M7 | purge with no tail and `id <= purged` → `OutOfOrder` (512) | survives |
| M8 | sparse purge requires `id < first row` (497) | survives |
| M9 | read re-check `row > purged` (339) | survives |
| M11 | meta decode `last <= purged` → `Corrupt` (136) | survives |
| M12 | read re-check of the tail row ID (340-342) | survives |
| M2 | swap memory before `checked(result)?` (386-387) | **caught** by the SQL-fault test |

Several surviving mutants hurt safety, not just style. M6 lets a mismatched `truncate_after` at the
purged index delete the whole tail. M1 and M7 let the recorded `last`/`purged` go backwards in
`LogPos` order. M5 and M11 let a checksum-valid but inconsistent meta/row pair open.

**Required fix (tests only, production unchanged).** Add assertions that each fail under the
matching mutant. Error-path cases also need meta and rows unchanged after reopen:

1. Tail `1..=10` at term 2: `purge(LogPos{term:1, leader:3, index:20})` → `OutOfOrder` (M1).
2. After `purge(pos(20))` with no tail: `purge(LogPos{term:1, ..pos(25)})` → `OutOfOrder` (M7).
3. Purged `pos(3)` with a tail: `truncate_after(Some(LogPos{leader:4, ..pos(3)}))` → `Conflict`
   (M6).
4. Sparse rows `5..8` at term 2: `purge(LogPos{term:3, ..pos(4)})` → `Conflict` (M8).
5. Reopen with purged `pos(3)` and a valid-checksum row 4 rewritten as `LogPos{term:1, ..pos(4)}`.
   It is not the tail row, so the meta tail still matches → `Corrupt` (M5).
6. `bad_meta` on the purged+tail image with `last.term` (bytes 78..86) set below purged's term →
   `Corrupt` (M11).
7. After opening, rewrite rows through `s.db` with valid checksums: the row at `first` with a term
   below purged (M9), and the tail row with a lower leader (M12) → `Corrupt` and poisoned.

Then update the evidence. Item 1's "first/tail/purged mismatch" currently overstates the coverage.
Also, the 4097-byte membership case is rejected by the `MAX_ROW` reader bound before
`MembershipBlob::new` runs. Both bounds hold, and the constructor stays covered by the merged
state-store test, but the wording should say which bound rejects it.

No other blockers. No advisories are added beyond those the brief already carries forward
(fresh-mint ingress; SQL unknown outcome N5, where a commit-time failure cannot be injected without
a custom VFS).

## Checks run by this reviewer

Windows 11, normal user, `CARGO_TARGET_DIR=D:/Temp/arxcovo-m3-target` (incremental),
`TEMP=TMP=D:/Temp`, locked and offline, existing toolchain. No installs, deletions or cache
cleanup. The only subprocesses were the owned `current_exe` children.

- `cargo fmt --all --check`: pass.
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`: pass (8.1 s).
- `cargo nextest run -p arxd --locked --offline cluster::`: **33 passed, 456 skipped**, 1.18 s,
  run `a53d67b7-a7c3-47ce-bd23-5f7552ea66fe`. This covers both four-kill groups (log 0.83 s,
  state 0.95 s) and the 23 unchanged baseline tests.
- `git diff --check cd28bb9 2d88842`: clean. Builder/retained tree and patch-ID equality is listed
  above.
- Nine temporary mutants, as described in R1.

Reused, not re-run: the builder's standalone 23-test run at the refactor commit. The sources at
HEAD are what I verified. I ran no full workspace tests. The eight reported `owner_windows`
AccessDenied fixtures remain unattributed. They are not green and have no admin remediation. I did
not run the hosted three-OS checks, the OpenRaft Suite or any power-loss test.

## Claims and remaining gates

The evidence correctly claims only local process-kill consistency. Power loss, quorum, OpenRaft
integration, Suite execution and three-node availability remain open, **unverified on real
hardware**. Unix symlink refusal needs hosted Unix execution. Before merge: fix R1, get a fresh
different-provider recheck, then pass the required hosted Windows/macOS/Ubuntu fmt, Clippy and
cluster gates (cluster <60 s per OS, CI <15 min), agent-review and ci-time-budget. Only the
coordinator merges. M2 stays incomplete. A7 FINAL NO-GO and the consumed, unconfirmed A12 cleanup
stay unchanged, with no retry.
