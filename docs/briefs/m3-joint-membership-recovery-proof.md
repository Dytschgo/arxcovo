# M3 guarded joint-membership recovery proof

Proposal only; Sol 6.1 high, 2026-11-17; no children. Root review required before implementation.
Dependent base `25bef7a6`; inspected local HEAD `4d970d20` includes later coordinator bookkeeping, preserved.
Read with [L2 proposal](m3-production-learner-admission.md), [root review](../reviews/m3-membership-decisions-root-review.md),
[ADR 0005](../adr/0005-raft-membership-and-enrollment-spike.md) and [ADR 0024](../adr/0024-windows-three-node-test-quarantine.md).
Owner D1/D2/D3 remain accepted; this supplies engine API evidence only, no production authority or new owner choice.
Root-confirmed main `3084f8e5`, CI44077792712 green: 509 s wall / Windows 505 s. PR169 routing CI pending, not merged.
No remote state queried here. M2 open, M3 unchecked; **unverified on real hardware**; Sol3/3, Opus0/2 unchanged.

## Cached source evidence and precise cancellation boundary

Paths below are relative to the cached registry packages, not an online API version:
- `openraft-0.10.0/src/raft/impl_raft_blocking_write.rs:200-210,328-339`: public
  `change_membership_if(edit,retain,preconditions)` and `append_membership(membership,payload,preconditions)` delegate inline.
- `openraft-0.10.0/src/raft/api/management.rs:69-207`: the first call_core sends ChangeMembership and awaits its
  complete-only responder. Only after that result AND unused_rx does this SAME future send AppendMembership for uniform.
  Step two preserves CommittedLeaderId, replaces LastMembershipLogId with J and removes LastLogId. No spawned two-step driver.
- `openraft-0.10.0/src/raft/raft_inner.rs:78-111`: send_msg awaits tx_api.send; call_core then awaits the response.
  `src/core/raft_core.rs:1888-1917` dispatches the already queued message independently of the caller's subsequent polls.
- Arx uses declare_raft_types defaults (`consensus.rs:13`; cached `raft/mod.rs:260`): TokioRuntime.
  `openraft-rt-tokio-0.10.0/src/lib.rs:96` and `src/mpsc.rs:40-55` use bounded Tokio mpsc, not deferred spawn/send.
- Locked `tokio-1.48.0/src/sync/mpsc/bounded.rs:816-824,1272-1283`: send reserves then synchronously enqueues.
  `sync/batch_semaphore.rs:397-461,598` acquires available capacity immediately, subject to cooperative budget.
  Therefore Pending ALONE does not universally prove submission: full channel or cooperative yielding can precede enqueue.
- Cached `raft/message/precondition.rs:64-101` checks committed vote, effective membership ID and full tail ID independently.
  `core/raft_core.rs:534-579` checks guards before validation/write; membership_state/mod.rs:207-246 requires committed
  membership and compatible transition. `membership/membership_impl_raft_membership.rs:23-34` uses quorum intersection.
  `metrics/raft_metrics.rs:190-200` distinguishes effective from committed membership; neither alone proves local apply.

Conclusion from source: a deterministic public-API seam exists under the scheduling/capacity conditions below.
Its successful runtime realization is UNPROVEN. A constructed joint entry proves only direct-transition compatibility;
it cannot earn aborted-two-step credit. This proof MUST obtain J from a real change_membership_if future and drop that
future before its second request. It does not model production Owners cancellation, shutdown, restart or leader failover.

## One engine-only test, no fixture or production changes

Add exactly `cluster::consensus::membership_contract_tests::interrupted_joint_exact_guarded_flatten`.
Use `#[tokio::test(flavor = "current_thread")]`, real ConsensusNode engines/storage and existing mem_net Factory/MemRouter.
Three private_temp directories, separately started nodes 1/2/3, same synthetic Init and Params::default(). Initialize ONLY
node1 with voter {1}; add real nodes2/3 as blocking learners sequentially through existing public APIs. Wait for apply.
Add metadata-only learner4 (`mem:4`, no running fourth engine) nonblocking; retain unique `mem:1` through `mem:4` addresses.
This fourth record tests preservation of a learner outside the promotion target; it grants no fourth-host policy or identity.
Choose the narrow engine contract: no assumed FounderTicket, registry/key fabrication, enrollment, TLS or production claim.
No Found is needed; empty application registry is intentional. Validate membership storage through the existing codec/state.
Use existing test adapters as-is; do not call three_node::Cluster, its retrying write helpers or reset diagnostics counters.
Acquire the existing APPLY_PAUSE_OWNER across the test to avoid other tests' global pause slot; do not install a pause hook.

