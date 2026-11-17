# 0005: Durable Raft membership and enrollment

Status: proposed from the early membership spike; no cluster implementation or acceptance has run. LAN/Tailscale behavior is **unverified on real hardware**.

Keep the settled OpenRaft, embedded storage and cluster-CA/mTLS architecture. The research inspected OpenRaft 0.10.0, rustls 0.23.46 and rcgen 0.14.10. These are researched candidates, not implemented dependency pins. Storage-backend selection and its Windows durability behavior still need a bounded build/crash probe.

## Invariants for implementation

- Initialize a new cluster once. Allocate non-reused node identities through committed cluster state, scoped to an immutable cluster identity. Rejoining a wiped host gets a fresh identity; an old certificate or node ID must not resurrect membership.
- Add a learner, prove catch-up, then promote through guarded membership changes. Serialize membership intents and reconcile committed joint configurations after leader failure. Do not use address replacement as a shortcut for identity replacement.
- Persist votes before returning, acknowledge log appends only after durable storage, and atomically persist state-machine changes, applied log ID, membership, consumed-code state and deduplication results. Snapshots must include those records and install atomically. Verify committed-index/recovery semantics before serving reads after restart.
- Before the consensus engine starts, and while no other reader or writer exists, check the durable applied position against the log by index with exact log-ID equality. Startup refuses when the purged index is past applied, when the log reaches the applied index but its purged or retained ID there differs or is missing, or when a log ending below the applied index is ordered above it. Only a log that ends below the applied index and is ordered below it may be repaired, by purging up to applied. OpenRaft 0.10.0's startup helper does not enforce all of this, so Arxcovo must.
- Replicated commands carry stable request IDs. Bind deduplication to authenticated identity and canonical command contents, including the joining public key, so a reused ID cannot retrieve another request's result. Forwarding/retries keep that identity and ID, with bounded deadlines and a retryable no-leader result.
- Redeem a 15-minute one-time code in one state-machine transaction before signing a certificate. Authenticate the endpoint by the CA fingerprint carried through the owner-pasted code. Store only a hash of its random secret. Unknown/wrong secrets must not disclose usable enrollment information. An identical authorized retry may return the same enrollment; a new request/key cannot reuse the code.
- Issue only for the committed public-key binding. Validate both the CA chain and the current cluster/node/key binding on every peer connection; reject revoked/nonmembers and Raft sender IDs that differ from the certificate identity. Implement the real pre-vote RPC if pre-vote is enabled.
- A no-client-certificate enrollment path must not expose any peer RPC or owner control operation. Use a separate transport boundary or exhaustive deny-by-default authorization tests. CA-chain validation alone is insufficient authorization.
- Keep plaintext join codes out of MCP responses, logs, replicated state and snapshots. The owner-facing minting UI carries the code for transfer. CA private keys must be sealed under a daemon identity inaccessible to the account used for arbitrary agent host builds. Sharing that account's keychain would not provide that isolation.
- The redeem proof is a bearer credential. The node that receives it reduces it to the stored verifier form before proposing, then drops the proof. Raft entries, forwarded requests, state, snapshots and evidence carry only verifiers, so a copied log or snapshot cannot redeem a live code through the unauthenticated enrollment path. The state machine's verifier comparison is a consistency check under trusted-member Raft authority, not authentication against members. Mint entries come only from a code freshly generated in memory on the proposing host; a decoded Mint is never re-proposed. See [the durable state brief](../briefs/m3-durable-enrollment-next-slice.md).

## Quorum and unresolved choices

Three voting hosts can continue writes with one offline; two voters require both for writes. Maintenance must check remaining quorum. Do not promise the same availability for one/two hosts or introduce a forced-reset path that risks split brain.

The research proposed keeping host two as a learner until host three joins and capping voters at five. Neither is adopted here: the former changes acknowledged-write durability and failure behavior, while the cap is not a settled product requirement. Choose membership policy from explicit durability/availability evidence during milestone 3.

A replicated `max(previous_time, leader_wall_clock)` prevents a decreasing stored timestamp; it does not alone prove real 15-minute expiry after clock rollback or a slow-clock failover. Define trusted clock/skew assumptions and fail-closed behavior, then test rollback, forward jumps and failover. Do not claim a real-time guarantee from that maximum alone.

Key rotation, certificate lifetime and retry limits remain open design decisions. No new owner approval workflow is introduced. Resolve them through the security design before coding, and never store a private CA key in the Raft log.

### Amendment 2026-11-15: member registry and issuer role

