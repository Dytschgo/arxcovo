# M3 self-admission prerequisite: retain authenticated leaf identity

Current checkpoint (2026-11-18): identity prerequisite implemented on green main
e3e79089; Clippy and pure TLS13 passed, both semantic mutants killed/restored.
Independent Astra source and B1-B3 guard rechecks pass. Publication preparation
is ready; exact-public-head review and hosted gates remain pending. Earlier
policy and implementation holds below are historical. Production self-admission
is not implemented or accepted by this prerequisite.

Sol 6.1 high, design only, 2026-11-17; no children. Implementation-ready proposal,
Root and independent Sol contract review complete; source authorization remains
blocked on policy recovery and fresh exact-main gates.
Assigned branch t3/self-admission-kernel-brief; HEAD and local base ref
work/m3-after-anchored-preproof both verified as 0c9efc0a00e701d3c56ca309ffe723281a0197ce.
Only this document is delivered; no source/build/test/engine/network/public action.
Read AGENTS, KICKOFF, ADR0005, production L2 and its root review, latest membership
owner decisions, owned-query brief/root/recheck, evidence and source review.
Unpublished query source f9d0a413/evidence15610c33/review89efaaf5 remains held behind
PR163 budget recovery. Query implementation at this HEAD matches f9d0a413.
Supplied root evidence: published main3022d601 functionally recovered after runner
infrastructure recovery; first attempt905s remains budget blocked. Not fetched here.
M2 open, M3 unchecked; exhausted Sol3/3 and historical Opus0/2 unchanged.
All hardware paths remain **unverified on real hardware**. No feature publishing now.

## Why this smaller slice comes first

Actual pki/verify.rs: Identity and peer_identity retain cluster/node/SPKI digest;
AuthenticatedPeer(NodeId), from_verified_certificates and node() discard cluster/key.
from_client/from_server require completed TLS1.3, exactly one leaf, then parse it.
The comment explicitly trusts this module's reviewed configs: arbitrary insecure
rustls configs can forge evidence. The handle is not an unforgeable capability.
PeerVerifier::authorize verifies configured cluster, registry triple and target;
TLS13 signature verification proves possession; resumption is disabled both ways.
A future admission caller accepting only node() cannot independently bind a handle
from another cluster with the same numeric ID to its own fresh registry read.
There is no production admission dispatcher today; this is a missing contract,
not evidence of an exploitable deployed endpoint. Never fix it with body identity.

enrollment/registry.rs::{found,remove,authorize_peer} and enrollment.rs Redeem:
Found is once-only; replay cannot replace/restore removed entries. Founders are
below first_free. Redeem allocates next_node_id, checked increments without wrap,
rejects duplicate live keys, and exact replay requires the original binding.
Remove never decrements allocation. No current key-rotation/rebind command exists.
Thus (cluster,N) has an immutable key during its lifetime, and removed N cannot
be reused in that cluster. This does not make numeric N globally unique or prove
which cluster configured the connection. Preserve the actual leaf tuple first.

## Bounded implementation API and ownership contract

In pki/verify.rs, retain private node: NodeId and binding: Option<(ClusterId,KeyDigest)>
in AuthenticatedPeer. Keep Clone+Copy, node() and current public constructor signatures.
from_verified_certificates must call existing peer_identity exactly once, storing
its node and Some((cluster,key)); no separately supplied identity or certificate.
Add pub(in crate::cluster) authenticated_identity(self)
  -> Result<(ClusterId,NodeId,KeyDigest),VerifyRefusal>.
Return the retained tuple for Some; None returns existing static Incomplete.
Existing cfg(test) assume(node) retains node() behavior but stores binding=None;
it must never produce an admission-usable tuple. No production raw constructor,
Default, deserialize, setter, registry fallback, Debug dump or certificate buffer.
Do not turn Identity into a public construction API. No new error variants needed.
Retain all handshake/chain/signature/bounds/target checks unchanged. Accessor means
identity from the trusted-config adapter, not freshness, owner authority or custody.
Caller must use reviewed peer configs and freshly authorize the retained triple;
CA-only bootstrap/pre-proof connections never create admission authority.
This synchronous fixed-size copy owns no TLS session/store/task/permit and adds no
wait, deadline, clock call, serialization format or admission operation.

## Explicit future source allowlist and frozen surface

