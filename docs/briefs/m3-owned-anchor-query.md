# M3: owned, code-conditioned local anchor query

Sol 6.1 high design only, 2026-11-17; no children. Prepared base
`a66f15e5a8663c090e775db523ce5d4cf3e456b5`, containing actual main
`3022d601f09f24b0e5f092a33f2e9343b500471d` after PR174.
Root reports first hosted proof Ubuntu0.143/macOS0.296/Windows1.127 seconds,
108 tracked cases on every OS, Windows541s. Root evidence only, not checked here.
Main CI44126173935 was running at assignment. No source/build authorization.
Read with AGENTS, KICKOFF, ADR0005 (D1/D2/D3 and both kernel amendments),
[production L2](m3-production-learner-admission.md), its
[root review](../reviews/m3-production-learner-contract-root-review.md),
[membership decisions](m3-membership-owner-decisions.md) and their root review,
[atomic anchor](m3-atomic-join-anchor-fence.md) and root/independent reviews,
[pre-proof](m3-next-anchored-join-security.md) and root/independent reviews.
M2 open, M3 unchecked; Sol3/3, historical Opus0/2 unchanged; no new fix attempts.
Hardware paths remain **unverified on real hardware**.

## Scope and exact technical blocker

Add one internal local applied-state query, feeding the existing private
PreproofClient query (cluster16 || code_id16). No listener, socket, dispatch,
public RPC, generic read callback, mint authority or membership permission.
The public 32-byte query identifies a code; it is NOT a bearer proof. Possession
authorizes no redeem, signing, registry mutation, peer RPC or owner operation.
Success reveals only current canonical bound voters, at most641 bytes.
Previous L2 B5 and pre-proof contracts require BOTH existing four issue units
and eight read units inside the non-abortable closure. Retain that requirement:
it caps concurrent query work at4 and shares existing issuance admission.
Current issue_permits is Semaphore; try_acquire() is borrowed and cannot enter
a 'static spawn_blocking closure. Required representation is Arc<Semaphore>,
with try_acquire_owned(). This requires changing the initializer in start from
Semaphore::new(issue::MAX_ISSUING) to Arc::new(Semaphore::new(issue::MAX_ISSUING)).
BLOCKER under the assigned start freeze: root must explicitly resolve that
single initializer exception in contract review before implementation. No
borrowed permit, leaked node, separate pool, late weak upgrade or read-only
capacity shortcut substitutes for it. This is a technical scope conflict,
not a new owner product choice/approval flow. Without the exception, stop.
All other start code and existing read/shutdown helpers remain byte-identical.

## APIs and pure eligibility

In new consensus/anchor_query.rs, cluster-internal only:
ConsensusNode::anchor_query(&self, query: &[u8], clock: &dyn issue::SignerClock)
  -> Result<Vec<u8>, AnchorQueryError> (async).
Errors are static Refused, Busy, Unavailable; no nested Reject/StoreError or
caller data in Display/Debug. No public method/type or arbitrary writer escape.
Never log query/code text, secret/proof, certificate/private key or voter list; evidence uses synthetic counts/outcomes only.
Require query.len()==32 BEFORE copy, clock call, permit admission or allocation;
copy two fixed arrays. No secret, code text, anchor, verifier, CSR or proof input.
Malformed length and all eligibility failures return identical Refused.
Busy is only resource admission; closed/poisoned Owners, absent store, timeout,
poisoned lock or invalid membership decode return static Unavailable.
Sample existing SignerClock::now_ms once synchronously after argument bounds,
before waiting; move only u64 into work. Clock is a trusted internal dependency,
never a client timestamp. Existing synchronous-clock assumptions apply; no
hard timing promise covers arbitrary blocking clock callbacks. No new clock API.
Add EnrollmentState::pending_voters(cluster, code, now_ms, Option<&Membership>)
  -> Result<BoundVoters, AnchorQueryRefused>, pub(in crate::cluster), in
