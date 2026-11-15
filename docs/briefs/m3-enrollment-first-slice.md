# Brief: M3 enrollment first slice (pure join-code core)

Status: **proposed** (Opus 5.5 high, 2026-11-15), corrected on `a746cff` for Sol review T1/T2
(`docs/reviews/m3-enrollment-first-slice-sol-review.md`); code waits for Sol's delta recheck. M3 may
proceed while M2 is open. Nothing built or tested; no three-node, expiry, certificate or M2 claim.

## Scope and files

A deterministic, I/O-free join-code state machine that later OpenRaft storage, rcgen issuance and
rustls mTLS wrap unchanged ([ADR 0005](../adr/0005-raft-membership-and-enrollment-spike.md)). No
clock reads; randomness only through an injected fill. No network, tokio, storage, CA key, signing,
MCP/HTTP route, host or native code; **no new dependencies** beyond the `sha2`, `subtle`, `getrandom`
and `hex` pins. Still gated, unclaimed: CA-key placement and signing role, key storage, rotation,
revocation, certificate lifetime, transport split, storage backend, voter cap, learner policy, code
revocation and rate limits. Files (exact): `crates/arxd/src/lib.rs` gains only `pub mod cluster;`;
`cluster.rs` holds `pub mod enrollment;`; under `crates/arxd/src/cluster/`, `enrollment.rs` (types,
`Command`, `EnrollmentState::apply`, `Outcome`, issuance guard), `enrollment/code.rs` (`JoinCode`
text, hashing), `enrollment/tests.rs`. No other path changes; the coordinator owns `docs/PROGRESS.md`.

## Types, secrets and join code text

- `ClusterId`, `CodeId`, `RequestId`, `ActorId`: `[u8; 16]`; `KeyDigest`, `CaFingerprint`:
  `[u8; 32]`; `NodeId`: nonzero `u64`. Constants: `CODE_TTL_MS = 900_000`, `SKEW_MS = 30_000`,
  `TOMBSTONE_MS = 86_400_000`, `MAX_PENDING = 8`, `MAX_RECORDS = 64`, `MAX_CODE_TEXT = 4096`,
  `MAX_SPKI = 2048`, `MAX_TIME_MS = 253_402_300_799_999` (end of 9999 UTC).
- `KeyDigest::of_spki(&[u8])` takes 1..=`MAX_SPKI` bytes of caller-canonical SPKI DER and returns
  their SHA-256. SPKI parsing and CSR proof-of-possession belong to the gated issuance slice.
- `JoinSecret` is 32 random bytes. `RedeemProof = SHA256("arxcovo/join/proof/v1" || cluster ||
  code_id || secret)` and `CodeVerifier = SHA256("arxcovo/join/verifier/v1" || proof)`. State
  stores only the verifier, so a snapshot cannot redeem a pending code.
- `JoinSecret`, `RedeemProof` and `JoinCode` implement neither `Display` nor `Serialize`; `Debug`
  prints `<redacted>`. Zeroization is not claimed. `code_id` is not a credential; the secret, the
  exposed text and the proof are. A future redeem log holding the proof is outside this claim.
- `JoinCode::generate(cluster, ca_fp, fill)` draws `code_id` and `secret` from `fill` (production
  `getrandom::fill`; error `CodeError::Rng`). Text: `"arxj1-"` + hex of `cluster16|code_id16|
  secret32|ca_fp32|check4` (206 chars); `check4` = first 4 bytes of `SHA256("arxcovo/join/check/v1"
  || first 96 bytes)`, a paste check, not a security control.
- `JoinCode::parse(&str)` rejects input over `MAX_CODE_TEXT` bytes, trims ASCII whitespace, needs
  exact prefix, length, ASCII hex and checksum; every failure is unit-like `CodeError::Malformed`,
  never echoing. Plaintext only via `expose_for_owner_transfer(&self) -> String`, never logged/MCP.

## Commands and apply (deterministic; replicas must agree)

