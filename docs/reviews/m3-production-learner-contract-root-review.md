# L2 proposal: coordinator review

Root Sol 6.1 medium, 2026-11-17. Target: Sol high proposal 9b3cfeab,
based on owner decision record 50100a0. This is a separate-author Sol review
under the owner's revised routing, not cross-provider independence.

## Standards

PASS for a design proposal. The delivered change is one documentation file.
Production boundaries, unsupported guarantees and static-only evidence are
explicit. The implementation slices require their own bounded briefs.

## Spec

PASS for planning; not a build authorization. Owner choices remain unchanged:
voter fingerprints, bounded admission followed by progress, and host 2 remaining
a learner until host 3 catches up. The proposal correctly distinguishes host
arrival order from non-reused node IDs.

The endpoint hints make the whole code longer than the earlier 270-character
anchor-only estimate: up to 792 characters. This is an additional discovery
encoding proposal, not a claim that the owner approved its exact schema.
Before that slice, pin the parser and clarify that rejecting broadcast means
the limited broadcast address; detecting every subnet-directed broadcast would
require interface/network information the proposed parser does not have.

The next test-only slice needs its own deterministic interruption seam. Merely
constructing a joint entry through append_membership would not prove that a
cancelled two-step change leaves it behind. The brief must distinguish those
two proofs and specify which one runs. New multi-voter test cases need explicit
ADR0024 Windows classification and inventory guards before any engine run.
No new timing experiment or exhausted Windows fix attempt is granted here.

## Security

PASS for planning. Required apply-time anchor fencing has no guard-only
fallback. Replay retains the original redemption position and cannot renew the
grant. Receipt restart depends on sealed private-key custody. Anchor-only peers
have no owner-control authority; destructive membership removal stays blocked.
Snapshot crossing checks and per-RPC permission withdrawal are requirements,
not tested production behavior.

The proposal does not resolve real expiry policy, custody, authenticated
transport or owner identity. Those remain hard production blockers. No source
slice may infer permission from the fact that this planning review passes.

## Actual evidence and limits

Read the full proposal and relevant existing ADR/briefs. Read cached OpenRaft
management.rs two-step change implementation and public append_membership API:
the API future submits the uniform step, and explicit append validates guarded
transitions and returns after commit/apply. This is feasibility evidence only.
No build, test, runtime cancellation probe, network or hardware action ran.
Independent Sol reviewed owner acceptance separately in 77a7fa81.

Result: no blocker to preparing a narrower next proof brief. The notes above
must be resolved there. M2 open, M3 unchecked; hardware paths remain
**unverified on real hardware**.
