# M3 durable enrollment state store: independent Opus source/security review

Reviewer: Claude Opus 5.5, high effort, 2026-11-15. Target: parent `fd6599e` on
`work/m3-next-enrollment-boundary`, base main `4f323c36e2d134b56c1f6efb3035b6cc18143c84`
(PR #142). Sol builder commits `6aa2b8b` → retained `92a4f7f` (verifier boundary + A1) and
`309371e` → retained `45eb7c0` (store, codec, tests, evidence). Stable patch IDs match for both
pairs; `45eb7c0` and `309371e` have the same tree (`fcb9ccc`). Inputs read: AGENTS, KICKOFF,
the brief (`33410cf`, retained `c736cf7`), the Sol slice review (`bf6b6f7`), the evidence document,
ADR 0005 delta and all seven source paths.

**Verdict: source PASS on all three axes. One required test fix (R1) before merge.** The
production behavior is correct. One decode rejection named in the brief has no test that
reaches it. Merge also needs hosted Windows/macOS/Ubuntu gates and a different-provider
re-check of the R1 fix.

## Scope and change hygiene

Source changes touch only the seven files the brief lists. The only docs changes are this slice's
evidence file, the coordinator's PROGRESS edits and the ADR 0005 line from the brief commit. There
are no `Cargo.toml`, lockfile, workflow, gate or dependency changes. The code uses only existing
pins: `rusqlite =0.37.0` bundled, `sha2`, `hex` and dev `tempfile`. The commits are split as the
brief asks: boundary + A1 first, then the store. No OpenRaft, transport, endpoint, signing, CA,
MCP or logging code appears. The store has no clock, RNG, logging, `Serialize`, or `Display` for
credentials (checked by grep).

## Standards

- Errors are static. `From<rusqlite::Error>` maps every SQLite error to `Sql`, and every I/O or
  path failure maps to `Io`. `Display` contains no IDs, paths, SQL text or bytes. `Debug` for
  `StateStore` shows only the image (verifiers, key digests, metadata) and the poison flag.
- No input can make the decoder panic. Length is bounded (≤16384, ≥32) before hashing. Every sum is
  either range-checked first (`committed ≤ MAX_TIME_MS`) or short-circuited behind the range check
  (`expires + TOMBSTONE_MS`). `as u16` casts only bounded values (≤64 records, ≤4096-byte blob).
  The test truncates the blob at every length.
- Field drop order closes `db` before `_lock`, so the lock is released only after the connection
  closes. The `#[cfg(test)]` hook does not compile into production.
- The crash fixture follows the existing `store/crash_tests.rs` pattern: an `#[ignore]` child, a
  re-exec of `current_exe` only, `KillOnDrop`, and a 10 s deadline per child with a 60 s total.
  Like the existing fixture, `--include-ignored` runs it without its environment and fails. That is
  consistent with precedent and not a defect.

## Spec (brief contract)

**Credential boundary (92a4f7f).** `Command::Redeem` now carries only `presented: CodeVerifier`.
`Command::redeem(..., &RedeemProof, ...)` reduces the proof and keeps no reference to it. No
proof can reach entries, state, snapshots, DB files or `Debug`. `apply` keeps the `subtle`
comparison; the doc comment and ADR 0005 correctly call it a consistency check under
trusted-member authority, not authentication. Fresh-mint-only and proof-only ingress are written
down as rules for the future adapter (on `Mint` and `redeem`). This code does not enforce them,
and no claim says it does. The enum fields are public, so "built only by `Command::redeem`" is a
convention, not something the types enforce (see N4). The A1 test (wrong proof on an expired
unconsumed code → `BadProof`) is meaningful: swapping the core's proof/expiry order would make it
fail.

**Canonical blob.** The blob is at most 16 KiB and contains a magic, a `u16` version, an applied
flag and position, a membership flag, position, length and bytes, then the full state section, and
a SHA-256 trailer. Decode rejects a bad header or version, a bad digest, flags other than 0/1, and
trailing or missing bytes. For membership, it rejects a blob with no applied position, a position
above the applied index or term, the same index with a different position, and a length outside
1–4096. For records, it rejects more than 64, unsorted or duplicate code IDs, a duplicate
`(actor, request)`, `next_node` 0, and a consumed node that is 0, not below `next_node`, or
duplicated. It also rejects `committed > MAX_TIME_MS`, `expires` outside
`CODE_TTL_MS..=committed+CODE_TTL_MS` or with `expires+TOMBSTONE_MS ≤ committed`, and more than 8
usable pending at `committed`, using the core's own `usable_time < expires` definition. A final
re-encode comparison enforces canonical form.

I checked that the decoder is **not stricter than what the core can reach**, because a stricter
decoder would brick a live store. Every rule above holds after every `EnrollmentState::apply`
outcome. Expiry is `eff + TTL` with a nondecreasing `eff`. Retention matches `retain`. Pending is
≤8 after a mint at the new `eff`. Node IDs are strictly increasing. Owner lookup comes before
insert.

**Apply.** A poisoned store returns `Poisoned` before the order check. With `applied` `None`, only
index 0 is accepted. After that, apply requires index `applied+1` (checked add) and a term no lower
than the applied term; anything else is `OutOfOrder`, with memory and disk unchanged. Each entry
is applied to a clone. A rejected command still advances the position and stores the core's
deterministic result. `Membership` stores the blob and its position. `persist` writes `UPDATE`,
checks that exactly one row changed, commits, and only then swaps memory. Any error inside the
transaction closure poisons the store. `TooLarge` (unreachable: the maximal image is 13,504 bytes)
returns before SQL runs, so it correctly does not poison.

**Unknown commit outcome.** If `COMMIT` fails, memory stays at the pre-state and the store is
poisoned. The only way forward is to drop and reopen, and reopen trusts disk. SQLite rolls back a
hot journal (pre-state) or keeps a committed truncate (post-state). Either way the index guard
applies k+1 at most once. This design is correct. A commit-time failure itself cannot be injected
without a custom VFS, so it is not tested (residual risk, N5).

**Install.** Checks run in the brief's order: `Poisoned`, `TooLarge`, full decode (`Corrupt`),
`WrongCluster`, then `StaleSnapshot` for an unapplied image, a non-higher index or a lower term.
All of this runs before any effect, then the same commit-then-swap persist follows. A snapshot
carries no quorum proof, and no claim says it does.

**Open.** The directory and every existing lock, db, `-journal`, `-wal` and `-shm` path pass
`checked_path` (absolute, normalized, no links, no reparse points, no ADS) and `security::private`,
plus a regular-file check. The lock is created at `0600` on Unix and held through std `try_lock`.
The db is created at `0600` and checked before SQLite opens it, and sidecars are checked again
after commit. Settings are `busy_timeout` 250, `journal_mode=TRUNCATE`, `synchronous=FULL` and
`fullfsync=ON`, and each is read back exactly. Page size must read back ≤4096,
`max_page_count` exactly 64, and `page_count` ≤64. Because SQLite cannot lower `max_page_count`
below the current size, an oversized db fails the exact readback. In one transaction, creation
makes the table, the initial blob and `user_version=1`; an error before commit rolls back to
`user_version` 0. Version 1 reads the snapshot under `typeof='blob' AND length≤16384`, so no
oversize read happens, and requires exactly one row. Any other version is `Unsupported`, and a
foreign cluster is `WrongCluster`.

## Security

- **Bearer material.** I confirmed the marker derivation in the privacy test is identical to
  `JoinCode::proof()` (domain `arxcovo/join/proof/v1`, cluster, code, secret). The scan therefore
  looks for the real proof, the secret and the transfer text, raw and hex, with a positive control.
  If the store regressed to persisting proofs, this test would catch it.
- **Hot-journal probe is genuine.** The pre-commit hook calls `cache_flush`, which forces the
  journal sync and nRec update before db pages are written. The parent asserts the journal magic
  and nRec > 0, and recovery must equal the pre-state. Counterfactual run (below): removing
  `cache_flush` fails the magic assertion, so the probe is not vacuous.
- **Fail closed.** Malformed blob, version, cluster, lock, sidecar directory, symlink and Unix
  permission cases all refuse with static errors. A tampered db written with the owner's own
  rights, with the checksum recomputed, can still decode if it is semantically valid. That is
  expected: SHA-256 detects corruption and does not authenticate, the files are owner-private, and
  the threat model already gives the owner full authority.

## Findings

**Required before merge**

- **R1. The decoder's membership-length upper bound is never exercised.** In
  `canonical_roundtrip_and_semantic_decoder_rejections`, the `4097` length edit at `x[60..62]`
  fails because too few bytes remain, not because of the 1–4096 validator: the rich payload is
  about 436 bytes. Counterfactual run: raising `MembershipBlob::new`'s limit to 8192 leaves the
  test **passing**. The brief's test 1 requires every decode rejection to give `Corrupt`, and
  AGENTS requires a meaningful test for each behavior.
  **Fix:** in `state_store/tests.rs`, encode an `Image` whose membership is built directly as
  `MembershipBlob(vec![0x42; 4097])` (the tests module can reach the private field). The image is
  under 16 KiB. Assert that `decode` returns `Corrupt`, that `MembershipBlob::new(vec![0; 4097])`
  and `MembershipBlob::new(vec![])` are errors, and that `vec![0; 4096]` is accepted. This is a
  test-only change.

**Non-blocking (record for the adapter or a later slice; not required here)**

- N1. A decoded image with `applied: None` may hold non-initial state. The API cannot reach this,
  and it needs owner write access, but requiring `applied: None` ⇒ no membership, no records and
  `committed_ms == 0` would be cheap hardening.
- N2. `apply` accepts the same term with a different `leader`. Correct Raft never produces this,
  so the adapter should own this rule rather than the store.
- N3. A private, regular `-wal` or `-shm` file is accepted, not refused. The EXCLUSIVE → TRUNCATE
  → NORMAL sequence checkpoints any WAL, and full decode still runs afterwards. There is no test
  that a pre-existing WAL-mode db or one with `page_size > 4096` is refused.
- N4. Any code in the crate can still build `Command::Redeem { presented, .. }`. The future
  ingress and forwarding types must accept only `RedeemProof`, never a verifier.
- N5. The adapter must treat any `StoreError::Sql` or `Poisoned` from `apply` as an **unknown**
  outcome. It must drop and reopen, continue from `applied()`, and must not give a client a final
  failure for that index. Redeem retries rely on the retained-record dedup (bounded by
  `TOMBSTONE_MS`).

## Checks

**Run by this reviewer** (Windows 11, normal user, `CARGO_TARGET_DIR=D:/Code/Arxcovo/target`,
`TEMP`/`TMP=D:\Temp`, offline/locked, no installs or cache deletion):

- `cargo nextest run -p arxd --locked --offline cluster::`: 22 passed in 0.908 s; the four-kill
  test took 0.761 s.
- `cargo nextest run -p arxd --locked --offline cluster::state_store` five more times: 8/8 each
  time, 0.68–1.16 s, no flakes.
- `cargo fmt --all --check`: passed. `cargo clippy -p arxd --all-targets --locked --offline -- -D
  warnings`: passed.
- Temporary counterfactual mutations, each reverted with `git checkout` (worktree clean afterwards):
  1. Membership limit raised to 8192: the decoder test still **passes**, which confirms R1.
  2. `cache_flush` removed from the pre-commit hook: the four-kill test fails at the journal-magic
     assertion (`tests.rs:457`).
  3. Decoder pending cap disabled: the decoder test fails at the nine-pending assertion
     (`tests.rs:159`).

**Taken from the builder, not re-run:** the 14-test boundary-only run and the full-workspace run
(352 passed, 8 failed, 4 skipped, 115 unrun). All eight failures are the unchanged
`service::owner_windows::tests` fixtures failing at `owner_windows_tests.rs:49`
(`SetNamedSecurityInfoW` access denied). No baseline exists, so they are neither attributed to nor
cleared by this slice, and the workspace is not claimed green. No admin remediation was attempted.

**Not run:** hosted Windows, macOS and Ubuntu gates (required, still pending). Local Windows results
are not Linux or macOS evidence, and they do not stand in for hosted Windows either.

## Risks and claims boundary

This shows only local process-kill crash consistency for the state machine. It does not show
power-loss durability, quorum commit, vote or log durability, three-node availability, physical
expiry, signing, the CA or mTLS. Those remain open and are **unverified on real hardware**. SQLite
stays provisional until all three hosted OSes pass the genuine hot-journal probes and the marker
scans.

M2 remains incomplete. A7 FINAL NO-GO stands, and the consumed A12 D7l cleanup is still
unconfirmed with no retry. Nothing here grants any M2 credit.

## Eligibility

The source and security design are accepted. Merge requires: (1) the R1 test fix, with focused
cluster tests, fmt and Clippy re-run; (2) a different-provider re-check of that fix; (3) green
hosted Windows, macOS and Ubuntu `fmt`, Clippy and `cluster::` nextest, with cluster tests under
60 s per OS and CI under 15 min; (4) the coordinator's merge. This reviewer made no source,
PROGRESS or ADR edits and performed no push, PR, status, comment, merge or watch.