Commands: `Mint { cluster, actor, request, code, verifier, proposed_at_ms }` and `Redeem { cluster,
request, code, proof, key, proposed_at_ms }`. The minting host shows its in-memory plaintext after
commit; a retried mint cannot re-reveal it. `EnrollmentState::new(cluster, first_free: NodeId)` and
`apply(&mut self, &Command) -> Outcome` work over `next_node_id`, `committed_ms` and
`BTreeMap<CodeId, CodeRecord{actor, request, verifier, expires_ms, consumed}>` (`consumed`:
`Option<{request, key, node}>`). State derives `Clone + Eq + Debug`, with no credential in `Debug`.
A code is **usable** when it is unconsumed and `eff + 2*SKEW < expires_ms`.

- **Both commands, in order:** (1) wrong cluster: `WrongCluster`; (2) `proposed > MAX_TIME_MS`:
  `TimeOutOfRange`; (3) `proposed + SKEW < committed`: `ClockBehind`; (4) else `eff =
  max(committed, proposed)`, set `committed = eff`, and prune records with `expires + TOMBSTONE
  <= eff`. Pruning is the only removal; no dedup record is ever dropped early to make room.
- **Mint:** (1) a record with the same `(actor, request)` returns `Minted{replay: true}` with the
  same expiry if code and verifier match, else `RequestConflict` (both answer even when full);
  (2) an existing `code_id` returns `CodeIdCollision`; (3) `MAX_PENDING` usable codes return
  `TooManyPending`; (4) `MAX_RECORDS` retained records of any kind (pending, expired or consumed)
  return `StateFull`; (5) otherwise insert with `expires_ms = eff + CODE_TTL_MS`.
- **Redeem** (never inserts): (1) missing code: `UnknownCode`. (2) A failed `subtle` constant-time
  verifier comparison returns `BadProof` without consuming; the 256-bit secret means no attempt
  counter. (3) On a consumed code, the same request and key replay the grant (`replay: true`, same
  node, no allocation); the same request with another key returns `KeyConflict`; else `Consumed`.
  (4) An unusable code returns `Expired`. (5) `node = next_node_id` is allocated only if
  `next_node_id.checked_add(1)` succeeds (`u64::MAX` is never issued), else `NodeIdsExhausted`.
  (6) Otherwise consume atomically, advance `next_node_id`, and return `Enrolled(EnrollmentGrant
  {cluster, node, key, request, code_expires_ms, applied_ms: eff, replay})`.

**State effects.** `WrongCluster`, `TimeOutOfRange` and `ClockBehind` leave state byte-identical;
every later reject commits only the step (4) ratchet and pruning, consumes no code and never moves
`next_node_id`. Stored times stay at most `MAX_TIME_MS + CODE_TTL_MS`, so sums fit `u64`; every
time sum still uses `checked_add`, and an unreachable `None` is `TimeOutOfRange` before mutation.
A minter filling `MAX_RECORDS` gets `StateFull` for up to ~24 h 15 min (owner-scoped liveness loss,
never early pruning). Node IDs only increase, never reused after pruning or removal. **Error privacy:**
`Outcome::joiner_reply()` maps a grant to `Enrolled{node}` and `ClockBehind` to `RetryLater`;
everything else, mint outcomes and `TimeOutOfRange` included, is one `Rejected`. `Reject` and
`IssueRefusal` `Display` strings are static and contain no IDs.

## Grant authority, issuance guard and clock contract

`EnrollmentGrant` has private fields and no public constructor; only `apply` creates one. That only
blocks in-crate fabrication. **It is not proof of consensus commit:** anyone can `apply` to a local
uncommitted clone. The gated adapter must pass to signing only grants from applying durably
quorum-committed entries and sign only for `grant.key()`; **no certificate before commit** stays a
future-adapter obligation this slice does not verify.

Pure guard, time injected: `EnrollmentGrant::check_issuable(&self, signer_now_ms: u64) ->
Result<(), IssueRefusal>`: (1) `signer_now_ms.checked_add(SKEW)` is `Some(t)` with `t <
applied_ms`: `SignerClockBehind`; (2) `signer_now_ms.checked_add(2*SKEW)` is `None` or `>=
code_expires_ms`: `GrantExpired`; (3) else `Ok(())`. Future-adapter rules: read the signer's system
clock fresh for each check; check right before signing and again after signing before releasing or
persisting the certificate, discarding it on refusal; never cache a pass. Every retry, a `replay`
grant with a lost certificate included, re-runs both checks; an expired code needs a new code.

