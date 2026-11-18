# M3 fresh membership observation prerequisite

Root Sol6.1/high disposition of independent Sol6.1/high read-only scope analysis,
2026-11-18. Base exactgreenmain6f4618c76256c2ead9dfb501c2c3946fde14a81b,
CI44269052508 attempt1SUCCESS603wall/600charged. Owner authorizes independent M3
underADR0023; this is source admission for the bounded slice below, not an endpoint.
M2OPEN/M3UNCHECKED; Sol3/3 exhausted, Opus0/2 unchanged. No founder work authorized.

## Source gap and behavior

The identity prerequisite retains the real TLS leaf cluster/node/key tuple.
Existing fresh_read already obtains an established read leader and executes a
closure under the store lock with owned read resources. No production API joins
that same registry+applied membership image to full effective AND committed
membership views and a stable established leader. Test-only Observation is not
that API. Local advisory anchor_query deliberately needs no quorum and is frozen.

Add ConsensusNode::observe_membership(AuthenticatedPeer), cluster-visible only,
returning a bounded private-field ObservedMembership with established leader,
exact applied membership ID/content and applied watermark. Static errors:
Busy, NotLeader, Unavailable, RetryLater, PeerRefused; no binding/error payloads.
Synthetic assumed peer must refuse before read admission; no body tuple entry API.
Result owns no store/connection/permit/task/certificate. No Debug binding dump.

Take one metrics sample without retaining a borrow. Bound native memberships
BEFORE cloning (existing canonical encode bounds <=16nodes/<=2configs/<=200addr).
Sample committed vote/currentleader and effective+committed IDs AND full contents.
Call existing fresh_read unchanged, capturing bounded sample/localnode/retained
peer tuple. Under its store guard require applied watermark+canonical membership,
exact membership IDs AND contents equal both views; exactly one voter config,
reject duplicate-set joint too; committed vote leader and currentleader=self;
watermark covers membership, exact identity at equal index; actual same-image
registry authorize_peer must accept retained tuple. Return bounded candidate.
Established fresh_read leader must equal sampled committed vote leader. Resample
once, comparing vote/currentleader/both full memberships; any change =>RetryLater,
no retry. Unrelated progress fields do not invalidate otherwise identical views.

This observes a watermark under guards; metrics and SQLite are not atomically
sampled. Later Remove can revoke a returned observation. Future admission still
needs serialized guarded submission and fresh post-apply reauthorization. No
admission operation, permanent authorization, catch-up or production runtime claim.
No new issue unit: this authenticated read is separate from pre-proof query.

## Allowlist and tests

consensus.rs one module declaration; new consensus/membership_observation.rs only:
adapter, private validator/sample and synchronous tests. Root brief/progress,
evidence/review docs and CI duration checkpoint. Existing fresh_read, Owners,
start/shutdown, anchor_query, founder/issuance, TLS, state/log formats, fixtures,
workflows, quarantine, retries/settings/deadlines/permit counts stay byte-identical.
Additive inventory guard only if existing policy requires it; never hide cases.

Meaningful pure tests use real EnrollmentState apply Found/Remove and canonical
membership. Assert exact success result, independent effective/committed ID and
contents mismatches (voters/learners/addresses), missing/below/equal-index wrong
watermarks, joint including identical configurations, uncommitted vote/follower/
epoch/read-leader mismatches, wrongcluster/missing/key/realRemove, changed second
sample vs irrelevant progress, assumed-peer refusal and no state mutation.
No engine fixture or localWindows engine. D-only fmt/lockedoffline workspace
Clippy and focused pure tests, exact positive test count. Natural semantic red
stops for disposition. No fix counter reset. Independent actual-source review
before publication/allfive gates/freshgreenmain/Windows<=660s before merge.

Future actual-engine evidence is required before admission relies on the adapter;
pure validation tests cannot establish quorum/race/cancel behavior. Reuse prior
fresh_read ownership evidence only with byte-identical ownership implementation.
No new engine experiment follows from this brief. Hardware **unverified on real hardware**.

Designated meaningful semantic controls, each once only if baseline passes:
omit content comparison retaining IDs; omit actual authorization; omit established
leader equality. Intended assertion must fail; compile/unrelated failure is not
kill credit; restore exact source bytes after each. Root may assign a bounded
pure-only round after independent source review. No mutation execution yet.
