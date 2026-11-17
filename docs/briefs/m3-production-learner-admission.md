# M3 production learner admission: bounded L2 contract

Proposal only; Sol 6.1 / high, 2026-11-17. Required dependent base: `50100a023880f8fa271bae40ef0146669dd7222e`.
Read with [owner decisions](m3-membership-owner-decisions.md), [root review](../reviews/m3-membership-decisions-root-review.md),
[ADR 0005](../adr/0005-raft-membership-and-enrollment-spike.md), and [L1](m3-learner-admission.md).
Owner D1/D2/D3 are settled. This contract replaces their speculative technical details only upon independent review; it grants no production authority.
L1 merged main `3084f8e5`; exact-main CI `44077792712` pending per coordinator, not queried here. No production source before main green.
M2 open; M3 unchecked; **unverified on real hardware**. Shared Windows repair counters remain Sol 3/3, Opus 0/2; no retry allowance added.
Only this brief is delivered. Coordinator retains progress/gate bookkeeping. No new owner interview is required for the bounded choices below.

## B1: one-code endpoint discovery and bootstrap authentication

Propose variable-length `arxj2-` + hex(cluster16 | code16 | secret32 | ca_fp32 | anchor32 | hint_count:u8 |
[hint_len:u8 | hint ASCII]* | check4). Integers are BE. One to four distinct hints, sorted bytewise, 1..64 bytes each; maximum text 792 bytes,
within existing MAX_CODE_TEXT=4096. Check is first four SHA256 bytes under `arxcovo/join/check/v2`, covering the entire preceding payload.
This supersedes the earlier fixed 270-character encoding estimate; the anchor adds 64 hex characters and discovery adds variable bytes.
Hints are canonical numeric SocketAddr strings: IPv4:port or [IPv6]:port, nonzero port, no DNS, scope IDs, paths, userinfo, schemes or redirects.
Refuse unspecified, multicast, loopback, broadcast, IPv4-mapped and link-local addresses. Private LAN, Tailscale and other unicast IPs are allowed.
Use the same parser for admission addresses; <=64 is narrower than the existing membership codec's 200-byte limit (not the earlier 255).
Mint uses only actual concrete addresses of its eventual bound enrollment/peer listener, with wildcard/unavailable address => EndpointUnavailable.
No interface scan, DNS lookup, guessed port, new setting or second paste. Real listener/address integration remains a transport dependency.
One working minter address suffices; up to four addresses support its LAN/Tailscale paths. No promise of discovery if that host is offline.
Hints are untrusted dial targets, never authority. Try each at most once per explicit join attempt within existing budgets; no background retries.
Leader hints from authenticated replies obey the same parser/bound; never forward proof or caller identity. Unreachable/stale hints => RetryLater.

Anchor = SHA256(`arxcovo/join/anchor/v1` | cluster16 | count:u8 | sorted(node:u64,key_digest32)*), 1..16 unique nonzero IDs.
Mint takes a fresh applied read, with matching effective, committed and applied membership log IDs AND canonical contents, uniform and fully bound.
Refuse joint/in-flight/unbound sets. Recheck observation around mint response; later voter changes may still make a code stale, honestly unspent.
Mint authority stays the existing trusted local boundary; this proposal does not expose a public mint RPC. Only fresh in-memory codes are proposed.
Joiner rejects arxj1, malformed/noncanonical arxj2 and checksum/cluster mismatches. Checksum is corruption detection, never authentication.
TLS first pins CA fingerprint. Before proof, AnchorQuery returns the current bound voters. Require digest == code anchor AND TLS CertificateVerify
possession of a listed (node,key) on this SAME connection. Only then send proof+proven CSR+anchor+stable request ID. Never send a verifier as proof.
A CA-only attacker, substituted leaf key or forged list cannot obtain proof. Do not reconnect between list validation and first proof release.
Retain the validated list in process memory: live-process replay may redial a key in that SAME list without another query (consumed-code queries refuse).

## B5: closed ingress and bounded pre-proof read ownership

