# Root review of membership decision proposal

Target: Opus proposal 59c68332, based on ac1465a5. Root Sol reviewed
the full brief against ADR 0005 and cached OpenRaft 0.10.0 source.
This is cross-provider design review, not owner acceptance or source approval.

## Standards

No blocking finding. Scope and unverified claims are explicit. No build or test
was needed or run for this documentation review.

## Spec and security

The code-bound anchor and proof-before-release check address the CA-only
impersonation problem. The applied Redeem fence is stronger than withdrawal at
Found. Three corrections are required before the consolidated owner decision:

1. A stale anchor detected before Redeem leaves a code unspent. A membership
   change between the precheck and Redeem/issuance guards may consume it and
   then refuse certificate release. The unconditional unspent claim is false.
   Specify that race and recovery, or provide an atomic check design.
2. Admission must bind N to the authenticated caller's node and registered key.
   Being some current member is insufficient authority to replace another
   member's address. Define self-admission and existing-node replay precisely;
   administrative removal/promotion needs a separate authorization contract.
3. Narrow the owner decision to settled product semantics. Resolve or explicitly
   defer endpoint discovery, bounded pre-proof anchor access, outbound admission
   anchor use and joint-change recovery before calling the proposal build-ready.
   In particular, a post-commit registry guard cannot undo a committed promotion.
   Do not promise a safe compensating removal without its authorization,
   ownership and failure behavior being specified.

Cached impl_raft_blocking_write.rs lines 163-205 confirm membership preconditions
guard the first proposal; a flatten-step failure can leave a committed joint
configuration. Its documentation requires a new request based on current joint
state. This is read-only API evidence, not a runtime recovery proof.

The recommendation to keep two hosts at one voter accurately exposes loss of
acknowledged-write durability if the founder disk is lost. Owner acceptance is
still required. No voter cap, stalled timer, network endpoint or new RPC is
adopted by this review. Production L2 remains blocked; test-only L1 can proceed.

Result: Standards 0 blockers; Spec/security 3 corrections before owner handoff.
M2 open, M3 unchecked; hardware paths **unverified on real hardware**.

## Recheck of f06d90c3

Root Sol read the full revised brief. All three original findings are closed
for presenting product choices; this is not a build-ready L2 contract.

- The proposed apply-time fence and consumed-code recovery distinguish races
  before and after Redeem. Deterministic membership decoding remains B3; its
  fallback must never silently replace the owner-approved security contract.
- Self-admission derives identity from the verified leaf, refuses address
  substitution, and does not forward another caller's identity.
- Promotion cannot be undone by a post-commit guard; the proposal now states
  revoked-voter and interrupted-joint outcomes without promising compensation.

B1-B5 remain implementation blockers. The pre-proof read also needs the same
bounded read ownership/cancellation discipline as existing store reads; a
connection timeout must not release capacity while a blocking read still runs.
These are requirements for a later reviewed contract, not evidence they work.

The owner's choices are code-bound voter trust, bounded admission/progress,
and the two-host durability/availability tradeoff. No choice is adopted here.
The binary code grows by 32 bytes, which adds 64 hexadecimal characters to its
text form (206 to 270), rather than only 32 text characters.

Actual check: documentation and cached API evidence review only. No build,
runtime probe, test, hosted action or network action was run for this recheck.
Verdict: PASS for owner decision preparation; production L2 remains blocked.