enrollment/anchor.rs. AnchorQueryRefused is a static unit type, not a reason oracle.
It borrows &self, reads private founded/cluster/committed_ms/records, and requires:
- founded, matching cluster, now_ms<=MAX_TIME_MS;
- checked now_ms+SKEW_MS >= committed_ms (reject rollback beyond existing skew);
- effective=max(now_ms,committed_ms), checked effective+2*SKEW_MS;
- exact records.get(code), consumed.is_none(), usable_time < record.expires_ms;
- present canonical membership and successful existing BoundVoters::observe.
Equality at expiry safety boundary refuses; absent/joint/unbound lists refuse.
No verifier comparison: query carries none. No record scan, secret hash lookup,
apply, pruning, committed_ms update, proposal, persistence or signing.
Return existing BoundVoters encoding count1..16 + sorted(node:u64BE,key32)*;
reuse observe/encode/hash, never duplicate anchor construction or list decoder.
One actual store mutex guard supplies BOTH StateStore::state() and membership().
Decode membership blob via existing membership::Membership::decode (<=4096),
then call pending_voters while guard remains held; encode <=641 before release.
Missing membership is Refused; malformed canonical blob is Unavailable, not a
usable list. No new StateStore API/schema or SQLite query is needed.
Only bounded local applied state is observed, on followers too. No Raft clone,
linearizer, quorum read or metrics equality: this is an advisory snapshot hint.
Lag can expose locally pending code already consumed elsewhere or stale voters.
Client MUST still match code anchor and same-session TLS key possession. Actual
first redemption MUST still pass authoritative E-1 fence; both signing guards
remain. Query success proves neither freshness, unspent code nor successful join.
Time checks use existing proposed-time/skew model and sampled local clock only;
waits, lag, clock rollback/failover can spoil real-time expiry. No real15-minute
guarantee, new trusted clock policy, mutation/pruning or freshness claim follows.

## Admission, cancellation and ordered release

Use existing READ_BUDGET1500ms absolute entry deadline (sampled monotonic entry),
inside existing issue5s envelope; no new deadline/timer/settings or retry loop.
Try-acquire issue owned unit, then read owned unit; failed second acquisition
drops first. No wait queue. Under Owners mutex refuse closed, reap completions,
require sm present, clone store Arc, and register exactly one async owner.
No await between closed check and registration; shutdown uses this same lock.
Owner checks tx.is_closed/deadline BEFORE submitting blocking work. If expired,
drop Arc then units without spawning. Otherwise move store Arc AND BOTH units
INSIDE closure passed to existing adapter::blocking; owner awaits it un-timed.
Closure locks store, computes bounded Result, leaves inner scope (guard drops),
explicitly drops store Arc, then read unit, then issue unit, then returns result.
Owner sends reply only if receiver live and before deadline. timeout_at applies
only to caller's oneshot wait. Timeout/drop never aborts owner or reclaims units.
After submission there is no early return that leaves units in the async owner.
Lock poison/error paths obey same order; normal RAII unwind must retain ownership
until closure unwinds. Do not promise cleanup after every panic/runtime failure.
Unchanged shutdown closes Owners, joins with total5s, records held reads/store
references before abort, and awaits Released flags. Aborting the owner cannot
abort spawn_blocking: closure still retains units/Arc while waiting for store.
Blocked work yields existing Orphaned, never clean shutdown or reclaimed capacity.
Existing report has no issue-held field; do not add one. Tests inspect existing
issue pool directly. Completion follows resource release; no callback owns Arc.

## Future implementation allowlist and frozen semantics

Production: new consensus/anchor_query.rs; enrollment/anchor.rs only pure method
and static refusal; consensus.rs only module binding, issue_permits Arc type,
and the initializer exception IF root explicitly permits it. Existing enroll/
found borrowing via Arc deref stays unchanged; no issuance semantics changes.
Tests: new consensus/anchor_query_tests.rs, bound as cfg(test) inside new module;
new pure tests in enrollment/anchor.rs; tools/ci/test_windows_cluster_quarantine.py
only additive dedicated query inventory/negative controls. No old test rewrites.
Freeze StateStore, membership codec, adapter, fresh_read/linearizable_state,
shutdown, all other start logic, issue/found, pki/preproof and existing TLS helpers.
Freeze arxj2 length270/proof-v2, formats/schemas/limits/time constants, E-1 fence,
replay/origin/fresh signer time/revocation, retained55, Cargo/lock, workflows,
nextest/filter/sampler/quarantine policy, service/control/executor/UI/transport.
Any further file/API change stops for root scope disposition, not a workaround.

