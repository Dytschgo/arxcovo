# Durable enrollment slice: independent Sol review

Target Opus33410cf retainedc736cf7, ADR0005 narrow verifier invariant and140-line brief. Standards/Spec/Security PASS for this bounded storage probe, with the coordinator clarifications below applied before implementation. No build/test/runtime action ran in this review.

SQLite uses the existing pin; one transaction swaps canonical full state and applied/membership metadata only after commit. Poison-on-SQLite-error, exclusive private lock, sidecar/path checks, bounded pages/blob/codec and process-kill before/after commit tests are appropriate. This is local state-machine crash consistency, not vote/log durability, quorum commit or power-loss proof. All hosted OSes must pass the genuine journal probes before the provisional backend claim.

Verifier reduction removes bearer proofs from the proposed committed record and preserves the constant-time consistency comparison. Future ingress must accept only proof, never a client verifier; trusted Raft members can already mint/commit. Fresh-mint proposal/retry rules and finite tombstone dedup are explicit future-adapter constraints, not current network enforcement. The A1 wrong-proof/expired test belongs in this change.

Coordinator clarified fail-closed codec invariants already implied by reachable core state: expiry no later than committed+TTL, no duplicate owner/request pair, and membership position consistent with applied term/index. Snapshot install requires a higher index and nondecreasing term. Tests must recompute corruption checksums to reach semantic validators. Removed an unsupported calendar-based two-week dependency gate; future adapter pinning depends on actual source/API review, with no OpenRaft dependency in this slice.

Architecture remains ADR0005's settled OpenRaft/embedded storage/cluster CA/mTLS. No endpoint, CA key, native host operation or M2 closure follows. Implement exactly the listed paths; independent Opus source review and hosted green gates remain required before merge. M2 hardware gaps remain open; A7 FINAL NO-GO and consumed A12 no-retry stay closed.