- **Authorization source.** Committed state holds a bounded member registry (node → SPKI digest), written atomically with code consumption. Founders are registered once. Revocation means removal from the registry. Node IDs are never reused, so an allocated ID that is missing from the registry is revoked, and an expired consumed record never ends a member's binding. Peer verification checks the CA chain plus this registry on the local applied state. That check refuses a removed binding only after the local node applies the Remove. It has no time bound under lag or isolation and is not linearizable revocation. Whether production control and execution traffic needs freshness is a separate open decision, and the answer must not make Raft peer traffic need a quorum in order to form one. A certificate is never the authority on its own. A replayed Redeem for a removed node returns no grant. One key cannot be bound to two current members. Removing a node from the registry does not change Raft membership. The later membership workflow pairs the two. See [the registry brief](../briefs/m3-enrollment-certificate-next-slice.md).
- **Issuer role.** Every voter that holds the sealed CA key may issue certificates, and learners never hold it. This keeps joins available whenever writes are, and avoids making one host a single point of failure for joins and renewals. A stolen CA key alone cannot impersonate a member, because verifiers also require the committed binding. Issuance uses only the grant committed on the leader, with fresh guards before signing and again before the certificate is released. It signs only the CSR's proven public key, and the signer builds the certificate parameters itself. CA key transfer to new voters and sealing under the daemon identity are still future work and **unverified on real hardware**.

## Required bounded probes

1. Three persistent nodes: acknowledge writes, stop either a follower or leader, continue writes with two, restart and verify all acknowledged state converges. Verify linearizable reads and explicit no-quorum failures.
2. Reject expired/reused codes; race two public keys against one code; prove exactly one enrollment, deterministic authorized retry and no certificate before commit.
3. Crash at append/apply/snapshot/membership boundaries; consumed codes, identities, deduplication and membership survive restart without stale reads or duplicate acknowledged effects.
4. Reject foreign CAs, wrong enrolled keys, sender/certificate ID mismatches, unauthenticated RPC and removed-node replay. Exercise interrupted joint changes and competing promotions.
5. Inspect persisted state and bounded exported evidence for test secret markers. Test daemon/build-account key access separation independently from encryption-at-rest claims.