No-client-cert enrollment listener dispatches exactly AnchorQuery and Redeem. Unknown method/frame, peer RPC, mint, control, execution and MCP deny.
AnchorQuery is exactly cluster16+code16, one per connection; reject oversized frames before allocation. Success <=641 bytes (count+16*40).
Pending/unexpired code only, local applied read only; no fresh/quorum read, proposal, database scan, secret lookup response or log write.
Unknown/consumed/expired code, wrong cluster, joint/unbound membership and bad query return a fixed-shape refusal; overload returns fixed Busy.
Expiry here is advisory under the existing proposed-time model; it is never evidence of real-time 15-minute expiry. Redeem remains authoritative.
Acquire owned units from existing four issue and eight read permits using try-acquire; no queue or new capacity. Making issue semaphore Arc-owned
is a required ownership change, not a permit-count change. Under Owners lock, refuse closed, reap completions, spawn one local-read owner.
Move store Arc AND BOTH permits into the spawn_blocking closure before it can run; closure locks store, copies bounded answer, drops lock/Arc,
then permits, then signals completion. Before spawning, check receiver closure/deadline. After spawning, cancellation cannot release capacity.
Outer response budget is existing 5s, local read capped by existing 1.5s READ_BUDGET (earlier absolute deadline wins). Timeout/drop only detaches reply.
No timeout around the owner that would detach unaccounted blocking work. Shutdown closes Owners and uses unchanged total 5s; aborting an async owner
cannot stop blocking closure. Held units/store Arcs remain visible; unresolved release => existing Orphaned, never clean shutdown or reclaimed capacity.
No proof, query body, code text, anchors or receipt in logs/MCP/evidence; fixtures use synthetic markers and report counts/outcomes only.

## B3: deterministic apply fence, schema and original receipt position

Current consensus.rs encode/decode already provides a canonical membership-v1 codec (1..2 voter configs, <=16 nodes, <=200-byte addresses,
4096-byte blob). Move its pure structural validation into shared cluster/membership.rs; consensus converts that representation to/from OpenRaft.
StateStore imports only the pure codec, not engine metrics or an async callback. Validate membership at row decode, state open/peek/install and apply.
Canonical encode-back equality, sorted unique IDs, voter-subset-of-nodes, bounds and complete input consumption are mandatory. Invalid blob => store
error with no persist/sign; valid joint/unbound/absent membership => deterministic AnchorChanged for a FIRST redemption, no code consumption/allocation.
StateStore::apply passes the E-1 image's membership+registry and current full LogPos E into the pure enrollment transition. Compute anchor there,
before first consumption, node allocation, timestamp advance or record pruning. Preflight in enroll is only a fast refusal. Never use an issuer-guard
comparison as fallback: if shared decoding cannot be implemented/proven, stop L2. No clock, network, metrics or signer I/O inside apply.
First Redeem carries anchor32 in a distinct versioned command (new log tag 6, not a longer old tag 2). Code proof is reduced before proposal as today.
Consumption atomically records original anchor and full E=(term,leader,index) with request,key,N. Exact replay authenticates proof/request/key/anchor,
checks N still bound and returns ORIGINAL E, even if voters have since changed; changed anchor/request/key cannot retrieve the grant.
An already-consumed authorized replay does not re-run the new-consumption anchor fence. Keep existing grant/time checks; replay does not renew lifetime.
Unknown after apply is consumed. Before/at-fence refusal is unspent. Post-E anchor drift alone cannot invalidate signing/release; retain both fresh
issue_guard checks (grant, binding, issuer leadership), issuable-time checks and rollback checks. Return leaf,N,original E, not replay entry index.
Change IssueTicket accordingly: original E for receipt, current response leader for issuer guard; only committed adapter responses construct it. Removal after E gives Revoked, never an unspent claim.

Explicit incompatible local schema: new state-image version 3 and DB user_version 2, log DB user_version 2/tag6; old state/log DBs refuse Unsupported.
Do not silently migrate, delete or reopen an old directory as empty. No released cluster exists; developers retain old stores for inspection.
Legacy tag2 cannot be applied under the new contract. No mixed-version cluster claim. Snapshot codec includes consumption anchor/E and persisted genesis
first_free=2; open rejects a join directory of another cluster or genesis even after next_node_id advances. Derive init from code, never caller choice.
Keep 16KiB state, 4096-byte membership and 4163-byte log-row limits. Extra receipt fields mean MAX_RECORDS alone no longer proves image fits.
Before any enrollment mutation, validate tentative encoded state plus full reserved 4096 membership bytes and image overhead <=16KiB; otherwise
StateFull with previous enrollment data intact, while persisting applied position. No new limits/settings. Decoder enforces same reservation invariant;
all <=4096 membership replacements must then fit. Include maximal consumed-record and membership cases; do not rely on persist() failing after consume.