Paths below are repository-relative; this task changes none of them:
- crates/arxd/src/cluster/pki/verify.rs: only handle representation, constructor
  tuple retention, accessor and synthetic assume representation above.
- crates/arxd/src/cluster/pki/tls_tests.rs: append exactly one cfg(test) path module
  binding to authenticated_identity_tests.rs; existing helpers/bodies unchanged.
- New crates/arxd/src/cluster/pki/authenticated_identity_tests.rs: three tests below,
  nested under tls_tests to access its private complete helper without widening it.
- tools/ci/test_windows_cluster_quarantine.py: additive dedicated identity inventory
  and binding/omission/ignore/platform-hiding negative controls only.
Freeze every other source file, including consensus.rs/Owners/start/shutdown,
mem_net and assumed-founder engine control, all existing TLS tests/helpers,
enrollment/state/log/snapshot/codecs, anchored-query and pre-proof kernels,
service/executor/CLI/ingress/UI, Cargo/lock/toolchain/workflows/nextest/sampler,
quarantine classifications, retry/timeouts/permit counts/settings and docs/PROGRESS.
No scope workaround or edits to frozen helpers; report a concrete blocker to root.

## Meaningful proof plan, not executed

Three additive synchronous non-engine tests, all OS; no Tokio runtime or Raft start:
1. peer_identity_retains_verified_tuple: actual completed mutual TLS via existing
   connections/complete helpers, real CA/leaf/key and registry. Extract BOTH handles;
   compare tuple to independent expected cluster[1;16], node4 and SPKI hash of the
   fixture key (not peer_identity output). Incomplete client/server still refuse.
2. peer_identity_fresh_binding_controls: reuse genuine completed handles, create
   EnrollmentState::new plus actual apply(Found) fixtures, checking every Outcome.
   Matching tuple authorizes. Same N/key in another cluster -> WrongCluster;
   same cluster/N with another registered key -> KeyMismatch; missing N -> NotMember.
   Apply Remove in a two-member fixture -> old retained handle now NotMember.
   Existing new/apply/FoundMembers APIs suffice; no seeded private registry fields,
   always-Ok verifier, mocked tuple or synthetic handle for positive evidence.
3. peer_identity_assumed_founder_denied: assume(node4).node()==node4, but new accessor
   returns Incomplete. Existing sender check/engine founder control stay unchanged.
   Assert all errors are static; no tuple/key/cert material emitted as evidence.
Pin these three names and BOTH parent/module path bindings in additive inventory;
reject deletion/rename/ignore/cfg hiding including on helpers/parents. Keep existing
TLS14/fixture28/sample8/founder13/atomic1/anchored-proof1/query inventories unchanged.
Two designated semantic mutants, each once after a passing authorized baseline:
M1 replace retained cluster with [2;16] after real parsing; test1's independent
cluster equality must fail. M2 return a fabricated Some binding for assume; test3's
Incomplete assertion must fail. Compile/unrelated red/survival earns no kill.
Restore exact baseline bytes/hash after each; save failing assertion only after
TLS fixtures are dropped and helper threads (if used) joined. No rerun-to-green.
Root sets one bounded round/mechanical allowance after contract and main gates;
natural semantic/static red stops. No reset of exhausted engine fix counters.
Future fmt/Clippy, focused pure TLS tests and additive Python guards are appropriate;
ordinary hosted gates compile restored source. No local Windows engine execution.
Proof classification: real in-memory TLS plus pure registry apply, not transport,
Raft admission, deadline, catch-up, revocation freshness or production acceptance.

## Following kernel contract and remaining feasibility gaps (not this slice)

Cached openraft-0.10.0/src/raft/impl_raft_blocking_write.rs:198 change_membership_if
accepts preconditions; management.rs:69-207 drives its complete-only response and
possible second step inline. management.rs::add_learner supplies Batch::of([]),
so add_learner cannot enforce our observation guards, regardless of blocking flag.
Use ChangeMembers::AddNodes plus retain=true and LastMembershipLogId(observed),
CommittedLeaderId(observed). membership/change_members.rs::apply uses or_insert
for AddNodes, not address replacement. SetNodes is forbidden. Engine edits do not
check application registry/catch-up. Precondition::ensure_satisfied checks EFFECTIVE
membership, not applied/committed membership or registry; registry Remove is unguarded.
Future fresh read must collect registry/applied membership under one store guard,
match full effective+committed IDs AND contents, uniform established leader, then
recheck/guard submission. Existing fresh_read uses read8/1500ms and owns blocking
store reads; it alone does not implement this membership observation contract.
Require bound self tuple, N not voter for new admit, <=16 nodes including revoked,
and no other bound learner below fresh catch-up target; caught-up host2 allows host3.
Already learner/voter same canonical addr returns current applied membership
watermark, not original receipt; different addr -> AddressConflict without proposal.
Same operation is (authenticated cluster,N,key,canonical addr); no request actor/N.
Canonical address parsing still needs its own accepted scope; no discovery changes.