1. Capture stable uniform U, current committed vote and full tail via public with_raft_state; ensure leader-term entry
   committed and effective == committed == applied membership (ID AND contents). Release every metrics/store borrow.
   Save U's real ID/tail for stale controls. No concurrent API writer; channel capacity is existing 1024, never changed.
2. Isolate nodes2/3 with existing router switches. Create and Box::pin ONE change_membership_if(AddVoterIds({2,3}),
   true,[LastMembershipLogId(U),CommittedLeaderId(current)]). Its target is {1,2,3}; old/new quorums require joint.
   Use std::future::poll_fn returning Ready(the inner Poll) to execute exactly ONE poll through
   tokio::task::unconstrained(future.as_mut()). No spawn, timeout wrapper, select loop or second poll of this future.
   On current_thread, the core cannot process this newly sent API request during this synchronous poll; its response
   cannot already be ready. Empty channel + unconstrained reserve excludes the pre-send Pending cases above.
   Require inner Pending; then park the Box without polling it. A wake only wakes the test task; it cannot poll the Box.
3. Let engine tasks run through bounded metrics waits. Require actual effective J=[{1},{1,2,3}], committed U, same
   leader epoch, J.index > U.index. This runtime observation confirms enqueue, not just an assumption about Pending.
   While isolated, guarded exact append using CURRENT J/leader/tail must return ChangeMembershipError::InProgress,
   with no added entry. This is the effective!=committed negative control, not a simulated observation object.
4. Reconnect2/3 without touching the Box. Wait effective == committed J, local_committed >= J and local applied >= J;
   decode applied membership and compare full J and nodes. Confirm all three engines apply J before recovery.
   Box remains owned and unpolled; require leader tail == J (full ID), proving no uniform entry was submitted.
   Drop the Box itself, not merely a Pin<&mut _>. Observe J still current through the following read/control barriers.
   Any Ready, missing J, early uniform, changed epoch or deadline is a failed seam: stop, preserve evidence, no fallback J.
5. From committed/applied J build Membership::new(vec![J.get_joint_config().last().unwrap().clone()], ALL J.nodes()).
   Never derive voters by union/first set or from intended IDs; do not discard metadata for nonvoters.
   Capture current full tail using with_raft_state(|st| st.last_log_id().cloned()), plus committed vote and membership.
   Fresh observation is test-local, not a new production helper or authority.
6. Run each refusal ONCE against the same otherwise-valid exact target, refreshing good observations before each:
   stale LastMembershipLogId = saved U; stale LastLogId = saved pre-J tail; stale CommittedLeaderId = current ID with
   an older term. The latter is deliberately synthesized stale epoch input, not evidence of an actual leader election.
   Change only that one guard; require the corresponding exact PreconditionFailed::*Mismatch with expected/actual
   fields, not any Err. Require unchanged full tail, effective/committed/applied J and all metadata after each.
   Wrong leader must reach the leader and yield CommittedLeaderIdMismatch, not ForwardToLeader or an unrelated refusal.
7. Submit exact flatten ONCE with all three fresh guards and EntryPayload::Blank; no retry or other membership intent.
   Require applied response F with index J+1 and exact last config {1,2,3}; learners exactly {4}, ALL addresses unchanged.
   Observe equal committed/effective/applied F on all3. Inspect retained membership log entries: exactly one J followed
   by exactly one F from this change. Decode persisted state through existing accessors, not a mocked response.
   Simulate lost reply only by discarding the already successful response value; status/readback must find F without
   another append or new future. This proves observation after reply loss, not durable operation-ID deduplication.
8. Shut down every node, remove router-held engine clones, assert clean existing shutdown reports and store release;
   no ignored cleanup error, background task or surviving directory handle. Retain diagnostics on assertion failure.

Direct constructed-joint control is deliberately excluded from this one-test slice. Public append of a caller-built
J followed by flatten could demonstrate transition support but would not exercise management.rs interruption. If the
parked future cannot produce J, report BLOCKED; do not replace step2 with append_membership(J) or alter a production helper.

## Deadline, platform classification and exact implementation allowlist

