# Brief: M3 durable member key registry, then certificate issuance

Status: **proposed** (Opus 5.5 high, 2026-11-15) on `ad3d95e` (main `0ab5038`, PR #146); Sol review R1/R2 corrected on `2a9399b` (decisions 2, 5, 7), and the
coordinator rechecks before Sol builds. Nothing is built or run. No M3 acceptance, transport, listener, CA key, owner keystore or real-host claim. M2 stays open:
A7 is FINAL NO-GO, and the consumed A12 attempt with unconfirmed cleanup gets no retry. ADR 0005 gets one amendment (registry and issuer); its policy choices stay open.

## Why the registry comes before certificates

ADR 0005 requires checking the CA chain **and** the current cluster/node/key binding on every peer connection. The merged code has no durable binding to check:

- `EnrollmentGrant` exists only as an apply outcome. Its node/key pair sits in `CodeRecord.consumed`, and `apply` prunes that record once `expires_ms +
  TOMBSTONE_MS` passes, about 24.25 h after minting (`enrollment.rs` `retain`). After that, no state links node N to key K.
- Raft membership is `Membership<u64, BasicNode{addr}>` (`consensus.rs` codec). It carries no key, and nothing in Arxcovo's state records a removal.
- An identical Redeem retry returns `replay: true` with the grant for as long as the record lives. Once removal exists, a removed node could get a fresh issuable
  grant this way.

A certificate signed now would be the only proof of the binding, which amounts to an irrevocable approval. So this slice adds the committed binding the verifier
needs and closes the replay gap. It needs no new dependencies. The certificate slice comes immediately after it ([handoff](#ca-handoff-next-slice-settled-here)).

## Verified facts (primary sources, read only, not pinned)

- Lockfile at `ad3d95e`: **no rustls, rcgen, ring, aws-lc, webpki or x509 crate** (0 matches across 243 packages). rustls is not an existing dependency.
  `reqwest` dev-dependency is `default-features = false` (no TLS). Toolchain `1.95.0`.
- crates.io / static.crates.io on 2026-11-15. Stable releases: rustls `0.23.46` (sha256 `48e13bd8…8fdf`, MSRV 1.71), rcgen `0.14.10` (`8774e05a…8ad8`, MSRV 1.88),
  rustls-webpki `0.103.17` (`e3a0760a…d5`), x509-parser `0.18.1` (`d43b0f71…5202`), ring `0.17.14`. rustls `0.24.0-dev.*` is prerelease and excluded.
- rcgen `src/csr.rs`: `CertificateSigningRequestParams::from_der` (feature `x509-parser`) runs `verify_signature()`, which is proof of possession. It **copies the
  requested subject, SAN, KU, EKU and BasicConstraints (including CA)** into `.params`. Unknown extensions give `UnsupportedExtension`. Its `signed_by(&Issuer)` signs
  those requested params. `CertificateParams::signed_by(&impl PublicKeyData, &Issuer)` signs caller-built params. `PublicKeyData::subject_public_key_info()`
  re-encodes the SPKI DER. The rcgen `ring` feature enables `x509-parser?/verify`. rustls features: default is `aws_lc_rs` + `prefer-post-quantum`; `ring` and `std` are
  separate; `tls12` is opt-in. `WebPkiClientVerifier::builder_with_provider`, `no_client_auth()`, and the `ClientCertVerifier::verify_client_cert(end_entity,
  intermediates, now)` signature were confirmed in `src/webpki/client_verifier.rs` and `src/verify.rs`.

## Decisions (this slice)

1. **Registry state.** `EnrollmentState` gains `founded: bool` and `members: BTreeMap<NodeId, KeyDigest>`, with `MAX_MEMBERS = 16` (the same as the membership codec's
   node cap). Every member satisfies `node < next_node_id`, and keys are unique across members. Node IDs are never reused, so an allocated ID that is absent from
   `members` is revoked. No tombstone list is needed, and the state stays bounded.
2. **Commands** (row tags 4 and 5; 0, 1-2 and 3 are already Blank, Mint/Redeem and Membership). Both carry `cluster` and `proposed_at_ms` and go through the existing time, skew and prune prefix unchanged.
   - `Found { cluster, members: FoundMembers, proposed_at_ms }` registers the bootstrap set atomically. **Wire bound, not policy (R2).** `FoundMembers` wraps a
     private `Vec<(NodeId, KeyDigest)>`. Its only constructor, `FoundMembers::new(impl IntoIterator<Item = (NodeId, KeyDigest)>) -> Result<Self, FoundTooLarge>`,
     pulls at most `MAX_FOUND_WIRE + 1` items (`MAX_FOUND_WIRE = 64`) and refuses a 65th, so a huge or endless input never allocates past 65 pairs. It keeps the
     caller's order and checks nothing else: empty, unsorted, duplicate, unallocated and 17..=64-pair lists are representable on purpose. Every caller (tests, the
     future founder API, which is trusted but still technically bounded, and decode) goes through it. `as_slice()` is the only accessor.
   - Apply, after the existing prefix: (a) structure: 1..=16 entries, strictly ascending nodes, each `< next_node_id`, distinct keys, else `BadFounders`. This runs
     whether or not `founded` is set, so an empty or 17-pair Found after founding is `BadFounders`, never a vacuous replay. (b) If `founded`: `Founded{replay:true}`
     if every pair is a current member with the same key, else `AlreadyFounded`. (c) Otherwise insert all pairs, set `founded`, and return `Founded{replay:false}`.
     Only (c) changes state. Replay is **current-pair idempotence**: the original list is not stored, so `replay:true` does not prove equality with the bootstrap
     set (a subset, or a pair a later Redeem added, also replays). Nothing authorizes from this outcome. `next_node_id` is untouched. In production the founder
     initializes node 1 with `first_free = 2`, then proposes `Found[(1, own key)]`. The test fixture registers nodes 1, 2 and 3.
   - `Remove { cluster, node, proposed_at_ms }` returns `NotFounded` before founding, else `UnknownNode` if `node >= next_node_id` and `LastMember` if it would empty `members`. Otherwise it deletes the
     member and returns `Removed{replay:false}`, or `replay:true` if the node was already absent (idempotent for `ProposeError::Unknown` retries).
3. **Gated enrollment.** Mint and Redeem return `NotFounded` before founding. On a fresh Redeem, apply checks key uniqueness before consuming: a key held by any current
   member gives `KeyInUse` (A3: one key, two nodes). A full registry gives `MembersFull`. In both cases nothing is consumed, and the code stays redeemable with a
   different key. The consumption and the `members.insert(node, key)` happen in one apply step, so one SQLite transaction (I/O is unchanged).
4. **Replay closes for removed nodes.** A consumed-record replay returns the grant only if `members[node] == key`. Otherwise it returns `Revoked`, and
   `JoinerReply` maps that to `Rejected`. A removed node never gets an issuable grant again, and the code is still consumed.
5. **Peer authorization source: local, not fresh (R1).** `EnrollmentState::authorize_peer(cluster, node, key) -> Result<(), PeerRefusal{WrongCluster, NotMember,
   KeyMismatch}>` is pure, with static error text. It reads only the calling node's **local applied** state, never ReadIndex. Exact guarantee: a node refuses a
   removed binding once it has applied that Remove, from its log or an installed snapshot, and not before. There is **no time bound**: a lagging or isolated node
   can keep accepting a removed binding indefinitely. This is not linearizable revocation, and nothing here claims global revocation. So this helper alone must
   not authorize production control or execution traffic under isolation. Whether that traffic needs freshness, and how, is a separate future transport/control
   decision. Whatever it picks must not make Raft peer traffic need a quorum in order to form one (election, catch-up and bootstrap would deadlock).
   `ConsensusNode::local_authorize(...)` (existing blocking lock) is test-only. The issuer's fresh ReadIndex guards in the handoff are unchanged and unbuilt.
6. **Removal, rejoin and membership.** A registry Remove revokes authorization. It does **not** remove a node from Raft membership. Until the later workflow removes a
   voter, Raft safety rests on log up-to-dateness, and a revoked voter still counts in the quorum denominator: removing one of three voters means both remaining
   voters are needed. A rejoining wiped host redeems a fresh code, gets a fresh ID, and generates a fresh key every attempt. State does not forbid reusing a
   removed key, and that is not load-bearing: the binding is the (node, key) pair, and removed IDs never return. The later membership workflow (not here) adds a
   learner only for registry members, proves catch-up, promotes through one serialized intent that reconciles joint configurations after leader failure, and
   pairs Remove with the joint change. It keeps honest two-voter quorum. No voter cap and no host-2-learner policy is adopted.
   `consensus::registry_covers(&Members, &EnrollmentState) -> bool` (every membership node is a member) is a test helper for that workflow.
7. **Codec v2. v1 is refused, not migrated.** The state image keeps `ARXSM1\0\0`, and the version u16 becomes `2`. After the records, append `founded` (flag), `u8`
   count 0..=16, then ascending `node u64 BE ‖ key[32]`. Decode rejects: count > 16; unsorted or duplicate nodes; duplicate keys; member `>= next_node_id`;
   members or records present while unfounded, or an empty founded registry; a consumed record whose node is a member under a different key; trailing bytes; a non-canonical re-encode. A
   version-1 blob, on open or in `install`/`peek`, gives `StoreError::Unsupported` and leaves the store byte-identical. No production v1 state exists; every v1
   store is a test temp directory. The SQLite `user_version` stays 1 (schema unchanged).
   **Log rows (R2).** Tag 4: `cluster[16] ‖ proposed_at_ms u64 BE ‖ count u8 ‖ count × (node u64 BE ‖ key[32])` in stored order. Tag 5: `cluster[16] ‖ node u64 BE ‖
   proposed_at_ms u64 BE`. Sealed sizes: Found = 90 + 40n B (16 pairs 730 B, 64 pairs 2650 B; ClusterId stays16bytes), Remove = 97 B. Because every
   `Command` is bounded by type, `encode_command(&Command, &mut Vec<u8>)` and `encode_row(..) -> Vec<u8>` stay infallible with unchanged signatures. The one count
   cast is on a length the constructor proved ≤ 64. `log_store.rs` adds `const _: () = assert!(90 + 40 * MAX_FOUND_WIRE <= MAX_ROW);`. Decode reads the count and
   returns `Corrupt` if it is > 64 before reserving anything, reserves ≤ 64, reads the pairs, and builds through `FoundMembers::new`. It accepts semantically
   invalid lists, so committed `BadFounders` entries stay decodable on restart, catch-up and snapshot rebuild. Checksum, `MAX_ROW` read bound and canonical
   re-encode checks are unchanged. An over-bound Found cannot be represented, so it never reaches propose or append. Append's `TooLarge` check and the adapter's
   callback-Err path stay as they are (defense in depth, not the rejection path). A semantic rejection is an ordinary committed outcome, never a storage error.
   The snapshot member count ≤ 16 is a separate decode invariant. Worst-case image: 64 records + 16 members + maximal membership ≈ 13.7 KB, under the 16384
   `LIMIT` (tested), unchanged because only a valid Found writes state. An old binary rejects tags 4 and 5 as `Corrupt`. Mixed-version clusters are unsupported
   until rolling update work.
8. **Not proof of commit.** `EnrollmentGrant` and `authorize_peer` remain values derived from apply. No `CommittedGrant` or quorum-certificate type is invented.
   Commit evidence is only the leader's `ClientWriteResponse` or a ReadIndex read, which is the handoff contract below.

## Files (Sol high; Cargo.toml and Cargo.lock unchanged)

`cluster/enrollment.rs` (commands, outcomes, gates) and a new `cluster/enrollment/registry.rs` (≤ 150 lines: members, `FoundMembers`/`FoundTooLarge`/`MAX_FOUND_WIRE`,
Found/Remove validation, `authorize_peer`). `enrollment/codec.rs` (tags 4 and 5), `log_store.rs` (tag match `1 | 2 | 4 | 5`, const assertion), `state_store.rs`
(version 2, v1 `Unsupported`), and `consensus.rs` (`local_authorize`, `registry_covers`). No signature changes to `encode_command`, `decode_command`, `encode_row`,
`decode_row`, `append`, `propose` or the adapter, and no OpenRaft or store semantics are relaxed.
Tests go in the existing `tests.rs` files and `consensus/three_node.rs`, plus `docs/evidence/m3-member-registry.md`. Existing fixtures that mint/redeem gain a `Found`
first; update them, never weaken them. The coordinator keeps PROGRESS.

## Tests (each behavior fails without its fix; three-node ≤ 45 s under `timeout`)

- **Pure.** Found validation table, replay and `AlreadyFounded`. `NotFounded` for Mint, Redeem and Remove. Redeem inserts a member. `KeyInUse` against a founder key and
  another joiner's key, where a later Redeem of the same code with a fresh key succeeds. `MembersFull`. Remove gives `UnknownNode`, `LastMember`, success and
  replay. A replay after Remove gives `Revoked` and no grant. `authorize_peer` matrix: wrong cluster, unallocated, removed, wrong key, current. Registry outlives
  prune: after commands past `expires_ms + TOMBSTONE_MS`, the record is gone, `authorize_peer` still passes, and a replay gives `UnknownCode`.
- **Found wire bound.** `FoundMembers::new` accepts 0, 1, 16, 17 and 64 pairs and refuses 65. A counting iterator of 10^6 items is pulled at most 65 times. Each
  accepted size round-trips `encode_row`/`decode_row` byte-identically, with the full-row SHA-256 recomputed (not length only); 64 pairs is 2650 B. Sealed rows
  with count 65, count 64 carrying 63 pairs, or trailing bytes give `Corrupt`; no truncation panics.
- **Codec and store.** v2 round trip at maximum size. Every rejection in decision 7, with no panic at any truncation. v1 blob gives `Unsupported` with the file
  byte-identical. Tags 4 and 5 round trip, and tag 6 gives `Corrupt`. Kill/reopen and `install` carry the registry. The privacy marker scan covers the new files.
- **Engine (actual OpenRaft, `mem_net`).** T4: initialize 3 voters, `Found[1,2,3]`, Mint/Redeem joins node 4, and `local_authorize(4, K)` passes on all three. Stop a
  follower, then commit `Remove(4)` and enough entries to purge (T3 params). The restarted follower catches up via snapshot and refuses node 4, while the founders
  still pass. Advance past prune: node 4's record is gone, and the founders and registry state match on every node. `registry_covers` holds for the initial
  membership. Semantic Found on the same engine: before founding, 0-, 17- and 64-pair, unsorted, duplicate-key and unallocated Founds each commit with
  `BadFounders` with registry/node counter unchanged (only the common time/prune prefix may advance). After the valid Found, empty and 17-pair Founds give `BadFounders`, a subset gives `replay:true`, and a conflicting key gives
  `AlreadyFounded`. Kill/reopen and a snapshot-installed follower reach identical state bytes on every node.

## Operational advisories: not fixed here, assigned

- Advisory 1: `Weak` expiry is not file-close completion. It must be fixed (a release signal after the store fields drop) before the first non-test in-process
  restart caller, at the latest in production service wiring.
- Advisory 2: the ReadIndex 1500 ms `wait_timeout` does not bound apply catch-up. The **certificate slice** must fix it, using `get_read_linearizer` +
  `try_await_ready(Some(deadline))` or an outer deadline with `Unavailable`, because its signer guards are the first non-test ReadIndex callers.
- Nothing in this slice is production-ready.

## CA handoff (next slice, settled here)

- **Issuer role.** Every **voter** that holds the sealed CA key may issue. Learners never hold it. The key never enters Raft entries, forwards, state, snapshots,
  logs, evidence or MCP. Reasons: issuance already needs quorum for Redeem, and a single fixed issuer makes one host a permanent single point of failure for joins
  and renewals. Losing the founder would then strand the cluster. KICKOFF promises continued writes, not join availability. This choice keeps joins available
  whenever writes are, at no owner cost. Exposure: a stolen CA key alone cannot impersonate a member, because verifiers also require the committed (node, key)
  binding. A compromised member already holds enrollment authority through Raft. Distributing the key to newly promoted voters (over authenticated mTLS, in the
  membership workflow) and sealing it under the daemon identity, away from the build account, are later work and **unverified on real hardware**.
- **Signing scope.** Only the leader that committed the Redeem signs, from its `ClientWriteResponse`, including a `replay:true` grant. Before signing, a fresh
  ReadIndex read requires the consumed record, `authorize_peer(grant)` and `check_issuable(signer_now)`. The same checks run again before the certificate is
  released. Nothing is cached. A `Remove` committed between the two checks refuses release.
- **CSR.** Bound the CSR at 4096 B. Run `from_der` for proof of possession, then use **only `.public_key`**, with algorithm ∈ {ECDSA P-256 SHA-256, Ed25519}. The digest is
  `KeyDigest::of_spki(public_key.subject_public_key_info())`. The enrollment ingress computes the same value before it proposes Redeem, and the signer re-derives it
  from the same bytes and requires `== grant.key()`. Never call `CertificateSigningRequestParams::signed_by`. Params are built fresh: `IsCa::NoCa`, KU
  digitalSignature, EKU serverAuth+clientAuth, exactly one SAN URI `arxcovo://cluster/<32 hex>/node/<id>`, a random 16-byte serial, and a bounded validity. The
  lifetime value and rotation stay open. Re-signing for an identical retry inside the window is harmless, because the binding is the authority.
- **Pins (candidates).** `rcgen = { version = "=0.14.10", default-features = false, features = ["crypto", "ring", "x509-parser"] }` and `rustls = { version = "=0.23.46",
  default-features = false, features = ["ring", "std"] }` (TLS 1.3 only). Use ring rather than aws-lc for CI time. The lockfile may only add packages. x509-parser is
  used only through rcgen, plus the verifier's SAN/SPKI extraction.
- **Verifier.** A wrapper runs `WebPkiClientVerifier`/`WebPkiServerVerifier` with only the cluster root and no system roots, then extracts the single SAN and SPKI
  digest (bounded cert ≤ 4096 B) and calls local `authorize_peer`. The Raft sender ID must equal the certificate node. Tests use an **ephemeral in-memory test CA**
  and in-memory TLS 1.3 handshakes, with no sockets. Negative cases: foreign CA, wrong key, bad CSR signature, CSR asking for CA or extra SANs (the issued cert has
  neither), CSR key ≠ grant (substitution), stale or expired grant, Remove between the two guards, revoked after issue, sender mismatch, oversized input,
  unsupported algorithm.
- **Ingress boundary (transport slice).** The no-client-cert enrollment `ServerConfig` (`no_client_auth`) accepts only {RedeemProof derived from a pasted code, CSR}.
  It never accepts verifiers and exposes no peer RPC or control operation. Peer and control configs make client auth mandatory, with exhaustive deny-by-default
  tests. Mint stays fresh-in-memory-only (A2). Owner codes never reach MCP, logs or evidence.
- **Clocks.** The signer uses its own wall clock under the existing `SKEW_MS` assumption (all hosts within 30 s). This is fail-closed through
  `SignerClockBehind`/`GrantExpired`, but rollback and slow-clock failover are not proven. No real-time expiry guarantee is claimed.

Full M3 acceptance needs three processes or hosts and an agent build from A on B, and stays unmet after both slices.