## Durable joiner receipt, custody and snapshot fence

Before sending Redeem, create CSR key/request in memory. After reply, verify CA, cluster, own CSR key, N absent from anchors and full E sanity.
Atomically persist local receipt {version,cluster,N,leaf,own key digest,anchor list,E,genesis} <=8192 bytes, outside Raft state/log/snapshot.
The corresponding private key must already be durably sealed to the daemon identity and referenced by receipt; receipt alone cannot resume mTLS.
No proof/secret or private key in receipt. Private directory/owner, no symlink/reparse traversal, fixed basename, bounded exact codec, atomic replace,
flush+directory durability per platform and exclusive startup ownership are requirements. Restart must refuse missing/mismatched key or receipt.
This is BLOCKED on M2 daemon/build-account custody and a reviewed key-store adapter. An ordinary private file is not a substitute for custody.
Crash before receipt (including after consumed E) leaves orphan N. No persisted proof means restart cannot redo Redeem; report incomplete/consumed,
never auto-redeem with a new ID. Live-process exact replay is allowed under existing grant lifetime; cleanup is unavailable until B2 is implemented.
Only after receipt+key are durable may joiner start serving bootstrap replication or self-admit; joiner starts with founder_init(code.cluster), no initialize.
Pre-fence verifier is CA AND (own local applied registry OR receipt anchors). Anchor-only permission is inbound AppendEntries/InstallSnapshot alone;
all Vote, pre-vote, forward, control, execution and MCP deny. Outbound anchor exception is own Redeem and own admit_learner only.
Existing registry-authorized Raft peers retain normal roles; anchors never grant owner authority. No seed writes to EnrollmentState from receipt.
Withdraw anchors once local applied >= original E with exact ID at equality AND own (cluster,N,key) is bound. At later index a validated snapshot
may cross E without retaining its log row; require matching cluster, own binding, valid membership/applied metadata and existing monotonic snapshot guards.
Adapter already compares snapshot meta to image in install_snapshot; preserve it. Snapshot below E leaves anchors active, at/above E crosses fence.
If a crossing snapshot revokes N, stop bootstrap and refuse admission; never keep anchors to resurrect N. Malformed/stale/wrong-cluster snapshot refuses.
Withdrawal is durable/idempotent; first switch verifier to own registry, then persist receipt without anchors before accepting more sessions.
Re-evaluate bootstrap permission on every RPC, including existing sessions; crossing the fence removes their anchor-only exception immediately.
Crash between apply and withdrawal re-evaluates fence BEFORE binding listener on restart. Never withdraw merely at Found.
Anchored keys removed after E retain only this replication exception until fence; stale membership may stall catch-up. If leadership moves to a newly
promoted, unanchored voter before its binding is applied, fail closed; no anchor expansion from replies, CA-only fallback or automatic re-enrollment.

## D2 and B2: admission, replay and removal authority

admit_learner{addr} is leader-only mTLS self-admission. Extract cluster,N,key from verified leaf/connection, require current fresh registry binding;
no body node/actor/forwarded-principal fields. Follower returns bounded NotLeader hint, never forwards. Parser is B1; every dial verifies registry key,
CA and sender ID. No HTTP/URL fetch through addr. Address ownership is not proven by syntax; failed dial stalls rather than authorizes another node.
Read membership+registry consistently as for mint. Require N not voter for a new admission, no other BOUND learner below catch-up and node count<=16.
One exception to learner ordering is essential: caught-up host2 permits admission of host3. Revoked learners still occupy membership slots.
Owned task through existing 64 proposal units/Owners calls change_membership_if(AddNodes{N:BasicNode(addr)},true,
[LastMembershipLogId(observed),CommittedLeaderId(observed leader)]). Match effective/committed/applied view first; precondition protects subsequent races.
At apply response return {N,L}; never infer commit from effective metrics. Already learner/voter with same addr returns its observed applied membership
position (observation watermark, not a fabricated original admission receipt); different addr => AddressConflict. No SetNodes/address replacement.
Budget 5s at entry covers reads+proposal waits. Busy/pre-submit refusal is definite; after submission timeout/drop/fatal/ambiguous error => Unknown.
Owners retains unit until API resolves or existing shutdown aborts. Caller cancellation neither cancels membership nor starts another intent.
Replay/status resolves Unknown after current membership is committed/applied; no retry loop. A concurrent registry Remove before L => AdmittedButRevoked,
excluded from promotion and bound-learner ordering, counted toward capacity, never automatically removed. Guard cannot undo L.
Status reads only nonblocking metrics; progress is a hint until applied. Report matched, local applied, target commit watermark and membership phase.
Leader match and joiner apply progress are separate: monotonic local Instant baseline resets on restart/leader epoch, not on lower/stale observations.
Increasing matched/applied counts as progress; heartbeat/status polling does not. 60s without advance => Stalled; later advance clears it. No auto-removal.
No fixed catch-up deadline or custom replication worker; OpenRaft owns replication. UI implementation is outside this contract.

