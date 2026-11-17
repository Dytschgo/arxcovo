# Atomic join-anchor contract: root review and corrections

Root Sol6.1 medium, 2026-11-17. Separate-author review of Sol high proposal
2462bf33 against enrollment, code/codec, StateStore, log storage, issuance,
founder and quarantine-guard source. No source/build/test was run for this review.
The following corrections are normative; independent recheck precedes build.

## Standards

The integrated apply/signing boundary is appropriate. Shared membership-v1 stays
unchanged/std-only; this is a new behavior/schema slice, not another extraction.
Version changes and earlier StateFull refusals need an additive ADR0005 record
before implementation, not a claim of wire/backward compatibility. Preserve old
files and refuse old formats; never migrate, delete or reset a directory here.

## Spec and security

**R1: fresh signer time must remain fresh (blocking in the original proposal).**
The old Grant.applied_ms is the effective time of THIS operation, even a replay.
check_issuable uses it to refuse a signer clock behind the current proposal.
Replacing it with the original redemption time weakens that existing fence.
Persist original_applied_ms separately in Consumption and expose it separately
on Grant for provenance/check_grant. Keep Grant.applied_ms and check_issuable's
fresh-time meaning unchanged. Match retained original time to the record and
require original <= fresh grant time <= current committed_ms. Never compare
fresh replay time for equality with the original stored time.

Required pure regression: redeem at1000ms; later exact replay at60000ms. Original
time remains1000. Before checking the fresh field value, assert that signing at
20000ms returns SignerClockBehind (20000+30000 <60000). An implementation using
the old origin time would wrongly allow it. A fourth once-only pure mutant sets
fresh replay applied_ms to original_applied_ms and must fail that exact refusal
assertion, with baseline restored. This changes no production clock constant or
unsettled real-time expiry guarantee.

**R2: guard the new one-voter test (blocking scope omission).**
assert_issuance_quarantine currently permits exactly11 fixture cases. Adding an
ungated async test fails that guard unless explicitly classified. Allowlist the
guard file to add ISSUANCE_REQUIRED with exactly
anchored_single_voter_receipt_replay_and_revocation, and pass it through the
existing required= argument. Keep ISSUANCE11/reasons and all old inventories
unchanged. Mutants must reject its omission/rename/ignore/Unix gating, old fixture
ungating and inventory drift. Check its body uses one ConsensusNode::start,
founder_init, found and enroll, and no Cluster helper (exact word/type match,
not a substring ban on ClusterCa). Inspect helpers too; tokens are not proof.
The new case runs on ALL OS; fixture28/sample8/founder13/TLS14 remain unchanged.

**R3: full origin sanity and a clear response name.**
At the adapter require origin.index<=L.index AND origin.term<=L.term, with exact
full equality when indices match. Malformed origins map to existing
IssueError::Rejected(Reject::UnknownCode); direct tests must reach the origin
check with otherwise valid own-response fields. Keep current response L leader
for issuer guards. Return identity_commit in IssuedLeaf: E for enrollment and
the committed founding position for found(). Reconstruct the founder position
from existing leader/index; grant no new signing constructor or authority.

The new single-voter test has a cooperative40s entry deadline including5s cleanup
reserve; existing per-operation/service caps remain5s. Remove router handles,
drop/resolve caller futures and verify clean shutdown before any passing result.
Reuse the reviewed startup limitation: pending blocking startup can fail with
unresolved cleanup and runtime teardown beyond the cooperative deadline. No new
reaper, runtime timing setting, detached cleanup or claimed graceful release.

## Other reviewed decisions and arithmetic

Proof-v2 binds anchor into the existing SHA256 secret-proof derivation. A changed
anchor with repaired checksum therefore changes the stored verifier commitment;
this is stronger than an unbound request field. Keep uniform current applied
membership/registry authoritative at E-1, including direct trusted proposals.
Exact replay authenticates retained proof/request/key/anchor and keeps E, expiry
and original time; no new-consumption fence after voter drift. Revoked stays so.

The proposed state reservation arithmetic is correct: fixed4866 plus209 per
record gives16361 at55 and16570 at56, under/over16384 respectively. Root accepts
this conservative technical refusal policy: effective retained-code capacity55,
with existing MAX_RECORDS64/MAX_PENDING8/MAX_MEMBERS16 still upper bounds. Record
the changed behavior explicitly; do not say capacity is unchanged. Reserve after
normal pruning and only for a NEW Mint; exact Mint replay must still work at the
boundary. Reject snapshots violating the same invariant. No byte-limit increase.

No production ingress, key custody, network bootstrap, owner-control identity or
clock policy is closed here. Existing native/Windows repair stops remain.
Result: original R1/R2 require the stated corrections; revised contract needs
independent recheck before source authorization. M2open/M3unchecked; hardware
paths remain **unverified on real hardware**.
