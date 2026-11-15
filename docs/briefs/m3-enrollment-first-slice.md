# Brief: M3 enrollment first slice (pure join-code core)

Status: **proposed** (Opus 5.5 high, 2026-11-15) on parent `5056d90`, accepted main `0df242e`;
implementation waits for independent Sol review and coordinator disposition. Steering lets M3 work
proceed while M2 is open (the coordinator owns the stale "no M3 before M2" checklist line). Nothing
was built or tested here. Pure contract only: no M3 three-node or reused/expired-code acceptance, no M2 closure.

## Scope and files

A deterministic, I/O-free join-code state machine that later OpenRaft storage, rcgen issuance and
rustls mTLS wrap unchanged; [ADR 0005](../adr/0005-raft-membership-and-enrollment-spike.md) applies.
It reads no clock and draws randomness only through an injected fill function. It adds no network,
tokio, storage, CA key, signing, MCP/HTTP route, host or native code. **No new dependencies:** it
uses the existing `sha2`, `subtle`, `getrandom` and `hex` pins.

Still gated, no acceptance claimed: CA-key placement and signing role, key storage, rotation and
revocation, certificate lifetime, transport split, storage backend, voter cap and learner policy,
code revocation, and transport rate limits. Files (exact): `crates/arxd/src/lib.rs` gains only
`pub mod cluster;`; `cluster.rs` holds `pub mod enrollment;`; under `crates/arxd/src/cluster/`,
`enrollment.rs` (types, `Command`, `EnrollmentState::apply`, `Outcome`), `enrollment/code.rs`
(`JoinCode` text and hashing) and `enrollment/tests.rs`. No other production path changes; the
coordinator owns the `docs/PROGRESS.md` entry.

## Types, secrets and join code text

- `ClusterId`, `CodeId`, `RequestId`, `ActorId`: `[u8; 16]`; `KeyDigest`, `CaFingerprint`:
  `[u8; 32]`; `NodeId`: nonzero `u64`. Constants: `CODE_TTL_MS = 900_000`, `SKEW_MS = 30_000`,
  `TOMBSTONE_MS = 86_400_000`, `MAX_PENDING = 8`, `MAX_CODE_TEXT = 4096`, `MAX_SPKI = 2048`.
- `KeyDigest::of_spki(&[u8])` takes 1..=`MAX_SPKI` bytes of caller-canonical SPKI DER and returns
  their SHA-256. SPKI parsing and CSR proof-of-possession belong to the gated issuance slice.
- `JoinSecret` is 32 random bytes. `RedeemProof = SHA256("arxcovo/join/proof/v1" || cluster ||
  code_id || secret)` and `CodeVerifier = SHA256("arxcovo/join/verifier/v1" || proof)`. State
  stores only the verifier, so a snapshot cannot redeem a pending code.
- `JoinSecret`, `RedeemProof` and `JoinCode` implement neither `Display` nor `Serialize`; `Debug`
  prints `<redacted>`. Zeroization is not claimed. `code_id` is not a credential; the secret, the
  exposed text and the proof are.
- `JoinCode::generate(cluster, ca_fp, fill)` draws `code_id` and `secret` from `fill` (production:
  `getrandom::fill`; error: `CodeError::Rng`). Text: `"arxj1-"` + hex of
  `cluster16|code_id16|secret32|ca_fp32|check4` (206 chars); `check4` = first 4 bytes of
  `SHA256("arxcovo/join/check/v1" || first 96 bytes)`, a paste check, not a security control.
- `JoinCode::parse(&str)` first rejects input over `MAX_CODE_TEXT` bytes, then trims ASCII
  whitespace and requires the exact prefix, length, ASCII hex and checksum; every failure is the
  unit-like `CodeError::Malformed`, never echoing input. Plaintext appears only via
  `expose_for_owner_transfer(&self) -> String` for the later owner "Add host" UI; nothing in this
  slice logs it or returns it over MCP.

## Commands and apply (deterministic; replicas must agree)

Commands: `Mint { cluster, actor, request, code, verifier, proposed_at_ms }` and
`Redeem { cluster, request, code, proof, key, proposed_at_ms }`. The minting host keeps the
plaintext in memory and shows it after commit; a retried mint cannot re-reveal it.
`EnrollmentState::new(cluster, first_free: NodeId)` and `apply(&mut self, &Command) -> Outcome`
work over `next_node_id`, `committed_ms` and
`BTreeMap<CodeId, CodeRecord{actor, request, verifier, expires_ms, consumed}>`, where `consumed` is
`Option<{request, key, node}>`. State derives `Clone + Eq + Debug`, with no credential in `Debug`.
A code is **usable** when it is unconsumed and `eff + 2*SKEW < expires_ms`.