B2 is explicitly BLOCKED, not assigned fictitious production authority. local.rs authenticates a configured OS client; http.rs maps bearer tokens to
MCP Principal names; neither source supplies a cluster OWNER capability for destructive membership control. service config client_identity is not that role.
Do not equate peer mTLS, any MCP principal, configured local client or a request actor field with owner authority. No public remove/promote recovery RPC.
Future authenticated owner-control integration must carry its existing unforgeable identity into a leader-local operation; until it exists refuse removal.
Required future sequence: fresh quorum/remaining-voters check; guarded RemoveVoters and flatten (if voter), RemoveNodes at uniform commit, then registry
Remove; serialize with promotion, re-read on Unknown, preserve IDs and all partial states. Registry-only removal must also be inaccessible to peers.
Learner/orphan removal uses same authority; neither Stalled nor a post-commit revocation authorizes compensation. No forced reset, approval UI or auto cleanup.

## D3 and B4: promotion, membership observations and interrupted joint recovery

Automatic leader-internal initial policy only: uniform voters {1}, exactly two eligible bound learners a,b, both matched>=fresh commit watermark; promote
both together to {1,a,b}. Host2 alone stays learner. Host arrival numbers are not node IDs: orphan/revoked IDs are never reused. For hosts4+ leave as learners; no new voter cap or odd-count policy is adopted.
Fresh read includes registry and applied membership; compare full effective and committed metrics membership (ID AND contents) with that applied blob.
Mismatch, uncommitted leader term or joint => defer new admission/mint/promotion. Raft LastMembershipLogId guards EFFECTIVE membership, not applied state.
Never treat metrics membership as proof of commit or a substitute for apply-time anchor checking. Registry changes do not change membership log ID.
Use change_membership_if(AddVoterIds({a,b}),false,[LastMembershipLogId,CommittedLeaderId]) under same Owners/64/5s contract.
Registry removal racing promotion may yield PromotedButRevoked: an offline-equivalent voter for quorum. No guard rollback or automatic compensation.
Promoted voters must acquire sealed CA custody before issuing; learner never holds CA. Missing custody does not authorize CA transfer in a Raft entry.

Cached OpenRaft0.10.0 exposes PUBLIC append_membership(membership,EntryPayload::Blank,preconditions). For a committed/applied joint J whose effective
and committed views both equal J, construct Membership::new(vec![J.get_joint_config().last().clone()], ALL J node metadata). Preserve every learner
and address. Call append_membership with LastMembershipLogId(J), CommittedLeaderId(current) and LastLogId(observed tail); no new target is chosen.
This is an exact flatten of the recorded last voter set, not AddVoterIds against a guessed configuration. Default Membership's quorum-intersection
transition accepts joint->its last uniform set. A stale tail/membership/leader rejects; don't overwrite newer intent or spin/retry inside response budget.
If views differ or J not committed, report Pending/Unknown and wait for engine progress; if uniform, recovery already complete. UnsupportedTransition
is a hard contract failure, not permission to switch APIs. Membership change APIs do not enforce learner catch-up; our preflight must do it.
management.rs drives two-step flatten inside its future, retains leader precondition and replaces membership guard with joint ID. Caller timeout detaches
only reply, not that Owners future. Shutdown abort after joint commit can strand J. New leader recovery uses above exact flatten only after fresh checks.
PreconditionFailed on step2 can mean J already committed. Fatal, cancel, shutdown and lost reply can also be Unknown; read current committed/applied state.
Only one local membership intent at a time, represented under existing Owners lock; no extra semaphore, persistent queue or unbounded recovery worker.
Lost local ownership on restart is reconciled from committed membership; automatic recovery changes no owner policy and removes no host.