## Bounded meaningful proof and counterfactuals (not executed here)

Ceiling: six additive pure test functions, one dedicated all-OS engine case
owned_anchor_query_single_voter, plus additive inventory controls. No three-node
fixture. Pure tables cover pending success; unknown/consumed/expired/wrong cluster;
unfounded/missing/joint/unbound; exact time boundaries/rollback/range/overflow;
canonical max16 and learner/address invariance; byte-identical state on every
success/refusal including expired retained tombstones. Compare independent
literal list bytes, not expected values generated by the method under test.
One real start/found/propose(Mint)/query/PreproofClient/enroll pipeline is feasible
using current anchored_join_tests APIs as reference, without editing that test.
Use genuine founder leaf/key and existing Driver/server; receive/decrypt fixed
query32, pass those ACTUAL bytes to anchor_query, then send returned list through
same TLS session/read_anchor/accept_anchor/release_proof. Count server plaintext
by phase: query32 is not proof32. Fresh consumed-code session calls real query;
driver sends any returned list and attempts normal gate release before asserting
zero received proof, then Refused. It must not short-circuit on an expected result.
No new production TLS escape, forged ticket, seeded registry or always-Ok verifier.
Snapshot stale/changed-anchor controls preserve client refusal and E-1 tests.
Same single-voter case uses F10-style held store lock, joined gate thread and
actual admitted closure-entry signal (cfg(test), in new module) before assertions.
Drop four callers: issue0/read4, four Owners and four extra store Arcs remain;
fifth query Busy. Release gate, join work, assert issue4/read8 and baseline Arcs.
Also prehold all8 shared read units: query Busy returns issue unit immediately.
Timeout one query behind lock: Unavailable while issue3/read7/store Arc retained.
Shutdown while still blocked: Orphaned, read_held1, store retained, issue3/read7
even after owner abort. Release/join gate; units return and store/log weak refs
expire. Closed Owners refuses subsequent valid query without new blocking work.
Whole case cooperative40s (35s work,5s cleanup reserve), existing operation caps;
do not require successful shutdown for the deliberately blocked Orphaned phase.
Join all test work before PASS; unresolved startup/cleanup fails as unknown.
Two designated once-only semantic mutants, restore exact baseline bytes/hash:
M1 move read unit to async owner (keep store captured in closure): blocked
shutdown must fail actual read7/held-capacity assertion after abort, not token check.
M2 omit consumed eligibility: actual consumed-query TLS path receives proof32,
breaking zero-proof assertion. Pure consumed refusal also fails; plaintext is
the primary oracle. Compile/unrelated red/survival earns no kill or retry credit.
Dedicated QUERY_REQUIRED inventory pins the one new case all OS and guards
omission/rename/ignore/platform hiding/module binding; inspect body/helpers for
exactly one start and no Cluster fixture. Preserve fixture28/sample8/founder13/
TLS14/atomic-issuance1/anchored-proof1, every old assertion and guard/sampler case.
After root contract review + independent Sol recheck + fresh green exact main,
root sets mechanical budget; one natural local round stops at first red. Local
pure/static checks only; NO local Windows engine. First engine/mutant ownership
and plaintext evidence requires ordinary hosted all-OS CI under root's bounded
authorization; no bespoke runner experiment or repeat-to-green. Exact-head
independent Sol source review, five normal gates, Windows<=660s and fresh green
exact main precede coordinator merge. This design grants no fixes/runs/publication.
Resolved conditionally: bounded code eligibility, coherent hint, closure ownership.
Remaining: start initializer exception; public dispatch/deny-by-default read policy,
production clock/freshness, discovery, sealed CA/joiner custody, durable receipt,
bootstrap permissions, owner/removal authority, admission/promotion/controller.
No production listener/discovery/key custody/controller/clock acceptance here.