- **Both commands, in order:** (1) a wrong cluster returns `WrongCluster`; (2) if
  `proposed + SKEW < committed`, return `ClockBehind` and change nothing; (3) otherwise
  `eff = max(committed, proposed)`, set `committed = eff`, and prune records with
  `expires + TOMBSTONE <= eff`.
- **Mint:** (1) a record with the same `(actor, request)` returns `Minted{replay: true}` with the
  same expiry if code and verifier match, else `RequestConflict`; (2) an existing `code_id` returns
  `CodeIdCollision`; (3) `MAX_PENDING` usable codes return `TooManyPending`; (4) otherwise insert
  with `expires_ms = eff + CODE_TTL_MS`.
- **Redeem:** (1) a missing code returns `UnknownCode`. (2) A failed `subtle` constant-time verifier
  comparison returns `BadProof` without consuming; the 256-bit secret means no attempt counter.
  (3) On a consumed code, the same request and key replay the grant (`replay: true`, same node, no
  allocation); the same request with another key returns `KeyConflict`; else `Consumed`. (4) An
  unusable code returns `Expired`. (5) A failed `checked_add` returns `NodeIdsExhausted` and changes
  nothing. (6) Otherwise consume atomically and return
  `Enrolled(EnrollmentGrant{cluster, node, key, request, replay: false})`.

Node IDs only increase and are never reused, even after pruning or a later member removal. Every
reject except `ClockBehind` commits only the time advance and pruning. **No certificate before
commit:** `EnrollmentGrant` has private fields and no public constructor; only `apply` creates one.
The gated signing slice must take a `&EnrollmentGrant` from an applied committed entry and sign
only for `grant.key()`. **Error privacy:** `Outcome::joiner_reply()` maps a grant to
`Enrolled{node}` and `ClockBehind` to `RetryLater`; everything else, mint outcomes included, is one
`Rejected`. `Reject` `Display` strings are static and contain no IDs.

## Clock-boundary contract (honest assumptions)

Assumption: every proposing leader's wall clock is within `SKEW_MS` of true time (NTP expected, not
verified by Arxcovo). Then the 2×`SKEW` margin means no code is accepted over 15 real minutes after
its mint instant, and each stays usable at least 15 min − 4×`SKEW` (13 min). Expiry is judged at the
proposal timestamp; a late-committed proposal is `ClockBehind` once later enrollment commands move
`committed` over `SKEW` past it. Rollback beyond `SKEW` fails closed. A forward jump ratchets
`committed`: codes expire early and correct leaders get `ClockBehind` until real time catches up
(a liveness, not safety, loss). Undetected clock error above `SKEW` can extend a code's life by the
excess. `committed_ms` covers enrollment commands only; claim nothing stronger.

## Required tests (`cluster::enrollment::tests`)

1. Round trip of generate/expose/parse. Oversize, wrong prefix or length, non-hex, non-ASCII and
   bad checksum each return `Malformed`; no `Display`/`Debug` contains the input.
2. Secret marker through mint/redeem/retry: `Debug` of state, outcomes, code, proof and secret never
   contains the marker or proof (raw or hex); only the verifier may appear in state.
3. Two codes give increasing node IDs, never reissued after pruning; an identical retry returns the
   same grant (`replay: true`) with `next_node_id` unchanged.
4. New request/same key, same request/new key, both new: exact rejects; only `committed_ms` moves.
5. Race: keys A and B redeem one code in both orders; one grant, to the first applier, counter +1.
6. A wrong proof is `BadProof` without consuming, then the correct one succeeds; wrong cluster rejected.
7. Boundary: a redeem at `expires − 2*SKEW − 1` succeeds; at `expires − 2*SKEW` it is `Expired`.
8. Clock: rollback within `SKEW` uses `committed`; beyond is `ClockBehind` with byte-identical state;
   forward jump: `Expired`, then correct clocks get `ClockBehind`; a delayed stale proposal applied
   after a later command is `ClockBehind`.
9. Mint replay, `RequestConflict`, `CodeIdCollision`, `TooManyPending`, freed slot after expiry;
   pruned code is `UnknownCode`; `NodeIdsExhausted` (near-`u64::MAX` `first_free`) changes nothing.
10. `joiner_reply` gives one `Rejected` for every reject except `ClockBehind`; one sequence applied
    to two clones gives equal states and outcomes.

## Acceptance for the Sol implementation

Required green on hosted gates: `cargo fmt --check`,
`cargo clippy -p arxd --all-targets --locked -- -D warnings`,
`cargo nextest run -p arxd --locked cluster::enrollment`. Review checks: diff limited to the listed
files, no new dependency, no clock or RNG call outside `fill`, no `Serialize`/`Display` on
credentials. Pure-core results only; three-node, storage, crash, mTLS and LAN/Tailscale behavior
remain open and **unverified on real hardware**.
