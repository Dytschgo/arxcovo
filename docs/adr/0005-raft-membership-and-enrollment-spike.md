# 0005: Durable Raft membership and enrollment

Status: proposed from the early membership spike; no cluster implementation or acceptance has run. LAN/Tailscale behavior is **unverified on real hardware**.

Keep the settled OpenRaft, embedded storage and cluster-CA/mTLS architecture. The research inspected OpenRaft 0.10.0, rustls 0.23.46 and rcgen 0.14.10. These are researched candidates, not implemented dependency pins. Storage-backend selection and its Windows durability behavior still need a bounded build/crash probe.

## Invariants for implementation

- Initialize a new cluster once. Allocate non-reused node identities through committed cluster state, scoped to an immutable cluster identity. Rejoining a wiped host gets a fresh identity; an old certificate or node ID must not resurrect membership.
- Add a learner, prove catch-up, then promote through guarded membership changes. Serialize membership intents and reconcile committed joint configurations after leader failure. Do not use address replacement as a shortcut for identity replacement.
- Persist votes before returning, acknowledge log appends only after durable storage, and atomically persist state-machine changes, applied log ID, membership, consumed-code state and deduplication results. Snapshots must include those records and install atomically. Verify committed-index/recovery semantics before serving reads after restart.
- Replicated commands carry stable request IDs. Bind deduplication to authenticated identity and canonical command contents, including the joining public key, so a reused ID cannot retrieve another request's result. Forwarding/retries keep that identity and ID, with bounded deadlines and a retryable no-leader result.
- Redeem a 15-minute one-time code in one state-machine transaction before signing a certificate. Authenticate the endpoint by the CA fingerprint carried through the owner-pasted code. Store only a hash of its random secret. Unknown/wrong secrets must not disclose usable enrollment information. An identical authorized retry may return the same enrollment; a new request/key cannot reuse the code.
- Issue only for the committed public-key binding. Validate both the CA chain and the current cluster/node/key binding on every peer connection; reject revoked/nonmembers and Raft sender IDs that differ from the certificate identity. Implement the real pre-vote RPC if pre-vote is enabled.
- A no-client-certificate enrollment path must not expose any peer RPC or owner control operation. Use a separate transport boundary or exhaustive deny-by-default authorization tests. CA-chain validation alone is insufficient authorization.
- Keep plaintext join codes out of MCP responses, logs, replicated state and snapshots. The owner-facing minting UI carries the code for transfer. CA private keys must be sealed under a daemon identity inaccessible to the account used for arbitrary agent host builds. Sharing that account's keychain would not provide that isolation.

## Quorum and unresolved choices

Three voting hosts can continue writes with one offline; two voters require both for writes. Maintenance must check remaining quorum. Do not promise the same availability for one/two hosts or introduce a forced-reset path that risks split brain.

The research proposed keeping host two as a learner until host three joins and capping voters at five. Neither is adopted here: the former changes acknowledged-write durability and failure behavior, while the cap is not a settled product requirement. Choose membership policy from explicit durability/availability evidence during milestone 3.

A replicated `max(previous_time, leader_wall_clock)` prevents a decreasing stored timestamp; it does not alone prove real 15-minute expiry after clock rollback or a slow-clock failover. Define trusted clock/skew assumptions and fail-closed behavior, then test rollback, forward jumps and failover. Do not claim a real-time guarantee from that maximum alone.

CA-key distribution across voters versus an enrollment-signing role, key rotation/revocation, certificate lifetime, retry limits and embedded storage remain design decisions. No new owner approval workflow is introduced. Resolve them through the security design before coding, without storing a private CA key in the Raft log.

## Required bounded probes

1. Three persistent nodes: acknowledge writes, stop either a follower or leader, continue writes with two, restart and verify all acknowledged state converges. Verify linearizable reads and explicit no-quorum failures.
2. Reject expired/reused codes; race two public keys against one code; prove exactly one enrollment, deterministic authorized retry and no certificate before commit.
3. Crash at append/apply/snapshot/membership boundaries; consumed codes, identities, deduplication and membership survive restart without stale reads or duplicate acknowledged effects.
4. Reject foreign CAs, wrong enrolled keys, sender/certificate ID mismatches, unauthenticated RPC and removed-node replay. Exercise interrupted joint changes and competing promotions.
5. Inspect persisted state and bounded exported evidence for test secret markers. Test daemon/build-account key access separation independently from encryption-at-rest claims.

Sources: [OpenRaft 0.10](https://docs.rs/openraft/0.10.0/openraft/), [membership source](https://docs.rs/crate/openraft/0.10.0/source/src/docs/cluster_control/dynamic-membership.md), [rustls verifier](https://docs.rs/rustls/0.23.46/rustls/server/struct.WebPkiClientVerifier.html), [rcgen CSR signing](https://docs.rs/rcgen/0.14.10/rcgen/struct.CertificateSigningRequestParams.html).