consensus.rs::{Owners,propose_response,shutdown}: proposal64 shared owned units,
closed check+JoinSet spawn under mutex, caller oneshot detach, shared5s shutdown deadline (not a proven hard wall ceiling),
abort/join and Orphaned store-release accounting exist. Membership needs ONE local
intent reservation under Owners, surviving caller cancellation until engine API
resolution (shutdown may abort). Do not hold std mutex across await or add a pool.
A replay while intent pending must not submit again; definite concurrent intent
refuses RetryLater. Same-address retry reconciles committed/applied state after
Unknown; restart reconciles membership, never relies on lost in-memory reservation.
Entry deadline5s covers reads and reply; timeout only caller wait. Pre-submit Busy/
Stopped/NotLeader/RetryLater are definite only with no possible submission. Once
submission may have occurred, timeout/drop/fatal/ambiguous error -> Unknown, never
failure or automatic retry. Public API Pending is not evidence of queue submission.
Owned future retains proposal unit until API resolves; any non-abortable blocking
work must itself own read units/store Arc, including after shutdown abort.
On applied membership response L, fresh registry recheck must cover at least L;
revoked N -> AdmittedButRevoked, never success inferred from effective metrics.
If recheck cannot complete within budget -> Unknown; replay resolves later. Later
Remove may revoke any success; no linearizable perpetual authorization promise.
Revoked nodes count capacity, cannot promote, and are excluded from bound-learner
ordering. Never compensate by RemoveNodes/registry writes or grant owner powers.
Progress/Stalled60s, joint recovery and automatic initial three-voter promotion need
separate briefs/proofs; host2 stays learner until host3 caught up. No auto-removal.
Actual engine guards/serialized replay/Unknown/Remove-before-L runtime proof remains
required; pure mocks cannot earn it. Multi-voter cases need ADR0024 classification.
No transport/discovery/removal authority/custody/clock policy is resolved here.

## Proposed PROGRESS entry (root-owned, not applied)

Sol6.1/high: isolated design complete, identity-retention prerequisite specified;
node-only peer handle prevents full self tuple revalidation. No implementation or
runtime credit. Root contract review + independent recheck + fresh exact-main gates
required before source. PR163 budget hold; M2 open/M3 unchecked, Sol3/3 and historical
Opus0/2 unchanged; hardware unverified. Last merged PR unchanged by this task.

## Root disposition of independent review

Review29ec4fd5 has no blockers. For implementation, use a dedicated synchronous
three-name guard inventory without widening the async fixture classifier; pin both
parent bindings and reject duplicate/hidden/empty inventories. Test1 also includes
a genuine second cluster using configs assembled in the new file, with no helper
changes. Compare identity components with boolean predicates and static messages,
never Debug-print bindings. The existing shutdown deadline is shared across timed
phases; mutex acquisition and the post-abort join loop prevent a hard wall claim.
These are contract refinements only. No source authorization before PR163 policy
recovery, confirmed fresh exact-main green, and a bounded verification assignment.

## Root green-main implementation admission - 2026-11-18

Exact main05b9c829 and first CI44212212325 green608wall/605charged are verified.
The old policy-recovery hold is historical. Root authorizes this pure-TLS identity
prerequisite locally on that main, independent of PR176 pending verification.
For the genuine second-cluster fixture, build a fresh synthetic CA/leaf with
existing rcgen APIs directly in the new test module; use production TLS config
builders/verifier. No forged handle, ticket/API/helper widening or logged keys.
No production admission, engine, transport, owner authority or custody claim.
Preserve current MAIN inventories; the unpublished query inventory is not on
this base and must be retained when integrated later. Source scope is unchanged.
