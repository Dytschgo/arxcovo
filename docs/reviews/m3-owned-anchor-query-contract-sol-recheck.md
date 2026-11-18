# Owned anchor query: independent Sol contract recheck

Sol 6.1 high, 2026-11-17; independent reviewer, no children. Static PASS
with implementation advisories below; no contract blocker found.

Reviewed prepared HEAD 10309ef4ee892160fc7dedaf899c31970e61c62e against
source main 3022d601f09f24b0e5f092a33f2e9343b500471d. The local comparison
changes only PROGRESS, the query brief and root review; source is unchanged.
Original proposal 3502c69, retained as
4f9c86de77239f49e08bad56977fa9a496ffe4fb, is historical. Normative
`m3-owned-anchor-query-root-review.md` R1/R2 overrides its initializer blocker,
hosted mutation route and repeated engine ownership phases.

## Standards

Blocking: none. Read AGENTS, KICKOFF, CONTRIBUTING, ADR0005 including both
kernel amendments, query brief/root review and dytschgo-review skill/profile.
Current owner Sol routing controls historical cross-provider instructions.
No baseline smell requires abstraction or unrelated refactoring here.

Allowlist is sufficient: new consensus/anchor_query.rs and descendant
anchor_query_tests.rs; enrollment/anchor.rs pure method/refusal/tests; additive
query inventory controls in test_windows_cluster_quarantine.py. In consensus.rs
only module binding, issue_permits: Arc<Semaphore>, and Arc::new around the
existing MAX_ISSUING initializer. Existing try_acquire borrows in issue/found
continue through Deref. Capacity4 and all other start/read/shutdown logic freeze.
No StateStore/helper, pki/preproof, transport, Cargo, workflow, timeout or retry
change is needed. This review edits only this document.

Advisory: pin BOTH module bindings, including the cfg(test)/path declaration
inside the new query module, and guard attributes on that module and its helpers.
Classify new durable async non-engine tests separately from QUERY_REQUIRED's
one all-OS engine test. Negative controls must reject omission, rename, ignore,
platform hiding and hidden helper/parent bindings, and inspect start calls in
helpers as well as the test body. Preserve fixture28/sample8/founder13/TLS14,
atomic-issuance1/anchored-proof1 and every existing guard assertion.

## Spec

Blocking: none under root R1/R2. The actual APIs support the contract:

- EnrollmentState's child anchor module can read founded/cluster/committed_ms/
  records, add pending_voters and a cluster-visible static unit refusal without
  exposing fields. Its private module need not be re-exported: callers infer
  the refusal and map it uniformly. Keep method/error visibility at most
  pub(in crate::cluster); AnchorQueryError lives in the new module with the
  query method. An internal module binding can provide its named error path.
- StateStore::state and membership borrow one store image. Decode the existing
  <=4096 blob under the same guard, then observe/encode existing BoundVoters
  (1 + 40*16 = 641 maximum). No new store API or membership decoder is needed.
  Missing membership refuses; decode/lock failure is Unavailable. adapter::blocking
  accepts Result<T, StoreError>; use T = Result<Vec<u8>, AnchorQueryError> to keep
  eligibility Refused distinct from adapter failure, without leaking StoreError.
- Pure eligibility is founded/matching cluster, exact pending unconsumed record,
  bounded now, checked now+skew >= committed, effective=max(now,committed), and
  checked effective+2*skew < expiry. Equality refuses. No mutation or prune;
  compare snapshots/state before and after success and refusals. Keep six pure
  test functions with tables and independent literal list bytes.
- Descendant Rust privacy permits constructing all ConsensusNode fields and
  SmAdapter's tuple. Open a private durable StateStore; sequential actual apply
  calls at indices starting0 establish canonical Membership, Found, fresh Mint
  and (for M2) Redeem. Check every Outcome. Init first_free2 permits founder1.
  Build real Arc/Weak/Released, Owners and proposal64/read8/issue4 fields;
  raft=None. For absent log, release_pair plus dropping its ReleaseAck sets the
  actual flag; use an empty typed Weak. No fake ticket, seeded joiner replica,
  production constructor or seal implementation is needed.
- Actual shutdown accepts raft=None as EngineEnd::Absent, still closes Owners,
  joins/aborts owners, takes sm and waits for Released. This exercises the actual
  query closure/store-release path without an election or Raft engine. Released
  means preceding destructors completed, not proven SQLite/OS close success.

Implementation advisories for the runnable proof:

1. Register under Owners without await between closed check and spawn. Acquire
   owned issue then read, returning issue on read admission failure. Check caller
   loss/deadline before submission. Move store Arc AND both units into the actual
   blocking closure, then await adapter untimed. Nest computation so lock guard
   drops even on errors, then drop Arc, read, issue, then complete. Avoid `?` that
   exits before explicit release order. Caller timeout covers only oneshot receive;
   sample READ_BUDGET1500 entry deadline before synchronous clock sampling. No
   private ISSUE_BUDGET access, second timer or new cleanup promise is needed.
