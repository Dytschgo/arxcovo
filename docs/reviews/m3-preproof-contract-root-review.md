# Anchored pre-proof contract: root review

Root Sol 6.1 medium, 2026-11-17. Reviewed Sol high brief e859ff16,
retained79f5fa01, against current anchor, state-store and TLS APIs. Static
inspection only; no new runtime evidence. Separate contract recheck precedes
source authorization. Main59a665c9 CI44114182696 is now green on attempt1.

## Standards and scope

The next slice is appropriately internal: bounded voter-list representation,
private no-client-auth bootstrap client and a real single-founder proof.
It does not create an enrollment listener or authorize public joining. Keep
all current peer verifier, code/apply/schema and service semantics unchanged.
StateStore.state() and membership() permit a coherent observation while one
store lock is held; no new production read task is necessary for this proof.
Helper visibility may widen only to the pki ancestor, never crate-wide signing
authority. No always-Ok registry, synthetic founder ticket or seeded joiner.

## Required clarification R1: wrapper owns byte admission

The brief bounds ciphertext in the test driver but must also bound the new
wrapper's own feed/drain path. A test-only bound is not an implementation fence.
Use separate checked cumulative incoming/outgoing counters, at most128KiB per
direction for the ENTIRE session (handshake, query, list and proof combined),
checked before accepting bytes or emitting beyond the limit. Refuse an oversized
feed before processing its bytes; overflow/quota/TLS/gate errors permanently
close the gate. No phase reset can replenish quota. No arbitrary application
writer or connection escape. Drain uses a caller-sized buffer, not an unbounded
internally growing Vec. The exact bounded rustls adapter mechanics may be chosen
by the builder within this contract; stop if they cannot enforce this fence.

List length is checked before copying/decoding (max641), proof is32 bytes, root
and leaf DER remain bounded4096, intermediates forbidden, outgoing rustls
application buffer4096. Add exact/over-budget, repeated small-feed aggregate,
counter overflow/failure-state and post-failure zero-proof controls. No sockets
or scheduling/deadline setting changes follow from these byte limits.

## Required clarification R2: same verifier and meaningful counterfactuals

Signature mutation M3 touches only the new bootstrap verifier TLS1.3 check;
the existing peer verifier remains byte-identical except minimal helper
visibility extraction, with equivalent bodies. The wrong-CertificateVerify
test must use a valid anchored leaf whose certificate matches the list, and
a resolver/signer that actually signs with another private key. Its intact
handshake must fail on possession, not an earlier invalid-chain/key-list check.
Under M3 the same gate must actually release proof and break the zero-proof
assertion. Server-side plaintext counts establish nonrelease; packet marker
searches and callback refusal alone do not. For M1, attacker-provided list must
contain its actual endpoint key so only digest binding blocks it; for M2, genuine
anchored list/digest must pass while only endpoint-key membership blocks it.
All synthetic adversary certificates are signed with a test-owned rcgen CA;
no production CA accessor or ticket bypass is added to make controls possible.

## Disposition

No product choice or owner reconfirmation is needed. R1/R2 are contract
clarifications required before build, not authority for a new runtime attempt.
Pure tests may execute locally; the new one-voter case is hosted-only on this
Windows host. Existing fixture28/sample8/founder13/TLS14/atomic-issuance1 stay
unchanged; separately classify anchored_preproof_single_voter on all OSes.
Existing stop rules and repair counters remain. Root will set a bounded
verification/correction budget only after independent contract recheck.
Public query read ownership, discovery, receipt/key custody, owner/control
identity, admission controller and production clock assumptions remain open.
M2 open / M3 unchecked; hardware **unverified on real hardware**.
