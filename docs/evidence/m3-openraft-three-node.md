# M3 OpenRaft retained implementation evidence

Status: **local T1/T2 proof passed; fresh independent Opus review pending**.
Sol high, 2026-11-15. Resume parent `d52a009d17d2aca6203fd450873b9cd9d9728326`;
main `38750cf1e179e14da6fa871972027b6d66028ec9` (PR #144).
This section supersedes the historical blocked status below, preserving its facts.

## Retained scope

- Commit 1 remains `5cd8c6bd2f5a5a41fba98ce05f9479f82f5daed6`: pins/types/mappings/codec.
- Commit 2 `7b4bf62e25d902cb713a926bcfa8cc275cf06e0c`: adapters, eager callback,
  bounded snapshot peek, exclusive startup preflight, official Suite and startup tests.
- Commit 3 `e39535ff27af4d827cf51811a30c9d8c1b4f7fc6`: tracked proposal owners,
  ReadIndex, shutdown, test-only in-memory RPC, T1/T2/U6; routine Clippy corrections.
- Commit 4 is this evidence and the explicit immediate T3 follow-up below.

The source delta from the resume parent is 1752 added lines in six allowed paths.
`consensus.rs` is 406 lines; `adapter.rs` is 309. Params validation/default impls
live beside the adapters to keep both files within their prescribed bounds.
No log_store, enrollment core codec/order, dependency pins/lock, PROGRESS, ADR,
workflow, gate or other production path changed. No external agent, publication,
admin/native/VM/Lab Manager, listener, transport, CA or signing action ran.

## Current retained checks actually run

All build commands used CARGO_TARGET_DIR `D:/Temp/arxcovo-openraft-target` and
TEMP/TMP `D:/Temp/arxcovo-openraft-target/tmp`. These are Windows local results.
No previous partial-draft run receives current test credit.

- Commit-2 focused `cargo test -p arxd --locked --lib cluster::consensus::tests
  -- --nocapture`: 59 passed, 1.83 s runtime (before service restoration).
- Final `cargo nextest run -p arxd --locked cluster::`: **98 passed**, 456 skipped,
  8.788 s execution, 11.36 s build. This includes the unchanged old 36 cluster
  tests and both four-process-kill probes (eight owned process kills total).
- Final T1 1.236 s, T2 7.943 s, U6 1.662 s; each has a 45 s outer deadline.
- `cargo fmt --check`: passed. `cargo clippy -p arxd --all-targets --locked --
  -D warnings`: passed, 3.81 s including lock waits. `git diff --check`: passed.
- A separate current three-node run with `--nocapture` passed all three tests,
  7.60 s, recording RPC counters: sender mismatches 0 for every fixture;
  observed append maxima 3/7/64/5 (all <=64); snapshot maxima 0, full snapshots 0.
  U6 reported all 64 owners resolved after restored quorum.

Current local logs are in that target directory: `current-commit2.txt`,
`current-cluster-final.txt`, `current-clippy.txt`, `current-three-node-counters.txt`,
`current-openraft-features.txt`, `current-chrono-features.txt`.
Hosted three-OS checks and total CI wall time have not run for this head.
No full-workspace owner-security AccessDenied run was repeated.

## What these tests establish

Preflight runs inside the one blocking store-open closure, with exclusive owned
stores before any Arc, adapter or Raft exists. It reads A/P/L, checks the SQLite
index cap and checked successor, and reads at most one validated retained row.
Twelve refusal fixtures cover both term directions, leader mismatch, purged-ID
mismatch, interior mismatches, missing applied row, unordered lower-index tails,
behind-purged state and absent applied state with a purge. Direct preflight and
own start refuse with Corrupt/StartError::Preflight; reopened vote, log state,
every retained row and exact SM snapshot bytes remain equal. Extra snapshot
fixtures reject indices i64::MAX+1 and u64::MAX without lookup overflow.
Seven accepted fixtures cover empty/log-only stores, exact applied slot, retained
slot under a newer tail, P=A, and ordered SM-ahead holes including an empty log.
The hole test verifies purged=last=5, append 4 refused and append 6 accepted.
A bounded yield/reopen loop handles transient Windows file-lock release after
Weak expiration; no failed preflight is retried or relaxed.

Raw-helper characterization bypasses own preflight and grants no enforcement
credit. On these exact fixtures, higher-term/leader equal-index, both interior
mismatches and the two unordered hole tails return Ok. Lower-term/leader equal
index, P=L=T2-N1.5 over SM T1-N1.5, the missing-prefix log 7..9, and behind-purged
fixtures return Err. In particular, the first two additional Err results differ
from the brief's exploratory raw-helper expectation; tests retain observed facts,
not an invented Ok. Raw Raft::new behind-purged returns Fatal::StorageError.
No equal-index raw Raft Fatal is claimed. Own refusal passes independently.

All 47 official cases were actually rerun unchanged: S1's 27 return Ok; S2's
19 plus transfer_snapshot return Err containing "out of order", each under
its own spawned task with panics/unexpected Ok rejected. Exact named partition
is the unchanged historical S1/S2 list below and the retained tests. No case
was reclassified, no SM ordering was weakened, and this is not full conformance.

C1's actual plain append first owns Completion before iteration. Actual unpolled
valid/65/unencodable futures produce one callback Err and unchanged reopened
logs. Awaited success is durable on reopen; store error, iterator panic, injected
blocking append panic and subsequent poisoned access fail closed. A manually
polled then dropped append completes durably after its caller future disappears.
The IOFlushed::signal oneshot receives its sole value; Completion consumes the
callback on finish and Drop only handles an unconsumed callback.

C2 uses JoinSet owners and ProgressResponder::complete_only, retaining each permit
through the final engine response independently of the caller. U6 establishes
committed membership before isolation, observes 64 actual log entries beyond
local_committed, aborts/joins callers, and requires zero permits and next Busy.
Both isolated-leader shutdown and restored quorum restore 64 permits, no owners,
Weak release and later Stopped. Shutdown closes admission, stops the core before
joining owners and drops its own strong adapters under the shared five-second
deadline. No shutdown reports Orphaned in these successful fixtures.

T1 exercises committed Mint/Redeem, deterministic replay, follower offline with
2/3 continuation, reuse, concurrent competing keys (one node 5, one Consumed),
wrong proof and logical expiry. T2 exercises leader offline/election, continued
writes, ReadIndex covering acknowledged positions, follower refusal, durable
old-leader SM reopen, no-quorum write/read refusal, identical unknown retry and
node IDs 5 then 6 proving at-most-once allocation. Restarts pass the mandatory
preflight and converge to byte-identical state. Every fixture shuts down owned
nodes and scans all directory files for raw/hex deterministic secrets, bearer
proofs and owner-transfer codes. The code-ID and secret RNG patterns are distinct
so a public field cannot masquerade as the secret marker.

## Remaining delivery boundary

**T3 is an immediate follow-up, not completed or credited.** Source already adds
1752 lines before this evidence; a retained full_snapshot catch-up fixture would
exceed the roughly 1800-line slice budget. Next: small snapshot params (8/0/1/16),
stop a follower, commit 40 unknown redeems until leader purge passes it, restart,
require an actual full_snapshot RPC to that follower, byte-identical convergence,
then consecutive applies and bounded owned shutdown. Current zero-snapshot
counters explicitly provide no T3 credit.

Fresh Opus source/security review is required before publication; required hosted
gates remain required. This proves only real OpenRaft over persistent local stores
with in-memory RPC and graceful restarts. Logical time only; no physical-expiry,
authentication, separate-process/host, TCP/mTLS/CA, crash-during-consensus,
power-loss, membership-change or full M3 acceptance claim. All hardware paths are
**unverified on real hardware**. M2 remains open, A7 FINAL NO-GO and A12's consumed,
cleanup-unconfirmed no-retry boundary are unchanged.

## Dependency verification repeated for retained pins

Parsed Cargo.lock comparison to main: 208 existing packages, 243 current, 35 new,
zero removed. Only arxd and chrono existing blocks differ; metadata (including
versions/checksums/sources) is unchanged. Dependency additions are exactly
arxd futures-util/openraft and chrono js-sys/wasm-bindgen; none removed.
Cargo.toml/arxd Cargo.toml/Cargo.lock are byte-unchanged from resume parent.
Actual locked feature trees show OpenRaft only tokio-rt; chrono default/wasmbind
unification through display-more/OpenRaft macros remains as previously recorded.
No serde/clap/bt/single-threaded/type-alias OpenRaft feature; no serde-free claim.

---

# Historical handbacks (preserved; statuses below are historical)

# M3 OpenRaft resume: new bounded recovery handback

Status: **BLOCKED on new actual recovery-contract conflict**, Sol high, 2026-11-15.
Parent e055b87; main 38750cf1e179e14da6fa871972027b6d66028ec9 (PR #144).
The original handback is preserved below as dated history. Its responder and lock
blockers are resolved by the corrected contract; neither remains a blocker.

## Retained scope and stop boundary

Commit 7d60a76d3883d7ac868af1b1e22de18edc20c480 retains the prescribed pins/lock,
default Arx/Applied, exact position/vote/entry mappings, bounded canonical membership
codec, static Command Display, Membership error and four meaningful unit tests.
No log_store, core codec, state-machine order, workflow/gate, PROGRESS or ADR changed.
No adapter, service or network draft remains in the worktree.

The requested stop on a new actual conflict was applied before completing commit 2.
Commits 2-4 are incomplete. T1/T2 were drafted but **never run**. T3 is unimplemented,
not an accepted deferral. This is no publication candidate or usable consensus proof.
Fresh independent Opus review and coordinator contract correction are required.

Unreviewed working material is preserved at
D:/Temp/arxcovo-openraft-target/resumed-contract-conflict/: adapter.rs, consensus.rs,
consensus-tests.rs, state_store.rs, mem_net.rs, three_node.rs. Parent-directory logs:
resumed-suite.txt, resumed-startup-conflict.txt, resumed-chrono-features.txt.
The final diagnostic deliberately observes higher-term acceptance; its passing
result does not satisfy R3. Earlier partial-draft test results receive no credit.

## New actual conflict: equal-index recovery is asymmetric

Decision 9 says startup fails closed on an equal-index conflict; R3 requires the
real StorageHelper to reject it. Cached primary source openraft-0.10.0,
src/storage/helper.rs, checks that condition only inside
`if last_log_id < last_applied`. A higher-term log skips the branch and returns Ok
with contradictory durable applied and log positions. No store/recovery workaround
was attempted; the scope prohibits core changes.

Reproduction: real adapter-backed private SQLite directories, cluster [1;16],
first_free 4, persisted committed vote T3-N1. Apply SM blank indices 0..=5 at term 1,
leader 1; append log blank indices 0..=5 at the indicated term, leader 1; call
`StorageHelper::new(&mut ls, &mut sm).get_initial_state()`. Both streams are
consecutive and internally ordered. No committed-position override is implemented.
Actual positions recorded after the helper call:

| Durable SM | Durable log tail | Actual helper result |
| --- | --- | --- |
| T1-N1.5 | T2-N1.5 | **Ok**, violating general equal-index conflict rejection |
| T1-N1.5 | T0-N1.5 | Err: inverted log order, equal index |
| T1-N1.5 | T1-N1.5 | Ok |
| T1-N1.5 | T1-N1.3 before recovery | Ok, purged = last = T1-N1.5; append 4 rejected, append 6 accepted |
| T1-N1.1 | T1-N1.5, purged through T1-N1.3 | Err: state machine is behind the purged logs |

Acceptance-style R3 first failed on `assert!(result.is_err())`. A bounded diagnostic
then observed all five cases: 1 diagnostic passed, runtime 0.21 s, compile 25.05 s.
That diagnostic confirms the contradiction; it does not fix it. An earlier fixture
without vote hit the helper's default-vote `self.id.unwrap()`; saving the fixture
vote corrected that ordinary setup issue. Raft::new fatal-path proof is incomplete.
No bug was retried with three speculative fixes.

## Corrected responder and lock checks

The service draft compiled using default ProgressResponder::<Arx, _>::complete_only()
and client_write_ff(cmd, Some(responder)); no Responder override. This is compiler
agreement only: U6, tracked-owner cancellation, shutdown and Weak release are unrun.

Cargo generated the complete lock normally with cargo check -p arxd --offline;
subsequent checks used --locked. No manual incomplete lock, patch or substitution.
TOML package-block comparison against parent e055b87: 208 existing blocks, 206
byte-identical, only arxd 0.1.0 and chrono 0.4.45 changed; 35 added, zero removed.
Parsed assertions verified all existing metadata equal after removing only these
new dependency edges: arxd adds futures-util/openraft; chrono adds js-sys/wasm-bindgen.
All existing versions/checksums/sources are identical. Diff: 334 additions, zero deletions.

Actual cargo tree -e features -i openraft --locked:

```text
openraft v0.10.0
  openraft feature "tokio-rt"
    arxd v0.1.0
      arxd feature "default" (command-line)
```

Actual chrono feature tree confirms default-feature unification via display-more
and openraft-macros: chrono default -> wasmbind -> js-sys and wasm-bindgen. Full tree
is saved locally in resumed-chrono-features.txt. No openraft serde/clap/bt/
single-threaded/type-alias feature. Transitive serde exists; no serde-free claim.

## Official Suite: exploratory draft results only

Each of 46 unmodified test_store cases ran in tokio::spawn, plus transfer_snapshot.
S1 returned Ok; every S2 returned Err containing "out of order"; no panic or
unexpected Ok. The actual partition agreed with the proposal in this draft.
transfer_snapshot takes &Builder, not (LS, SM); the wrapper used its actual source
signature. BasicNode::default is localhost; its codec roundtrip passed.
No Suite case was blindly reclassified and no SM order was weakened.

S1 (27):

- `last_membership_in_log_initial`
- `last_membership_in_log_multi_step`
- `get_membership_initial`
- `get_membership_from_log_and_empty_sm`
- `get_initial_state_without_init`
- `save_vote`
- `get_log_entries`
- `limited_get_log_entries`
- `leader_bounded_stream`
- `entries_stream`
- `try_get_log_entry`
- `log_reader_reads_new_entries`
- `initial_logs`
- `get_log_state`
- `get_log_id`
- `purge_logs_upto_0`
- `purge_logs_upto_5`
- `purge_logs_upto_20`
- `delete_logs_after_11`
- `delete_logs_after_5`
- `delete_logs_after_0`
- `delete_logs_after_none`
- `append_to_log`
- `snapshot_meta`
- `snapshot_meta_optional`
- `apply_single`
- `apply_multiple`

S2 (19 plus transfer_snapshot):

- `last_membership_in_log`
- `get_membership_from_empty_log_and_sm`
- `get_membership_from_log_le_sm_last_applied`
- `get_membership_from_log_gt_sm_last_applied_1`
- `get_membership_from_log_gt_sm_last_applied_2`
- `get_initial_state_membership_from_empty_log_and_sm`
- `get_initial_state_membership_from_sm_inlog_is_smaller`
- `get_initial_state_membership_from_log_insm_is_smaller`
- `get_initial_state_with_state`
- `get_initial_state_last_log_gt_sm`
- `get_initial_state_last_log_lt_sm`
- `get_initial_state_last_log_lt_sm_past_snapshot`
- `get_initial_state_sm_lt_purged`
- `get_initial_state_inverted_log_order`
- `get_initial_state_conflict_at_same_index`
- `get_initial_state_log_ids`
- `get_initial_state_re_apply_committed`
- `last_id_in_log`
- `last_applied_state`
- `transfer_snapshot`

Exploratory run: 51 passed (four mapping/codec plus 47 Suite wrappers), runtime
1.54 s, compile 19.70 s. Expanded draft: 53 passed, 1 failed (R3 conflict), runtime
1.67 s, compile 21.81 s. Actual append unpolled valid/65/unencodable cases and
awaited durable success/store error passed, as did sparse SM and snapshot metadata
mismatch preservation. Full U4 panic/SQL/polled cancellation coverage is incomplete.
These unretained draft results establish neither retained adapter behavior nor
full Suite conformance. Params/U3, C2/U6 and T1/T2 are unverified at runtime.

## Actual final retained checks

Every Cargo command set TEMP, TMP and CARGO_TARGET_DIR to
D:/Temp/arxcovo-openraft-target. No installs or cache cleaning occurred.

| Check on retained dependency/types/codec scope | Actual result |
| --- | --- |
| cargo test -p arxd --lib cluster::consensus --locked | 4 passed; 0.03 s runtime; 18.07 s compile |
| cargo nextest run -p arxd --locked cluster:: | 40 passed, 456 skipped; 1.224 s test wall time; 17.10 s compile |
| Existing coverage in that run | Original 36 passed; both four-process-kill tests passed (eight probes) |
| cargo fmt --check | Passed |
| cargo clippy -p arxd --all-targets --locked -- -D warnings | Passed; 8.66 s including build-directory lock wait |

The new 40-test run is on retained source; it supplies no election/quorum/ReadIndex
credit. No full workspace or owner_windows suite ran; no historical AccessDenied
attribution. Hosted OS checks and CI <15 min across OSes remain unverified.

No subagents, pushes, PRs, watches, native M2/admin/VM/Lab Manager work, dispatches,
variables, listeners, signing, CA or owner-data actions. M2 stays open: A7 FINAL
NO-GO; A12 consumed, cleanup uncertain, no retry. Three-node graceful restarts,
snapshots, quorum, byte counters and fixture marker scans remain unverified.
Transport, mTLS/key authentication, physical expiry, power loss and KICKOFF full
M3 acceptance remain **unverified on real hardware**.

---

## Dated history: original 2026-11-15 handback (retained26360a5)

# M3 OpenRaft 0.10.0 bounded implementation handback

Status: **BLOCKED, no implementation retained**, Sol high, 2026-11-15.
Parent `3cdf92f` (`work/m3-openraft-adapter-boundary`); main baseline
`38750cf1e179e14da6fa871972027b6d66028ec9` (PR #144).
The corrected brief `bc90124` and C1/C2 recheck were read before implementation,
as were AGENTS, KICKOFF, ADR0005, ADR0023 and PROGRESS Ordering.

The explicit instruction to stop on actual C1/C2 API rejection applies. No responder
substitution, type-config override, dependency-feature workaround or weakened store
semantics was adopted. This commit retains only this evidence. A partial draft and
bounded compiler logs remain locally in `D:/Temp/arxcovo-openraft-target`; they are
unreviewed working material, not a ready implementation or acceptance artifact.

## Confirmed blocker: default responder disagrees with C2

Cached primary source: `D:/Temp/arx-openraft-0100/openraft-0.10.0`.
Archive SHA256:
`2628033834a66c7c5bf54c49673a5bbba7192fbcf529314449a0393cb15f7ad0`,
matching the resolved openraft registry package checksum.

`src/raft/mod.rs:258` defaults `Responder<T>` to
`openraft::impls::ProgressResponder<Self, T>`. Decision 2 retains every default:

```rust
openraft::declare_raft_types!(pub Arx: D = Command, R = Applied);
```

C2's owner used the specified construction:

```rust
let (responder, reply) = OneshotResponder::<Arx, _>::new_pair();
let result = match raft.client_write_ff(cmd, Some(responder)).await {
    // completion receiver awaited by the tracked owner
```

Actual `cargo test -p arxd --lib cluster::consensus --locked` compiler output
from the service draft (paths shortened; no diagnostic text changed):

```text
error[E0308]: mismatched types
   --> crates/arxd/src/cluster/consensus.rs:102:60
102 | let result=match raft.client_write_ff(cmd,Some(responder)).await {
    |                                          ---- ^^^^^^^^^ expected `ProgressResponder<Arx, Result<..., ...>>`, found `OneshotResponder<Arx, _>`
    = note: expected struct `ProgressResponder<consensus::Arx, std::result::Result<ClientWriteResponse<consensus::Arx>, ClientWriteError<consensus::Arx>>>`
               found struct `OneshotResponder<consensus::Arx, _>`
error: could not compile `arxd` (lib test) due to 1 previous error
```

Primary source also exposes `ProgressResponder::complete_only()` in
`src/raft/responder/impls/progress_responder.rs:95`; the macro supports an explicit
`Responder<T>` override. These are possible decisions for Opus to assess, **neither
was implemented or compiler-verified here**. Preserve cancellation-independent
JoinSet ownership, 64 engine-held permits and the required shutdown/U6 proof in
whatever corrected contract is selected.

C1's plain non-async append implementation, with Completion constructed as its
first statement, did compile in `cargo check -p arxd --locked` after correcting
ordinary type aliases. No U4 runtime cancellation tests ran. This is API agreement
only, not callback correctness evidence. That adapter draft is not retained in Git.

## Confirmed blocker: existing lock entry cannot remain byte-identical

Requested dependency settings were used exactly: openraft `=0.10.0`,
default-features false, only `tokio-rt`; futures-util `=0.3.34`; tokio `sync` added
on existing `=1.48.0`. Cargo resolved **35 added packages**, with no version,
checksum or source replacement of any existing package. Exact package-block
comparison nevertheless found two changed existing blocks:

- `arxd 0.1.0`: necessary new direct dependency edges `futures-util`, `openraft`.
- `chrono 0.4.45`: added dependency edges `js-sys` and `wasm-bindgen`.

Draft lock diff: 334 additions, zero deletions. Additive lines are **not** the same
as byte-identical existing entries. `cargo tree -e features -i chrono --locked`
shows OpenRaft/display-more/openraft-macros enabling chrono default features,
including `wasmbind`, which enables those edges. Both packages already exist in
the baseline lock; their versions did not change.

Removing only those two new chrono edges from the resolved draft lock and running
`cargo check -p arxd --locked` produced:

```text
error: cannot update the lock file <workspace>/Cargo.lock because --locked was passed to prevent this
help: to generate the lock file without accessing the network, remove the --locked flag and use --offline instead.
```

No manual incomplete lock, patch dependency, different version or feature
substitution was retained. Opus/coordinator must resolve the byte-identical
restriction before accepting a dependency commit.

Actual `cargo tree -e features -i openraft --locked` (workspace path omitted):

```text
openraft v0.10.0
└── openraft feature "tokio-rt"
    └── arxd v0.1.0 (<workspace>/crates/arxd)
        └── arxd feature "default" (command-line)
```

No openraft serde/clap/bt/single-threaded/type-alias feature was enabled. Transitive
crates can still use serde; this tree is not a claim of a serde-free dependency graph.
Full local tree outputs: `openraft-tree.txt`, `openraft-feature-tree.txt`,
`chrono-features.txt` in the dedicated D: directory.

## Actual checks and corrections

All Cargo runs used TEMP, TMP and CARGO_TARGET_DIR set to
`D:/Temp/arxcovo-openraft-target`. No installs or cache deletion occurred.

The temporary partial draft comprised dependencies, default Arx/Applied types,
position/vote/entry mappings, bounded canonical membership codec, static Command
Display and the named Membership error. Four tests exercised mapping/vote order,
normal command mapping/static Display, maximal 3605-byte joint membership,
every nonempty truncation, default node roundtrip and malformed/bounded inputs.
It was removed from the worktree after preserving the local draft; the following
passes apply **only to that partial draft**, not a real-engine service:

| Run | Actual result |
| --- | --- |
| `cargo test -p arxd --lib cluster::consensus --locked` | 4 passed, test runtime 0.04 s; compile 8.96 s |
| `cargo nextest run -p arxd --locked cluster::` | 40 passed, 456 skipped; 1.492 s test wall time; compile 25.70 s |
| Existing cluster coverage within that run | All original 36 passed, including both four-process-kill tests (eight probes total) |
| `cargo fmt --check` | Passed |
| `cargo clippy -p arxd --all-targets --locked -- -D warnings` | Passed, 24.24 s |
| `git diff --check` | Passed before cleanup; rerun on final evidence |

Initial draft errors were ordinary incorrect generic type spellings, dereferencing
MutexGuard and import paths; those were corrected before the adapter compiled.
A codec test incorrectly expected BasicNode::default to be empty, causing two
observed failed runs (one earlier queued draft and the expanded test). Actual
`src/node.rs` sets it to `localhost`; the corrected roundtrip passes. The earlier
commentary suggesting Suite/address incompatibility is withdrawn. No official
Suite case was moved or reclassified on that mistaken assumption.

The C2 E0308 failure above was **not fixed or retried with a substitute**. The
partial passing draft excludes the rejected service. No broad Windows owner
fixture suite was repeated, and no attribution is made for historical access-denied
failures. Hosted checks and independent Opus review have not run on this handback.

## Resume boundary and claims

Only the coordinator should update PROGRESS with this Sol/high/blocked outcome;
this subtask's exact-file restriction excluded PROGRESS. Last merged PR remains
#144. No publication or merge was attempted.

After Opus corrects the responder/lock constraints, resume the prescribed small
implementation commits. Adapter U4/U5, Params/U3, StorageHelper R1-R4, tracked
service/U6, mem_net and mandatory T1/T2 remain unimplemented/unverified. S1 (27)
and S2 (19 plus transfer_snapshot) are the brief's **proposed, wholly unrun**
partition; there is no Suite conformance claim. T3 snapshot catch-up is also unrun,
not an accepted deferred fourth commit. This handback has no election, quorum,
ReadIndex, enrollment-from-commit, restart-convergence, byte-counter or shutdown
proof. No tasks, stores, listeners or Raft engines remain running from the draft.

Fresh independent Opus review is required before any later publication. No
subagents, pushes, PRs, watches, native M2 work, admin steps, VMs, Lab Manager,
dispatches, variables, CA or signing actions occurred. M2 stays open: A7 FINAL
NO-GO and A12 consumed/cleanup-unconfirmed, with no retry. Transport, real hosts,
power loss, physical expiry and authentication remain **unverified on real hardware**.
