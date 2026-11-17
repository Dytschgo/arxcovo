# M3 joint membership recovery: independent Sol source review

Reviewer: separate-author GPT-6.1 Sol, high effort, 2026-11-17; no children.
Outcome: PASS for the bounded source contract; runtime acceptance remains OPEN.
No source blocker or API/ownership contradiction found. This is not merge approval.

## Identity and scope

- Exact reviewed head: 8e3df55affcdfef8abc77b71ee3c44f0ba0dfeb7.
- Source correction: 8a91dcb9c1cdcfc480e7ff0a9d879ff7400ea529.
- Original builder: 73533941b76ae869bba43ae8947358f092eb7198.
- Net-diff base: 76460c9706a6a15af7ea9fa037aee52876da3591.
- Review used only the assigned isolated worktree, initially clean at that head.
- Source SHA256: 451582242970a90eaf70c0e108d7d71a4559af26d5d64e5f245223e3be563aba.

Reviewed all six net-diff paths: consensus.rs, membership_contract_tests.rs,
tools/ci/test_windows_cluster_quarantine.py, ADR 0024, PROGRESS and the joint
membership evidence document. The new Rust file has 691 lines after the import.
The base-to-head history and original-builder-to-correction source diff were read.
The correction adds only WatchReceiver; it changes no oracle or operation.

Read AGENTS, CONTRIBUTING, kickoff including the current routing amendment,
accepted proof brief including its startup disposition, and root joint review.
The supplied independent disposition PASS 6948230778 is reused context, not a
remote check performed here. Owner routing permits this separate Sol review;
historical cross-provider wording in CONTRIBUTING does not override it.

## Standards

No blocking documented-standard violation found in the reviewed net diff.
The test is file-local behind exactly one cfg(test) binding. No production
configuration, shared fixture, old test body, adapter or engine helper changed.
The exact six-path diff excludes Cargo/lock, nextest, workflows and sampler.
Recorded frozen blobs were independently compared with base and reviewed head.
Removing only the new two-line binding reproduces base consensus.rs bytewise.

The fixed Scenario enum and explicit oracle phases keep the seven controls
auditable. The large body warrants care during future edits, but no baseline
smell warrants a finding here. Splitting the same phase checks into separate
positive/negative oracles would make the required independence harder to assess.
Smell assessment is judgment, not a tooling or acceptance gate.

Documentation retains the initial compile stop, correction and runner mistakes.
Chronological older inventory/check claims are superseded by the final section;
they are not presented as successful engine evidence. PROGRESS retains PR170 as
last merged and exact main CI44085879290 as pending. No main-success credit.

## Spec

No blocking source deviation from the accepted amended proof contract found.
The code implements one current-thread test and seven fixed sequential fixtures:
Positive, OmitMembershipGuard, OmitTailGuard, OmitLeaderGuard, FirstConfig,
DropLearner4 and ResumeFuture. Each fixture allocates three private directories,
three real ConsensusNodes and a new router. No fourth engine is started.
Only node1 is initialized; real nodes2/3 are blocking learners and metadata4 is
added nonblocking. Stable U checks full membership, apply/commit/tail and addresses.

The sole boxed change_membership_if future receives AddVoterIds({2,3}), retain
true and U/leader guards. poll_fn returns the result of exactly one unconstrained
poll. The Box then remains unpolled while the core runs. Effective real J,
unchanged committed U, epoch and metadata are required; Pending alone earns nothing.
No constructed-J append, alternate driver, spawned wrapper or fallback exists.

Cached OpenRaft management.rs sends ChangeMembership through call_core and waits
for the complete response before this same future can send AppendMembership.
raft_inner.rs awaits API send before response; parking the caller does not park
the core. The observed J supplies the enqueue witness. Resuming the original Box
can submit uniform; dropping it before that step cannot submit that second call.
Actual scheduling and convergence remain runtime obligations, not source proof.

The isolated append requires exact InProgress fields and unchanged state.
Reconnect convergence checks effective, committed, decoded applied membership
and apply/commit progress on all three nodes. Parked checks require full tail J.
The target comes from J's last config and all saved J.nodes(), not intended IDs.
Public log_ids.last() is the same full-tail accessor used by LogStateReader.

All three stale probes change one fresh guard and compare exact mismatch fields.
Refusal also checks parked J and unchanged retained membership logs. Each omission
removes only its selected stale guard, requires actual successful F and all-node
convergence, and passes the complete Final oracle before requiring exactly the
designated RefusalExpected violation. An arbitrary Err cannot earn control credit.