Sources: [OpenRaft 0.10](https://docs.rs/openraft/0.10.0/openraft/), [membership source](https://docs.rs/crate/openraft/0.10.0/source/src/docs/cluster_control/dynamic-membership.md), [rustls verifier](https://docs.rs/rustls/0.23.46/rustls/server/struct.WebPkiClientVerifier.html), [rcgen CSR signing](https://docs.rs/rcgen/0.14.10/rcgen/struct.CertificateSigningRequestParams.html).

### Owner decisions 2026-11-17: join trust, admission progress and initial voters

The owner selected all three recommended options after the cross-provider
membership decision review. These are adopted product/security requirements;
production joining and promotion are not implemented or accepted by this record.
The detailed proposal and remaining technical blockers are in
[the membership decisions brief](../briefs/m3-membership-owner-decisions.md).

- **Join trust.** The pasted join code fingerprints the current voting hosts and
  their registered keys. The joiner checks that fingerprint and the endpoint's
  possession of an anchored key before sending its redeem proof. A stolen CA
  key alone must not impersonate the cluster. The proposed encoding adds 32
  binary bytes, or 64 hexadecimal characters (206 to 270 text characters).
  Voting-host changes can require a fresh code. Code consumption and refusal
  during races must be reported honestly; no unconditional unspent guarantee.
- **Admission and progress.** Self-admission uses a 5-second response budget,
  then exposes catch-up progress separately. A timeout can mean Unknown and
  must be resolved by checking/replaying the same operation, not declaring
  failure or adding a second membership entry. No progress for 60 seconds
  reports Stalled. Nothing is removed automatically.
- **Initial voting policy.** Host 2 remains a non-voting learner until host 3
  catches up; then the leader automatically promotes both into three voters
  through a guarded membership change. With one or two hosts, writes depend
  on host 1. Losing its disk can lose acknowledged writes not yet replicated.
  This availability/durability tradeoff is accepted. There is no new owner
  approval step during joining or promotion, and no voter cap is adopted here.

The earlier paragraph leaving the host-2 policy unresolved is historical and
superseded by this amendment. Production L2 remains blocked on address discovery,
removal authorization, apply-time anchor fencing and interrupted joint-change
recovery. The enrollment endpoint's pre-proof query/deny-by-default boundary,
read ownership under cancellation, durable receipt and restart behavior also
need a concrete reviewed implementation contract and negative controls.
The suggested implementation details in the brief are not automatically adopted
by these three selections. No timeout, retry, capacity, workflow, protection or
quarantine changes are authorized by this record. M2 remains open, M3 unchecked;
production transport and hardware paths are **unverified on real hardware**.

### Technical amendment 2026-11-17: atomic join-anchor kernel

Root accepts the internal implementation contract in
[the atomic-anchor brief](../briefs/m3-atomic-join-anchor-fence.md), with
[root corrections](../reviews/m3-atomic-join-anchor-root-review.md) and the
[independent Sol recheck](../reviews/m3-atomic-join-anchor-contract-sol-recheck.md).
This authorizes the bounded local source slice, not public enrollment or release.

- First redemption compares the code-bound voter-key anchor against the
  canonical membership and registry immediately before that committed entry.
  Missing, joint or unbound voting state refuses. AnchorChanged leaves enrollment
  state untouched, while the applied log position advances. Persistence precedes
  any returned grant. Both existing fresh signing guards remain required.
- Exact authorized replay preserves the original commit, expiry and original
  effective time, even after voting-state changes. The grant's existing
  applied_ms remains the fresh operation time for signer rollback protection.
  Revocation never recreates a binding. IssuedLeaf.identity_commit reports the
  original redemption position, or the founding position for a founder leaf.
- Codes use fixed arxj2 (270 characters), including a 32-byte voter-key anchor
  bound into proof-v2. Command tag6 replaces legacy Redeem tag2. Snapshot version3,
  state SQLite version2, log SQLite/metadata version2 and ARXLE2 are explicit
  format breaks. Legacy formats refuse Unsupported; no migration, reset,
  deletion, mixed-version cluster or rolling upgrade is authorized. Preserve old
  files. A requirement to operate retained deployed data stops this slice for
  a separate migration design.
- The 16 KiB snapshot limit stays. New Mint reserves for future consumption,
  maximum registry and membership growth: 4866 + (retained records + 1)*209
  must not exceed16384. Effective retained-code capacity becomes55; existing
  MAX_RECORDS64/MAX_PENDING8/MAX_MEMBERS16 remain upper bounds. This is an
  intentional earlier StateFull refusal, after pruning and exact Mint dedup.
  Snapshot decode enforces the same invariant. SQL errors stay unknown/poisoned.
- Pure and durable tests must preserve prior semantic assertions through stated
  setup/format adaptations. Canonical membership max3605 is distinct from its
  4096-byte envelope. The new one-voter issuance test is required on all OSes;
  existing fixture28/sample8/founder13/TLS14 classifications remain unchanged.

No discovery, pre-proof authenticated endpoint, durable joiner custody,
membership controller, transport/control identity or production clock policy
is accepted here. Root authorizes one local verification round, with at most two
mechanical compile/import/lint corrections before the first behavior check.
A natural semantic/static-suite failure stops for disposition without rerun.
The four designated pure/durable mutants run once each and restore baseline
bytes. No local Windows engine scenario is authorized. Publication and merge
still require independent exact-head source review, all five normal gates,
hosted Windows at most660s and fresh green exact main. M2 stays open and M3
unchecked; hardware paths remain **unverified on real hardware**.

### Technical amendment 2026-11-17: internal pre-proof endpoint gate

Root accepts the [pre-proof brief](../briefs/m3-next-anchored-join-security.md),
[root byte-admission and signature clarifications](../reviews/m3-preproof-contract-root-review.md)
and [independent contract recheck](../reviews/m3-preproof-contract-sol-recheck.md)
for a bounded local implementation. A canonical voter list and a private TLS1.3
client demonstrate code-bound endpoint-key possession before proof release.
The initial chain-only bootstrap check grants no member or owner authority.
It is separate from the existing peer verifier and exposes no public listener,
generic application writer or raw connection. This is not authorization to
deploy a no-client-certificate endpoint or production joining.

The wrapper enforces separate cumulative128KiB input/output quotas over the
whole session, bounded feed/drain and terminal failures. Released state permits
bounded ciphertext drain but no second query/anchor/proof operation. Fatal state
permits no later operation, including draining queued proof. Distinguish the
fixed32-byte query from the32-byte proof when counting received plaintext.
Application buffer4096 is not a handshake limit. Partial proof-write failure
is fatal; it cannot retry or expose its queued bytes through a later drain.
Tests must prove same-CA wrong-key and wrong-CertificateVerify refusals with
zero proof received, plus their designated mutation counterfactuals.

No code length, schema, apply/signing semantics, production read ownership,
timeouts, transport, custody or permission policy changes. The new single-voter
proof is required on all OS, hosted-only on the local Windows host. Existing
quarantines and inventories remain. All normal publication/merge gates apply.
