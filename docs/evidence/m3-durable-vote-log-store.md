# M3 durable vote/log store evidence

Builder: Sol high, 2026-11-15. Base: `88b68dbb3a2bc3785158c89eb8e4b34bc2015134`, atop main `cd28bb91fb7a42307e6c48e360c96adbf36128fd` (PR #143). Implements the retained brief and independent Sol PASS; fresh independent Opus review is still required before publication. No push, PR, merge or watch was performed.

The first commit, `cad7f08c5e72ff780c759f78c6da91942e87e6e3`, extracts fixed-stem private SQLite opening and test helpers. StateStore retains its 64-page policy and existing test assertions. Test helpers use `pub(in crate::cluster)` because `pub(super)` inside `state_store::tests` only exposes them to StateStore, not the sibling log module. This is a visibility correction within the authorized helper scope, not a storage architecture change.

The second commit adds synchronous VoteLogStore, canonical verifier-only command/row/meta codecs, and ten test groups. Log storage requires exactly 4096-byte pages, reads back TRUNCATE/FULL/fullfsync and 2048-page capacity, holds its private file lock, and checks sidecars. It bounds batches to 64, live rows to 512 and encoded rows to 4163 bytes. All 512 maximal membership rows fit; row 513 returns Full without pruning or partial writes. Position and vote comparisons follow their declared field order. Index-zero initialization requires no vote; a higher-term uncommitted vote is valid.

Writes clone metadata, commit the row/meta transaction, then replace memory. SQLite failures poison every subsequent API operation, including getters and empty append. Memory remains pre-state; commit outcome is unknown until reopen. Trigger faults exercise rollback/pre-state recovery, not a claim that every SQL error guarantees rollback. No committed-position storage or OpenRaft adapter is present.

## Local checks actually run

All build outputs used `CARGO_TARGET_DIR=D:/Temp/arxcovo-m3-target`; TEMP and TMP were `D:/Temp`. Commands used the existing installed toolchain and locked offline dependencies. No installs, cache cleanup, owner fixtures, accounts, VM/admin operations or Lab Manager access.

- Helper-only `cargo nextest run -p arxd --locked --offline cluster::`: 23 passed, 455 skipped, 1.522 s test time. Existing assertions unchanged. Final helper-only run on the amended visibility: 23 passed, 455 skipped, 0.857 s; run ID `9f636e78-bc13-48f9-8e2e-abadefa670dc`. Source declarations for the new log module and command codec were temporarily restored to the first commit during this run, then restored byte-for-byte.
- Final implementation `cargo nextest run -p arxd --locked --offline cluster::`: 33 passed, 456 skipped, 1.049 s test time; build 6.28 s. Run ID `306831e5-3e88-423c-8088-7f84a28a5c78`. The two ignored child entrypoints run only through their owned parent process-kill tests.
- `cargo fmt --all --check`: passed.
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`: passed; final invocation 3.79 s after the additional corruption/boundary tests.
- `git diff --check`: passed.

An early compile overlapped a premature module declaration with the helper check; the declaration was removed before the successful helper-only run. Compilation also exposed the test-helper visibility issue and one test mutability typo, both corrected. The first complete test run found two Windows file-lock privacy-scan failures; scans now run after dropping the held store, without weakening raw-file assertions. The final tests pass. No broad workspace test was run: the reported eight owner_windows AccessDenied failures remain unattributed, and are not described as a green baseline.

## Test evidence and limits

1. Canonical round trips cover Blank, Mint, Redeem, membership lengths 1/4096 and command time u64::MAX. Every truncation, trailing byte, invalid tag/flag/version/header/digest, invalid index, first/tail/purged mismatch, membership zero/4097 rejects. Semantic mutations retain full payloads and recompute SHA where appropriate; core apply deterministically rejects out-of-range command time.
2. Append rejects gaps, overlaps, duplicates, nonconsecutive/lower-position batches, 65 entries and indexes above i64::MAX. Reopen checks unchanged state. Capacity fills eight 64-entry maximal batches, checks all payloads on reopen and atomic Full. An i64::MAX tail truncate/read/reopen probe protects SQL bound conversion.
3. Reads cover empty, reversed, clipped, purged, past-tail and limited ranges. A middle-row deletion poisons reads and all later APIs.
4. Truncate/purge cases are independent: tail10/purge0/append11 succeeds; purge20 requires next21 and rejects11. Tests exercise exclusive truncation, inclusive purge, sparse starts, mismatched ids, below-purged refusal and replacement of a conflicting tail.
5. Vote tests cover None, equal without writes, term/node/committed regressions, higher-term false, reopen durability and index0 without vote.
6. Reopen rejects row/meta bit flips, missing first/middle/last, extra row, valid-checksum position regression, metadata/row mismatch, oversized blob, negative SQL index, version2, foreign cluster, held lock and directory sidecars. Unix-only symlink refusal is compiled conditionally and remains pending hosted Unix execution. A 1024-byte-page database is refused.
7. INSERT-log and UPDATE-meta RAISE(FAIL) triggers preserve cached pre-state, poison all reads/writes/getters, and reopen to the exact original rows/meta.
8. Two independent stores share a directory: durable SM apply precedes purge; reopen checks purged <= applied; remaining entries replay to reference core outcomes. Snapshot install ahead of the log tail survives reopen, then purge and next append restore log alignment. The standalone API cannot enforce purge <= SM.applied; future adapter/startup ordering must enforce it.
9. Synthetic metadata avoids secret-marker collision. Privacy checks scan raw and hex secret/proof/owner-transfer-code markers across row/meta encodings, entry/store/error Debug and every raw store/lock/journal file. Separate positive-control files prove detection. No actual credentials are used.
10. Four owned current_exe kill probes cover append-before, append-after, truncate-before and vote-after. Before-commit hooks call cache_flush; parent verifies genuine hot-journal magic and nonzero page count. After-commit hooks have no nonempty journal. Recovery compares exact pre/post metadata and payloads, reference replay, duplicate-index refusal, lower-vote refusal and privacy scans. Each child has a 10-second deadline and KillOnDrop; log probe group completed in 0.706 s. Existing four StateStore kill probes also passed (0.842 s).

## Remaining gates

Fresh Opus independent code/security review, then coordinator-authorized publication and required hosted Windows/macOS/Ubuntu fmt, Clippy and cluster checks, agent-review and ci-time-budget remain pending. Hosted cluster <60 s and full CI <15 min are acceptance gates, not established by local timing. No OpenRaft dependency/Suite execution, quorum, three-node availability, transport, signing or host power-loss claim. These hardware/consensus paths are **unverified on real hardware**. M2 remains incomplete; A7 FINAL NO-GO and consumed A12 cleanup uncertainty remain unchanged, with no retry.