FirstConfig submits J's first set; DropLearner4 removes only record4. Both require
real success and convergence. The same Final checker first verifies unaffected
fields against the submitted membership, then checks the original saved target
and requires exactly FinalVoters or Metadata. Expected final membership is never
redefined by the wrong request. Metadata equality plus exact configs also fixes
the learner set: {4} normally, {2,3,4} for the first-config control.

ResumeFuture awaits that same original Box, requires successful exact F, all-node
apply and one-J/one-F logs, then requires NoUniform from the shared Parked checker.
The checker accepts no Scenario tag or fabricated observation. Observation reads
real API results, metrics, with_raft_state, decoded state and retained log entries.
Positive checks F=J+1, epoch, metadata and logs, then discards the reply and reads
back without another append. This is reply-loss observation, not operation dedup.

Cached append validation checks preconditions before membership validation,
requires committed membership and compatible transitions, and preserves metadata
of retained nodes. This supports InProgress, exact stale errors, first-config
compatibility and removal of a nonvoter record; it does not prove runtime success.
No random loop, retrying write, assertion weakening or compiled-mutant claim found.

## Completion ownership and deadline

The absolute 75-second end is created once before pause-owner admission. The
body cutoff is end minus five seconds; operation caps are at most five seconds
and also limited by that cutoff. Convergence loops yield and remain bounded.
No next fixture can pass startup admission after the cutoff or follow a failure.

Pending startup stays pinned through the reserve. A returned late node is added
to owned nodes, its body is failed, and cleanup is attempted. Startup still pending
at end yields StartupIncomplete and cleanup=unresolved; it cannot become a pass.
Dropping its caller cannot cancel frozen spawn_blocking work. Such work/runtime
teardown may outlive the failed body; the unchanged CI outer bound is not release
evidence. This matches the explicit amended disposition, not a hard exit promise.

Body errors return as data after local futures/clones drop. Diagnostics retain
metrics values, not engine/store handles. Cleanup clears router-held engine clones
BEFORE concurrent shutdown of every returned node. PASS requires all three stop
results, both release signals and zero weak-reference strong counts. Expected
oracle rejection cannot conceal setup, convergence, cleanup or release failure.
There is no new reaper or detached cleanup task. Shared-budget fit is unmeasured.

## Security

No blocking security finding. This is a reversible test-only engine API proof;
it supplies no production recovery, identity, enrollment, TLS or owner authority.
Synthetic mem addresses and cluster IDs are fixtures, not private owner data.
Bounded log reads and private test directories introduce no MCP/path authority.
No secret, public-publishing change or administrative action appears in the diff.
Frozen production fences are unchanged. Real hardware remains unverified.

## Verification provenance and remaining gates

Actual reviewer work: source/docs/cache inspection, diff and blob comparisons,
SHA256 recomputation, saved-JSON parsing and git diff --check. No Rust command,
engine test, pure-suite rerun, network/status action or external publication ran.
Only this new report is committed by the reviewer.

Reused root evidence lives at D:/Temp/arx-joint-root-z2dl6rai. Recomputed hashes
match the documented results.json, Clippy/guard/sampler logs, list.json, exact
sampler JSON, inventory.json and frozen.json. Source hash matches the pin above.
Saved listing independently parses to 643 cases, 28 ignored fixtures, original
eight sample matches, founder13 and requiredTLS14 unignored. These are listings.
Root records actual fmt, locked/offline all-target workspace Clippy, guard5 and
sampler60 passes; these were read and authenticated by hashes, not rerun here.

Guard additions exercise changed-byte mutants for test omission/rename/duplicate,
reason/gate drift, hidden tests, global ignore, module binding and inventory/sample
drift. Old inventories, reason checks and proof tokens remain intact. The parser
is a bounded source guard, not Rust semantic proof. ADR correctly records Windows
coverage loss: new proof unsampled and ignored, Unix execution still required.

The first builder Clippy red remains recorded. Root's raw FILTER listing omitted
ignored cases; its first filesystem/Git comparison confused CRLF with Git LF.
Corrected evidence retains both mistakes without claiming source/runtime repair.

Publication still needs fresh green exact main, exact-head review integration,
all five normal gates, actual PASS of all seven scenarios in ordinary required
Ubuntu/macOS CI, and hosted feature-head Windows <=660 seconds. No local engine
case has run. Windows quarantine28 earns no recovery/re-enable acceptance.
Natural red stops the round; no retry, workaround or new repair budget is granted.
Sol shared3/3 and Opus0/2 stay unchanged. M2 open; M3 unchecked;
hardware paths are **unverified on real hardware**.

Reversibility: reverting the test/binding/guard/classification restores the prior
test surface without a data migration; preserve evidence/history when doing so.
Standards: 0 blocking findings. Spec: 0 blocking findings. Security: 0 findings.
Readiness: source review PASS; publication, runtime and milestone acceptance OPEN.