2. Synchronize actual closure entry BEFORE its mutex lock, while a joined gate
   thread demonstrably holds that lock. Existing per-node cfg(test) read_blocking
   counter can supply this signal from the new closure; admission alone or yield
   is insufficient. Drop borrowing caller futures before &mut shutdown. Account
   for the gate Arc in baselines; let hooks retain no store Arc. Reap Owners when
   asserting completion and release all helper Arcs before weak-expiry assertions.
3. Four dropped callers must retain issue0/read4 and four store owners; fifth Busy.
   Preholding read8 must return issue4 immediately. A real 1500ms timeout retains
   issue3/read7 until closure exits. Blocked shutdown takes its existing5s, returns
   Orphaned/EngineEnd::Absent with read_held1, and still retains issue3/read7 after
   owner abort. M1 moves ONLY read into the async owner, held across its await;
   actual post-abort read.available_permits()==7 must fail as8. The report's
   read_held alone cannot kill M1: it is sampled BEFORE abort in both versions.
   Release/join the gate and await both units/store release before propagating
   the saved failed assertion, including in the mutant. Closed Owners must refuse
   a subsequent valid query without another blocking submission.
4. M2 needs no access to private TLS Fixture/plaintext helpers. Construct synthetic
   CA/leaf in the new test file using existing rcgen patterns, with leaf SPKI
   digest committed as founder voter through actual apply. Use public-in-cluster
   ProbeTime/server/Driver and bounded ServerConnection::reader reads. Drain and
   verify received query32, call the REAL query on those bytes, and send ANY
   returned list through read_anchor/accept_anchor/release_proof on that same TLS
   session. Drain actual server proof plaintext BEFORE asserting query refusal.
   Omitting ONLY consumed eligibility then returns the unchanged voter list,
   passes the gate and yields proof32, failing the SAME zero-proof assertion;
   intact code yields zero and uniform Refused. No callback/token oracle or early
   unwrap/assert on expected refusal. This proves local query gating, not issuance.
5. Separately retain ONE-start hosted all-OS real founder/Mint/query/TLS/enroll
   case using anchored_join_tests APIs, not its cached list. Cooperative35s work
   plus5s cleanup fits a40s envelope only with bounded helper waits and retained
   handles. Pure/durable tests need no new engine start; mutants run there once.
   No universal startup/panic cleanup promise follows: unresolved work is unknown,
   never PASS. Poisoned Owners/store locks, cancellation before submission, and
   elapsed deadlines need explicit no-work/resource-release assertions.

## Security

Blocking: none for this internal hint. Bounds check query32 before copy, clock,
admission or allocation; sample trusted SignerClock once and move only u64.
Static Refused/Busy/Unavailable must not include input, nested errors or lists.
The code identifier is not bearer proof, member authority or an owner credential.
Local applied pending/voters may be stale versus current cluster consumption or
membership; snapshot coherence proves no freshness, unspent code or real-time
15-minute expiry. Code anchor, same-session TLS key possession, E-1 first-redemption
fence and both signing guards remain authoritative at their separate boundaries.
No listener, discovery, key custody or public deny-by-default policy is accepted.
Normal RAII unwind is required; arbitrary runtime failure is not bounded cleanup.

## Evidence and source authorization readiness

Actual here: local file/diff/privacy/API inspection, clean initial worktree and
cached Tokio1.48.0 source matching Cargo.lock (owned permits, JoinSet abort/join,
spawn_blocking non-abortability after start). Also inspected a newer cached Tokio
copy, then checked the locked version; no newer-version inference is required.
No Rust/Cargo/test/engine/mutant/build or network/public action ran. The proposed
harness, semantic kills and timing remain UNRUN; static feasibility is not a pass.

Reused with attribution: root reports CI44126173935 pending on macOS,
Ubuntu/Windows green and Windows553s. Not independently fetched; no exact-main
success or verification-budget credit. Prior PR174 proof evidence in PROGRESS
is historical, not evidence for this query. Source readiness: contract recheck
complete, but implementation remains unauthorized until root confirms fresh green
exact main and sets the bounded verification budget. Preserve stop-at-first-red,
once-only mutants/exact restoration, five gates, Windows<=660s, fresh-main and
independent exact-head source review before coordinator merge. No new owner
approval flow or repair attempt. M2 open/M3 unchecked; Sol3/3 and historical
Opus0/2 unchanged. Hardware **unverified on real hardware**.

Standards: 0 blockers; Spec: 0 blockers; Security: 0 blockers. Advisories above
pin implementation/evidence details; pending CI and root budget still gate source.
