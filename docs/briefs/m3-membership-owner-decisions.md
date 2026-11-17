# Brief: M3 membership owner decisions D1-D3 (L2 and promotion)

Status: **D1-A, D2-A and D3-A selected by owner; technical contract incomplete** (Opus 5.5 high, 2026-11-17), docs only; nothing built, run or fetched. Base `3bbfb309`;
root review [m3-membership-decisions-root-review](../reviews/m3-membership-decisions-root-review.md) of proposal `59c68332`. Scope is L2
and promotion; the test-only L1 in [m3-learner-admission](m3-learner-admission.md) needs none of it. The owner decisions are recorded in ADR 0005
on 2026-11-17. No production source or build is authorized by this brief; L2 and promotion remain blocked. Production join security is **not closed** until
root re-review, an owner ADR 0005 amendment, L2, independent review and the controls below. M2 open, M3 unchecked; **unverified on real
hardware**.

## Source facts this rests on

- `JoinCode` (`enrollment/code.rs`) is `arxj1-` + hex(cluster 16 | code 16 | secret 32 | ca_fp 32 | check 4), 206 chars. It carries no
  member key or endpoint address. `MAX_CODE_TEXT` is 4096. Only the minting host has the plaintext. The cluster stores only `CodeVerifier`.
- `enroll()` (`consensus/issue.rs`): one of 4 `issue_permits`, a 5 s `ISSUE_BUDGET`, `propose_response` (64 permits, an `Owners`-spawned
  task that holds its permit until the engine replies), then `issue_guard` fresh reads (1.5 s `READ_BUDGET`, 8 permits) before signing and
  again before release. The ticket knows the Redeem commit index (`IssueTicket.index`). `shutdown()` closes `Owners` and allows 5 s in total.
- Registry (`enrollment/registry.rs`): `MAX_MEMBERS` 16. `remove` is permanent and node IDs are never reused (`next_node_id`), so a removed
  `(node, key)` binding never returns. Redeem replay with the same request and key returns the same N, or `Revoked` once N was removed; a
  new request on a consumed code is `Consumed` (`enrollment.rs`).
- `StateStore::apply` (`state_store.rs`) applies enrollment commands and membership entries into one image in log order, so the Redeem
  apply at E sees the applied membership blob and registry exactly as of E-1. The blob is opaque there today.
- OpenRaft 0.10.0 (cached, read-only): `Precondition::{CommittedLeaderId, LastLogId, LastMembershipLogId}` exists only on membership APIs;
  `RaftMsg::ClientWrite` carries only `expected_leader`, so Redeem cannot be guarded by `LastLogId`. `AddNodes` adds learners; one added
  voter needs no joint entry; `add_learner` has no precondition form. See the response section for line evidence.

## D1 - joiner pre-catch-up trust anchor (security; amends ADR 0005)

**Threat, judged against the actual code format.** The joiner authenticates the enrollment endpoint only by `ca_fp`, so any leaf that
chains to the CA passes. A network attacker without the CA key can block the join but cannot complete TLS. An attacker who holds the CA key
(ADR: "a stolen CA key alone cannot impersonate a member") can mint a leaf for any node ID with its own key. Under the earlier reply-only
O2 that attacker receives the bearer proof (and redeems it with its own CSR) and returns forged anchors, so the joiner's whole state is
attacker-written. LAN and Tailscale are treated the same. **Reply-only O2 is withdrawn.** O3 (CA chain only), O4 (a second owner-pasted
pin) and any PAKE or new primitive stay rejected.

**Option A - voter-set anchor bound into the same code (recommended).**
1. *Code revision `arxj2`.* `arxj2-` + hex(cluster 16 | code 16 | secret 32 | ca_fp 32 | anchor 32 | check 4) is 270 chars. `anchor =
   SHA256("arxcovo/join/anchor/v1" | cluster | count u8 | sorted (node u64 BE | key digest 32)*)` over the **bound voters**: the voters of
   the applied uniform membership in the minting leader's fresh read, each with its registry binding. Same domain-separated SHA-256 as the
   code; no new algorithm, dependency or second owner transfer. Minting refuses (`RetryLater`) in a joint config, when any voter lacks a
   binding, or when the set is empty. The anchor is not secret; it is not in the Mint record, only in the joiner's Redeem entry (step 3).
