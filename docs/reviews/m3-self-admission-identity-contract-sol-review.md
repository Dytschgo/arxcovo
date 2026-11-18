# M3 retained peer identity: independent contract review

Reviewer: Sol 6.1, high; independent of brief author; no children.
Date: 2026-11-17. Base and inspected HEAD:
83d9987719e3147876f6fe322dc90ada61568a7a (clean before report).
Target: docs/briefs/m3-self-admission-kernel.md, bounded prerequisite only.
Read AGENTS.md, KICKOFF.md, ADR0005, actual PKI/enrollment/consensus source,
quarantine guard and cached openraft-0.10.0 source. Static review only.
No builds, tests, engine runs, network actions or public writes performed.

Disposition: no actionable blocker to the smaller representation contract.
Root disposition and policy recovery still precede source authorization.
This is not implementation acceptance, publication or merge authorization.

## Standards

No blockers or standards findings. The fixed-size private representation is
cohesive; no speculative admission abstraction or new serialized format is needed.
NodeId, ClusterId and KeyDigest are Copy (enrollment.rs:14-46), so Clone+Copy
remains feasible. A method scoped pub(in crate::cluster) is legal here:
cluster is an ancestor of pki::verify, and the returned types are accessible.
The private Identity type and raw fields need no visibility widening.

The nested test module can use super::complete and other private parent items;
Rust permits descendant access to ancestor-private items. In tls_tests.rs use
#[cfg(test)], #[path = "authenticated_identity_tests.rs"], then the module
declaration. At this file level that path resolves beside tls_tests.rs.
No helper edit or new public constructor is required. This was not compiled.

Allowlist is sufficient: verify.rs for the four bounded handle changes,
tls_tests.rs for one appended module binding, the new nested test file, and
test_windows_cluster_quarantine.py for additive inventory/negative controls.
All other source and existing helper/test bodies remain frozen. Existing
pki.rs allow(dead_code) already covers the unused production accessor;
no lint-policy or parent-source exception is necessary.

## Spec

No blockers. verify.rs:280-313 currently discards cluster/key after parsing.
Keeping node plus Option<(cluster,key)> faithfully retains the actual leaf
identity while preserving node(), constructor signatures and synthetic sender
behavior. Parse exactly once in from_verified_certificates; preserve completed
TLS1.3 and one-leaf checks. None must return existing static Incomplete.
mem_net.rs:38-44 uses only node() with check_sender; assume(None) must continue
to support the existing founder control without supplying an admission tuple.

Test plan is meaningful for this contract: real completed mutual TLS on both
sides, expected SPKI digest from the fixture key rather than peer_identity,
pre-handshake refusals, pure Found/Remove application with exact Outcomes,
and assume denial. Set first_free above all founder IDs, use sorted distinct
members/keys, and use two members so Remove does not refuse LastMember.
Another same-cluster state with another key is an adversarial observation,
not a claim that production supports rebinding a node.
M1 and M2 should compile and fail the designated equality/Incomplete checks;
they exercise actual retention and synthetic-handle exclusion respectively.
No mutation kill, restoration, static guard pass or runtime result is claimed.

Advisory S1 - inventory implementation detail (guard:80-112,162-184):
assert_fixture_quarantine's required= branch requires tokio::test, so do not
put these three synchronous tests there or widen the generic classifier.
Use a dedicated exact three-name inventory with plain #[test] classification
(the existing pure= shape is compatible). Pin its literal expected names,
both pki.rs -> tls_tests and tls_tests -> new-file bindings and exact path.
Negative controls must cover omission/rename, duplicate bindings, empty or
weakened inventory, ignore and cfg hiding on tests, helpers and both parents.
Preserve TLS14/fixture28/sample8/founder13/atomic1/anchored-proof1/query sets;
these three are additive non-engine all-OS tests, never quarantine/sampler IDs.

Advisory S2 - fixture diversity: all proposed happy tuples use cluster[1;16]
and node4. This kills designated M1 but would not detect constant cluster[1;16]
retention. Consider an additional genuine different-cluster connection within
test1 using configs assembled in the new file, without changing frozen helpers.
This is coverage advice, not a prerequisite for the stated two-mutant plan.

Advisory S3 - following-kernel notes are not adopted or proven:
Cached raft/impl_raft_blocking_write.rs:198 and raft/api/management.rs confirm
change_membership_if preconditions, complete-only response and inline second
step; add_learner sends empty preconditions regardless of blocking flag.
membership/change_members.rs:186 confirms AddNodes uses or_insert.
raft/message/precondition.rs:109 checks effective membership log ID, not registry
or applied/committed membership. The proposed observation/intent/replay guards
remain future work; these APIs alone cannot enforce registry or catch-up state.
consensus.rs:348 confirms existing fresh_read owns its blocking store read and
uses read8/1500ms; it is not the proposed membership observation contract.
Clarify "total5s shutdown": shutdown:448 shares one five-second deadline across
timed phases, but synchronous mutex acquisition and the post-abort join loop
are not deadline-wrapped. Static source does not prove a hard total wall bound.
No other important unsupported API fact found in the inspected feasibility notes.

## Security

No blockers. Preserve the existing trusted-config adapter warning verbatim in
meaning: from_client/from_server accept arbitrary rustls connections and cannot
prove the reviewed verifier was used. This is not an unforgeable capability.
Returning the tuple adds neither freshness nor admission/owner authority.
Future callers must freshly revalidate the retained cluster/node/key against
appropriate authoritative state; local authorize_peer alone has unbounded lag.
CA-only bootstrap/pre-proof evidence does not grant admission authority.

registry.rs:65-114 and enrollment.rs:476-515 support immutable node/key bindings
within one valid cluster history: Found replay does not insert, Remove does not
rewind allocation, Redeem increments checked and rejects live key duplication.
Keys can be reused at a NEW node after removal; numeric IDs are not globally
unique. These assumptions do not cover arbitrary snapshot corruption or future
rotation/rebinding features. No transport/custody/clock/removal policy is resolved.

Advisory C1 - keep assertion evidence static. Do not assert_eq! full identity
tuples or KeyDigest values: their Debug output can print binding material on a
failure. Compare boolean component predicates with static messages; format only
static refusal variants. Propagate saved failures after dropping fixtures and
joining any helper thread, as required by the brief. No secret output was needed.

## Proposed progress entry (not applied)

Sol6.1/high independent static contract review at base83d99877: no blockers;
three Spec advisories and one Security advisory recorded. Smaller retained-leaf
representation feasible; no implementation/runtime credit. Root disposition and
policy recovery required before source. PR163 recovery remains next merge;
macOS attempt5 running/published head22a unchanged are supplied context, not
independently fetched. No feature publication/merge authority. Sol3/3 and historical
Opus0/2 unchanged; M2 open/M3 unchecked; hardware **unverified on real hardware**.
Last merged PR unchanged by this review; brief and PROGRESS left untouched.

Counts: Standards 0; Spec 3 advisories (guard classification, fixture diversity,
shutdown wording); Security 1 advisory (static assertion evidence); blockers 0.