## Build order, meaningful controls and frozen surface

Every slice needs a fresh bounded brief/allowlist and independent Sol reviewer; only coordinator merges after required green gates. No work authorized now.
1. TEST-ONLY next (after coordinator's main-green gate): new cfg(test) consensus/membership_contract_tests.rs plus module declaration; existing raw
   OpenRaft/mem_net APIs only. Prove effective!=committed rejects observation; interrupt future after joint commit then exact guarded append flattens;
   stale membership/leader/tail refuses without extra entry; preserve learners/metadata; Unknown replay observes one result. Mutation: drop each guard,
   choose wrong last config/drop learner; assertion must fail. Test double must exercise real engine states, not just a mocked success response.
   This proves API feasibility only. Multi-voter Windows cases require explicit ADR0024 classification; no live Windows engine run under exhausted budget.
2. Pure codec/apply slice later: membership.rs, enrollment.rs+codec/code/registry tests, state_store.rs/tests, log_store.rs/tests, consensus conversions
   and issue.rs/tests ONLY. Include arxj2 canonical bounds, tag6/schema refusal, deterministic log replay, anchor race, original-E replay after drift,
   size reservation, genesis mismatch. Mutations: omit apply fence, bind only key not anchor, return replay E, admit old tag2, omit reservation.
3. Owned-read/admission adapter slice later: consensus.rs and new consensus/admission.rs plus dedicated tests only. Held blocking read then caller
   timeout/drop/shutdown must retain both read+issue units/store refs; ninth/fifth pressure refuses. Mutation releasing permits at caller drop must fail.
   Test identity substitution, follower forwarding, address replacement/unsafe address, two concurrent intents, Remove-before-L and no catch-up permit.
4. Receipt/bootstrap verifier slice later: dedicated receipt.rs tests and pki/verify.rs tests, gated on custody adapter. Wrong sealed key, symlink/reparse,
   interrupted durable write, missing receipt, stale snapshot, snapshot past E with revoked N, Found-before-E, removed anchored key after fence refuse.
   Mutations: CA-only acceptance, withdrawal at Found, receipt without key, allow Vote/control on anchor, ignore snapshot metadata; proof-marker scan.
5. Transport/control integration remains BLOCKED on authenticated real transport, owner role and M2 key custody. Fake transport/clock can prove deny
   dispatch, CSR/proof gating and deterministic policy only. No real expiry, durable key isolation, LAN/Tailscale or production readiness claim follows.
   Trusted wall-clock/skew/fail-closed policy still needs security resolution before real expiry acceptance; existing constants and rollback guards stay.
For each later slice specify positive and refused outcomes before coding, run each named mutation once, restore exact bytes and record actual assertion.
Frozen across all slices absent separate authorization: Cargo/lock, service/executor, owner CLI, UI, main ingress, workflows, nextest, sampler, quarantine,
shared fixture bodies, retries, timeouts, permit counts and settings. Slice1 freezes ALL production behavior; production slices cannot borrow its permission.
This task performed static reads only: local briefs/review/ADR, source listed above, cached API management.rs:64-207, impl_raft_blocking_write.rs:163-339,
metrics/raft_metrics.rs:190-200, membership/change_members.rs:150-228 and membership_impl_raft_membership.rs:23-34. No build/test/probe/network/hardware run.

## Proposed ADR 0005 amendment text (not applied)

“L2 discovery hints travel in the same versioned join code; hints authenticate nothing. No-client-cert enrollment permits only bounded AnchorQuery and
Redeem. Bootstrap anchors authorize inbound replication and the joiner's own enrollment/admission only until its original Redeem position and binding
are applied, including validated snapshot crossing; they never authorize control. Deterministic apply fencing is mandatory, with no guard-only fallback.
Receipts preserve original Redeem position/anchor and require daemon-sealed key custody. Destructive membership recovery remains unavailable until
existing authenticated owner control is integrated. Exact committed-joint flatten preserves the recorded target and all metadata under engine guards.
These are proposed implementation boundaries, not production acceptance; M2 custody, real transport and trusted expiry policy remain open.”