**Clock contract. The pure proposal-time model proves neither physical expiry nor commit
authority:** a sole delayed redeem (mint at 0, proposed at 1 000, applied at 3 600 000, no other
command) still gets a grant; `ClockBehind` fires only after later commands pass a stale proposal by
`SKEW`. Conditional claim, only if the adapter obeys the guard rules and every proposing leader's
and the signer's clocks are within `SKEW_MS` of true time (NTP expected, not verified): stored expiry
is at most mint-commit real time + 15 min + `SKEW`, so the 2×`SKEW` guard releases no certificate
over 15 real minutes after the mint committed (plaintext is shown only after commit). Absent jumps,
a code stays issuable ~13 min (15 − 4×`SKEW`) minus latency (liveness). Rollback over `SKEW` fails
closed; a forward jump ratchets `committed`, expiring codes early and stalling correct leaders with
`ClockBehind` until real time catches up; undetected error over `SKEW` extends life by the excess.

## Required tests (`cluster::enrollment::tests`)

1. Generate/expose/parse round trip; oversize, wrong prefix or length, non-hex, non-ASCII and bad
   checksum each return `Malformed`; no `Display`/`Debug` contains the input.
2. Secret marker through mint/redeem/retry: `Debug` of state, outcomes, grant, code, proof and
   secret never contains the marker or proof (raw or hex); only the verifier may appear in state.
3. Two codes give increasing node IDs, never reissued after pruning; identical retry replays the
   grant, `next_node_id` unchanged. New request/same key, same request/new key, both new: exact
   rejects, only `committed_ms` moves.
4. Race: keys A and B redeem one code in both orders; one grant, to the first applier, counter +1.
   Wrong proof is `BadProof` without consuming, then the right one succeeds; wrong cluster rejected.
5. Boundary: redeem at `expires − 2*SKEW − 1` succeeds, at `expires − 2*SKEW` is `Expired`. Clock:
   rollback within `SKEW` uses `committed`; beyond is `ClockBehind`, byte-identical; forward jump:
   `Expired`, then correct clocks get `ClockBehind`; a stale proposal applied after a later command
   is `ClockBehind`; `MAX_TIME_MS + 1` is `TimeOutOfRange`, byte-identical.
6. Mint replay, `RequestConflict`, `CodeIdCollision`, `TooManyPending`, freed slot after expiry;
   pruned code is `UnknownCode`. `first_free = u64::MAX − 1`: one grant, then `NodeIdsExhausted`
   moves only `committed_ms`, leaving the code unconsumed and `next_node_id` unchanged.
7. Retention: mint+redeem `MAX_RECORDS` codes; a new mint is `StateFull` (only `committed_ms` moves,
   none removed), a replay still answers, and a mint at oldest `expires + TOMBSTONE` succeeds.
8. Delayed sole proposal: mint at 0, sole redeem proposed at 1 000; `apply` grants (named
   proposal-time semantics, not expiry proof), then `check_issuable(3_600_000)` is `GrantExpired`.
   `code_expires − 2*SKEW − 1` is `Ok`; `code_expires − 2*SKEW` and `u64::MAX` are `GrantExpired`;
   with `applied_ms > SKEW`, `applied_ms − SKEW − 1` is `SignerClockBehind`, `applied_ms − SKEW` not.
9. `joiner_reply` gives one `Rejected` for every reject except `ClockBehind`; one sequence applied
   to two clones gives equal states and outcomes.

## Acceptance for the Sol implementation

Required green on hosted gates: `cargo fmt --check`, `cargo clippy -p arxd --all-targets --locked
-- -D warnings`, `cargo nextest run -p arxd --locked cluster::enrollment`. Review checks: diff only
the listed files, no new dependency, no clock or RNG call outside `fill`, no `Serialize`/`Display` on
credentials. Pure-core results only; commit authority, signing, physical expiry, three-node,
storage, crash, mTLS and LAN/Tailscale behavior remain open and **unverified on real hardware**.