2. *Bounded pre-proof anchor query.* On the CA-pinned TLS channel with no client certificate, the joiner first sends `AnchorQuery{cluster,
   code_id}` (fixed 32 bytes). The endpoint holds one of the existing 4 `issue_permits` only for the read (else uniform `Busy`), reads only
   local applied state (no proposal, fresh read or log write) and answers with at most 1 + 16 x 40 = 641 bytes of bound voters
   while that code is pending and unexpired; otherwise one uniform refusal of fixed shape. One query per connection, 5 s budget.
   Disclosure: voter IDs and key digests (each voter's leaf is shown to any TLS client anyway) to a holder of the 128-bit `code_id`. The joiner requires
   `digest(list) == code anchor` **and** that the server leaf's CertificateVerify-proved `(node, key)` is in the list; only then does it send
   proof + CSR + anchor digest on the same connection. A CA-key-only attacker cannot present an anchored key, so the proof never leaves the
   joiner. A stale view gives a mismatch and the joiner tries the next address.
3. *Atomic issuer fence at Redeem apply (resolves root finding 1).* `enroll` pre-checks the digest against bound voters in a fresh read as a
   fast fail (`AnchorChanged`, code unspent). That pre-check is not atomic: a membership change or `Remove` can land before E. The
   authoritative check is therefore in the replicated Redeem itself: `Redeem` (codec v2) carries the digest, and its apply recomputes the
   bound voters from the applied membership and registry at E-1. A mismatch, a joint config or an unbound voter is a deterministic
   `Rejected(AnchorChanged)` that consumes nothing, so the code stays pending. Because the anchor is proven at E, the post-commit
   `issue_guard` reads no longer compare anchors and cannot refuse for anchor drift; their existing checks (leader, N still bound) and the
   issuable-time and clock checks remain. Feasibility needs deterministic decode of the membership blob inside apply, which L2 must
   prove; if it cannot, the fallback is the guard compare plus the honest consumed-code recovery in step 7, never an unspent claim.
4. *Receipt binding.* The reply is `{leaf, node N, Redeem index E}`. The joiner accepts it only if the leaf chains to `ca_fp` and carries its
   own CSR key, the cluster matches, and N is not in the anchor. Then one atomic durable write outside `EnrollmentState` (never log,
   snapshot, MCP or evidence) stores `{cluster, N, leaf, anchor list, E}` in its private store dir with reparse checks. Its server accepts
   anchored peers only after that write.
5. *Limited anchor role, closed list.* While anchors are active the joiner's server verifier is `CA chain AND (own applied registry OR
   anchor list)`. An anchor-only peer may send only inbound `AppendEntries` and `InstallSnapshot`; Vote, forwarding, control, execution and
   MCP are refused. Outbound, anchors may only authenticate the server of the joiner's own `Redeem` (step 2) and own `admit_learner` (D2).
   Both requests carry no secret beyond what step 2 already gated, and the joiner treats every reply as a hint: its progress is judged only
   from its own applied state. Anchors never authorize owner operations or any other outbound call.
6. *Applied fence and withdrawal.* Delete the anchors (durably, idempotently) only when `applied >= E` **and** `authorize_peer(cluster, N,
   own key) == Ok` locally. Step 3 makes every anchored binding present at E-1; removal is permanent and IDs are never reused, so each is
   in the joiner's registry at E and the joiner cannot be stranded on an anchored leader. Withdrawing at `Found` is wrong: a leader whose
   binding follows `Found` in replay would be refused and the joiner would stall.
7. *Code outcomes, stated honestly.* Refused by the step 2 query, the pre-check or the apply fence: unspent; mint a fresh code only if the
   owner wants to. Redeem applied but release then fails (guard refusal, deadline, crash, engine `Unknown`, cancel): **consumed**, with
   binding N. A joiner still running retries with the same request id, CSR key and proof; replay returns the same N unless `Revoked`. A
   joiner that restarted has neither, gets `Consumed`, and N is an orphan using one of 16 slots until removed (see Blockers B2).
8. *Genesis, back-compat and cold restart.* The joiner opens only `founder_init(code.cluster())` (`first_free` 2); a store with another
   cluster or `first_free` refuses to open, like F1 `FounderAllocation`. An L2 joiner refuses `arxj1` ("minted by an older version; mint a
   new code"); no released cluster exists. On restart: receipt present and pre-fence, reload anchors; past the fence, delete them;
   unfounded with no receipt, refuse "enrollment incomplete".

**Option B - minter pin in the code.** One digest of the minting voter's `(node, key)`; that host is the only endpoint and replicator until
the fence. Simpler, but the join strands if the minter loses leadership or goes down before catch-up. **Option C - keep L2 blocked.**
Residual risk (all options): a holder of the pasted code can redeem first (the existing 15-minute single-use bearer risk). CA key plus MITM
can deny a join. A stolen voter private key is member compromise. An anchored binding removed after E is honoured until the fence.

**Proposed sentence (A):** "Join codes are revised to `arxj2` and bind a SHA-256 anchor of the minting leader's bound voters. The joiner
releases its redeem proof only to an endpoint whose TLS key is in an anchor list matching that digest, and the Redeem applies only if that
anchor still matches. Until it has applied its own Redeem index and binding, it accepts inbound replication alone from those voters, and
then durably discards them."

## D2 - membership operation API and budgets (latency and product semantics)

**Option A - fast bounded self-admission, then engine-owned catch-up with status reads (recommended).**
- *Authorization (resolves root finding 2).* `admit_learner{addr}` is self-admission only. The caller is the joiner over mTLS with its new
  leaf; the server derives `N` and key from the verified leaf and requires `authorize_peer(cluster, N, key) == Ok`. The request carries no
  node ID, so no member can name another node. `addr` is bounded (<= 255 bytes, `host:port`), is only a dial hint, and grants nothing: every
  dial still verifies the peer's leaf against the registry. Leader only: a follower returns `NotLeader(hint)` and does **not** forward,
  because a forwarder would become the authenticated caller. The joiner redials and checks the leader against anchors (D1-A5).
- *Pre-check and proposal.* One fresh read: N bound, N not a voter, no other bound learner below catch-up, nodes <= `MAX_MEMBERS`. Then one
  of the existing 64 proposal permits in an `Owners` task runs `change_membership_if(AddNodes{N: addr}, true,
  [LastMembershipLogId(observed)])` and returns at commit: `{N, membership index L}`. Suggested budget 5 s, the `ISSUE_BUDGET` value.
- *Replay.* If N is already a learner or voter with the same `addr`, return the existing L (`replay`). A different `addr` is refused
  (`AddressConflict`); v1 has no address change, and identity replacement is remove plus fresh enrollment.
- *Unknown, cancel, shutdown.* Caller deadline or engine error gives `Unknown`; the `Owners` task keeps its permit until the engine replies
  or `shutdown` aborts it within 5 s. Dropping the caller cancels nothing. The joiner resolves `Unknown` by replaying its own admit. A
  closed `Owners` gives `Stopped`.
- *Race after the pre-check.* `LastMembershipLogId` serializes membership intents but not registry entries. A `Remove N` that lands before L
  leaves a learner N without a binding. It fails closed (it cannot authenticate anywhere) and is reported as `AdmittedButRevoked`. It counts
  toward `MAX_MEMBERS`, is excluded from the one-learner rule and from promotion, and is **not** automatically removed (Blockers B2).
- *Catch-up* is OpenRaft's own replication task: no custom task, no blocking wait. `learner_status(N)` on the leader is a non-blocking
  metrics borrow (matched against commit and L) and takes no permit. `join_status()` on the joiner is local (applied vs E, anchors state).
- *New capacity: none.* 64 proposal permits, 8 read permits, 5 s shutdown, at most one bound learner below catch-up.
- *Product numbers for owner review (not approved):* progress with no hard deadline while it advances; `Stalled` after **60 s** without
  progress in matched or applied; retry or remove offered; **no** automatic removal.

**Option B - blocking admission until catch-up** under a fixed deadline (for example 120 s): a new budget and capacity class, a permit held
for minutes, cut by the 5 s shutdown. Not recommended. **Option C - admission inside `enroll`**: one round trip and no outbound anchor use,
but a membership effect on the unauthenticated enrollment path inside the 5 s issue budget.

**Promotion (for D3).** An automatic leader-internal policy action, not a peer RPC; no member can request it. `promote(set)` reuses the 64
permits, 5 s and `Owners`. Fresh read: every target bound, a learner, and matched >= leader commit. Then `change_membership_if(
AddVoterIds(set), false, [LastMembershipLogId, CommittedLeaderId])`. Read-only evidence: OpenRaft does not refuse or wait for a lagging
target; the joint entry simply needs a quorum of both configs, unbounded inside the engine, so the 5 s budget yields `Unknown`. The flatten
step is driven by the API future, so a shutdown abort after the joint commit leaves a committed joint config. **Post-commit race:** a
`Remove` of a target after the read cannot undo a committed promotion. The result is `PromotedButRevoked`: a voter that cannot authenticate,
which fails closed and counts as an offline voter for quorum. No compensating `RemoveVoters` is promised or automated; cleanup needs the
removal contract (B2). Joint reconcile (re-propose the flatten from the current joint state, never a forced reset) is a candidate API
recovery with no runtime proof (B4). New codes are refused while joint (D1-A1).

**Proposed sentence (A):** "Learner admission is a bounded (5 s) idempotent self-admission by the authenticated joiner on the leader that
returns at membership commit, using the existing proposal permits and `Owners`. Catch-up proceeds in OpenRaft replication and is observed
through non-blocking status, and a join with no progress for 60 s reports `Stalled` without automatic removal."

## D3 - promotion policy (ADR 0005 "Quorum and unresolved choices")

**Honest availability.** KICKOFF promises that "any host can go down ... while the others keep working", and M3 acceptance asks for a
3-node cluster that survives one offline node. Only three or more voters provide that. Neither option gives two hosts HA.

**Option A - learner until three (recommended).** Host 2 stays a learner. When host 3 is caught up, one guarded joint change takes voters
from {1} to {1,2,3}; both learners must be caught up. With two hosts, writes need host 1 only: host 2 can go down freely, host 1 down stops
writes, and an acknowledged write is on one disk until replicated (normally immediate, not guaranteed), so losing host 1's disk can lose
acknowledged writes. Codes minted earlier become stale at the promotion (D1-A7, unspent).

**Option B - promote host 2 at once (two voters).** Every acknowledged write is on two disks, but writes need both hosts, so either host
going down stops writes: worse availability than A for the same host count. Host 3 then makes three voters.

**Hosts 4+ (suggestion only):** odd voter counts; host 4 waits as a learner, host 5 promotes 4 and 5 together, beyond 5 stay learners.
**Policy versus workflow:** a technical rule. Once conditions hold, the leader promotes automatically; the pasted code is the owner's only
action, with no approval step. Operator `promote`/`remove` exist only for recovery under the separate contract (B2), never as a gate.

**Proposed sentence (A):** "A second host joins as a learner; when a third has caught up, both are promoted automatically to three voters
by one guarded membership change; one- and two-host clusters cannot write without host 1."

## Recovery and rollback (for later L2)

Unspent code (refused before or at Redeem apply): optionally mint fresh. Consumed code with no receipt: orphan binding N; removal per B2,
then a fresh code. No proof or secret is stored at rest for retry. Stalled learner: wait for a leader it can authenticate, or remove it
(membership removal then registry `Remove`, under B2) and re-enroll under a fresh identity. Interrupted joint config: B4. Bad `arxj2`
rollout: revert the code; there are no released clusters to migrate. Residual liveness gap: if leadership moves to a just-promoted voter
before a lagging learner applied that voter's binding, the learner fails closed until leadership returns or it re-enrolls.

## Negative controls (later L2 tests; each with an inject/remove mutant)

1. Fake endpoint with the real test CA but no anchored key; anchored node with another key; tampered anchor (checksum recomputed); list
   with an entry added or removed: all refused, and the proof bytes never reach the endpoint socket. Unknown or expired `code_id` and an
   over-size query get one uniform refusal; a fifth concurrent query gets `Busy`.
2. Bound voters change between pre-check and Redeem (injected pause): apply returns `AnchorChanged`, the code is still pending, nothing is
   signed; the mutant without the apply fence consumes the code. A Remove after E does not cause an anchor refusal.
3. Anchor-only peer sends Vote, forward, control, exec or MCP: refused; a CA-valid unanchored leaf is refused before the fence.
4. Strand: founders {1}, leader 3 whose binding follows `Found` in replay: catch-up succeeds; the withdraw-at-`Found` mutant stalls.
5. After the fence anchors are gone on disk, and an anchored voter whose binding was removed after enrollment is refused.
6. Crash after applying E but before deletion: deleted on restart. Missing receipt while unfounded, `arxj1`, wrong cluster or
   `first_free` != 2: refused. Redeem `Unknown` then same-request replay: same N; new request: `Consumed`.
7. Admit timeout then replay: one membership entry. Different `addr`: `AddressConflict`. Leaf of node M asking while N is pending: only M
   is considered, never N. Follower: `NotLeader`, no forward. Two concurrent admits: one `RetryLater`. Remove before L: `AdmittedButRevoked`.
8. Promotion: lagging learner refused by the pre-check; Remove after the read gives `PromotedButRevoked` with no automatic removal; of two
   competing promotions one fails its precondition; abort after joint commit leaves joint and new codes are refused.
9. Receipt and anchor files, logs and evidence contain no proof or secret markers.

## Historical owner question (all three recommended options selected 2026-11-17)

"Three product choices before production multi-host join; none is adopted yet. (1) Join security: the join code gets 32 bytes longer and
fingerprints the cluster's current voting hosts, so a stolen CA key alone cannot fake the cluster to a new host. A code is rejected unused
if voting hosts change first; if a join fails after the code was accepted, the code is used up and you paste a fresh one. (2) Join speed:
Add host returns within about 5 s, then shows catch-up progress, and reports Stalled after 60 s without progress. Nothing is removed
automatically. (3) Two hosts: host 2 stays a non-voting copy until host 3 arrives, then all three vote; until then writes depend on host 1
and its disk. The alternative is two voters that both must be up. Recommended: (1) A, (2) A, (3) A. Not decided here and still blocking
production join: how a new host finds the cluster's address, who may remove a host, and recovery from an interrupted membership change."

## Remaining implementation blockers (root/technical, not owner choices)

- B1 Endpoint address source: `arxj1` has none. Unauthenticated hints in `arxj2` would be safe under D1-A (anchors authenticate), but no
  format is chosen. L2 production join stays blocked on it.
- B2 Removal authorization contract: who may remove a learner or voter (`RemoveNodes`/`RemoveVoters`, then registry `Remove`), from which
  path, with quorum checks and failure behaviour; covers orphans, `AdmittedButRevoked`, `PromotedButRevoked`, stalled learners. Peer mTLS
  membership never authorizes it.
- B3 Apply fence feasibility: deterministic membership-blob decode in `StateStore::apply`, Redeem codec v2, and the anchor-free guard.
- B4 Joint reconcile: which OpenRaft call flattens a committed joint config left by an aborted future, and a runtime test of it.
- B5 ADR 0005 amendment text: enrollment endpoint allow-list is exactly `AnchorQuery` and `Redeem` for no-client-cert callers; anchors as
  in D1-A5; everything else stays denied.

## Response to root review 3bbfb309 (evidence)

- Finding 1, corrected: the unconditional unspent claim is withdrawn. D1-A3 moves the anchor check into the deterministic Redeem apply;
  D1-A7 states consumed outcomes and the orphan path. Evidence: cached `core/raft_msg/mod.rs:78-84` (`ClientWrite` has only
  `expected_leader`), `raft/message/write.rs:57-58` ("PreconditionFailed should not occur for normal writes"), local `state_store.rs:403-423`.
- Finding 2, corrected: D2 self-admission derives N from the leaf, carries no node ID, never forwards; replay and `AddressConflict` are
  per-node; removal and promotion authority are separate (B2, leader-internal).
- Finding 3, corrected: the owner message lists only product choices and names B1, B2, B4 as undecided. Pre-proof query bounds (D1-A2)
  and the closed outbound anchor role (D1-A5) are resolved; no compensation is promised. Evidence: `raft/api/management.rs:130-185` drives
  the flatten step from the caller future and re-guards it with the joint log id; `impl_raft_blocking_write.rs:163-205` (joint left
  committed); no learner-lag check found in `change_membership`. All read-only API evidence, not runtime proof.

## Owner decision checkpoint - 2026-11-17

Selected: fingerprint current voters; bounded admission then progress; host 2
non-voting until host 3. ADR 0005 records the accepted requirements and the
founder-disk durability tradeoff. The historical question above remains evidence,
not a pending request. B1-B5 and root read-ownership requirements remain open;
none of the unverified API/schema details is accepted solely by these selections.