Before build, root settles this proposal's fixed entry budget: 75 s = fifteen existing 5 s operation windows:
startup, initialize, add2, add3, add4, mismatch observation, InProgress control, J convergence, three stale controls,
flatten, final readback, shutdown, plus pause-owner admission. One absolute Instant at test entry covers ALL fifteen;
each operation uses min(entry deadline, now+5 s). No independent restarted outer timeout, sleep-based seam or retry.
Params/default engine timers, production 5 s/1.5 s budgets and existing shutdown behavior remain unchanged.
If this cannot fit existing CI timing, stop for root disposition before implementation; never enlarge engine constants.

Owner's ALL three-node fixture tests Windows quarantine covers this new multi-voter proof even with isolated directories
and no Cluster constructor. Apply EXACT Windows-only REASON_FIXTURE from ADR0024; Ubuntu AND macOS execute it normally.
Extend guarded fixture inventory27 ->28, sample stays8. This is suite classification, not a fresh per-test repair allowance.
No live Windows run, --ignored run, diagnostic sample addition, counter reset or claimed Windows engine recovery.

After root review ONLY, exact allowlist for a later implementation:
- `crates/arxd/src/cluster/consensus/membership_contract_tests.rs`: new test and file-local test helpers only.
- `crates/arxd/src/cluster/consensus.rs`: one cfg(test) module binding only; no production cfg or helper alteration.
- `tools/ci/test_windows_cluster_quarantine.py`: separately named unsampled membership inventory, exact module binding
  and source checks; keep old inventories/reasons/proof-token checks intact. Source parser is a bounded guard, not Rust semantics.
- `docs/adr/0024-windows-three-node-test-quarantine.md`: additive classification28/sample8 and coverage-loss note only.
- `docs/PROGRESS.md`: coordinator records provider/effort/outcome, checks, blockers and last merged PR accurately.
Preserve every old test body, shared fixture, mem_net, adapter, engine, Cargo/lock, nextest, workflows and sampler bytewise.
This delivery edits ONLY this brief; coordinator retains progress updates. No source or architectural deviation is implemented.

## Controls and bounded evidence required later

Guard suite: one positive pass and each changed-input mutant once; assert mutant bytes differ before expecting failure.
Mutants: remove/rename/duplicate test; wrong reason; absent Windows gate; Unix/all-OS/global ignore; missing/wrong cfg(test)
module binding; extra hidden test; inventory omission; classifying new proof as sampled; sampler count/filter drift.
Pin exactly28 Windows ignored fixture cases, original8 sample matches, and new ID Windows-ignored/Ubuntu+macOS-required.
Required old pure/single-node TLS/founder tests stay required everywhere. Token checks never substitute for engine assertions.
Runtime positive and four negative controls above run once per normal required Ubuntu/macOS job; no repetition loop.
Named behavior mutants for supported-runtime follow-up: omit each of the three guards (each exact refusal must fail),
choose first instead of last config (voter equality fails), drop learner4 (metadata equality fails), poll Box after J
(parked-J/no-uniform assertion fails). Never mutate cached dependency or production code. Restore exact bytes afterward.
No supported local runtime is established here: these mutants remain unexecuted requirements, not invented Linux credit.
Root must arrange a supported-runtime mutation check separately; no extra hosted mutant pushes are authorized by this brief.

Windows later permits locked/offline compile, Clippy, fmt, static guard tests and nextest LISTING ONLY for this proof.
First engine execution is normal PR CI on required Ubuntu/macOS, after independent review and fresh green-main verification.
No workflow change or opportunistic local Linux claim. Natural red stops the round; no rerun/push-to-green, filter, ignore,
deadline change or fourth Sol repair. Existing main/PR all-five-gates, <=660 s Windows and coordinator merge rules remain.
No push/PR/status/merge/watch/link/admin/VM/install/cache deletion in this design task. Root review precedes any build.

Readiness: source-supported seam; runtime proof and mutations pending; build authorization pending root review.
If runtime needs a production helper, changed engine constants or unavailable public API, STOP and report before prescribing
that build. B1-B5, authenticated transport, owner removal role, M2 key custody and production recovery remain blocked.
The L2 parser can recognize IPv4 limited broadcast; subnet-directed broadcast needs subnet context and is outside this
proof. Syntax alone cannot claim every broadcast is rejected. No new parser implementation or owner decision here.
Actual work: local source/docs inspection and brief whitespace/scope/line-count checks only; no build/tests/network/runtime.
