# Owned anchor query: root contract disposition

Root Sol 6.1 medium, 2026-11-17. Static review of3502c69/retained4f9c86de
against actual current semaphore, Owners, local-state and shutdown source.
No implementation or runtime authorization. Main CI44126173935 is still pending.

## R1: narrow ownership representation exception accepted

Root accepts exactly two production representation edits in consensus.rs:
issue_permits: Semaphore becomes Arc<Semaphore>; its existing start initializer
wraps the same Semaphore::new(issue::MAX_ISSUING) in Arc::new. No capacity,
admission order or existing enroll/found semantics change. Their borrowed
try_acquire calls continue through Arc deref. All other start/read/shutdown
code remains byte-identical, apart from the new module binding. This closes
the technical freeze conflict without an owner product decision. New query
work retains both owned issue/read units in its blocking closure as specified.

## R2: mutation evidence must have an executable local seam

The proposal asks ordinary green hosted CI to supply compiled mutation kills
but specifies no way to execute intentionally failing mutations there without
extra pushes/runs. That is not a concrete feedback path. Do not add a bespoke
workflow, failure-seeking hosted push or secret runtime mutation switch.

Use a test-only local durable query harness, separate from the new engine proof.
Construct a ConsensusNode inside the new descendant test module with raft=None,
a real private-directory StateStore, actual shared semaphore/Owners fields and
real state-store weak/release handles. Initialize its synthetic committed state
through StateStore.apply, not direct record insertion. It owns no Raft engine;
its log side may be absent/already released. Label EngineEnd::Absent honestly.
No production constructor, fake ticket, joiner seeding or signing authority is
added. This harness exercises the actual anchor_query and actual shutdown owner
path on a real blocked store lock; it is not a three-node or engine scenario.

M1 runs once locally against this harness: preserve the actual blocking closure
and store Arc while moving only its read unit to the async owner. Actual shutdown
abort must expose the early permit return at the designated read-held assertion.
Keep issue unit ownership unchanged. Confirm intact closure-entry synchronization
and blocked-store setup before that assertion so scheduler luck cannot pass it.

M2 runs once locally against the real query plus pure TLS harness: synthetic CA
leaf key matches the actual committed voter binding, a consumed code is queried
using received query32, and any returned list takes the normal gate/proof path.
The SAME server-plaintext zero-proof assertion must fail with proof32 when only
consumed eligibility is omitted. Under intact code assert uniform Refused too.
No early assert/return on query result may hide that primary oracle. This proves
the local query boundary, not authenticated replication or actual issuance.

Allow these additive durable/non-engine async tests in the new test file. They
may execute on local Windows. The separate ONE-start actual-founder pipeline
remains required all OS and hosted-only; its body need not repeat all ownership
phases already proven by the local harness. Both paths use the same production
query. Keep the engine case's cooperative40s/5s cleanup reserve and all existing
inventories; no new engine start for a mutation. Exact list/count classification
must separate pure/durable ownership cases from the dedicated engine case.

## Other clarifications and disposition

Use the existing READ_BUDGET deadline for this read-only hint; being below the
issue5s envelope does not require a new timer or access to issue's private
ISSUE_BUDGET constant. Caller timeout/drop cannot release either submitted unit.
All eligibility/time checks are local advisory observations with no real-time
expiry/freshness/unspent promise. E-1 consumption fence and pre-proof key check
remain authoritative at their separate boundaries. Do not broaden public access.

Independent contract recheck must confirm the test-only construction, actual
shutdown EngineEnd::Absent path and both designated failure oracles are feasible
within the allowlist. Source remains unauthorized until that review and green
exact main; root sets the bounded verification budget afterward. No new owner
approval flow, repair counter reset or hardware acceptance. M2open/M3unchecked;
hardware **unverified on real hardware**.
