# Brief: M3 learner admission, catch-up and promotion after F2

Status: **L1 local verification and independent source review PASS** (2026-11-17). The initial Clippy stop and supported-API correction are retained in the evidence. Hosted gates and exact-head review remain pending. L2/promotion blocked.
Exact-main CI 44069931184 passed on fa837888, attempt 1 (472 s wall, Windows 467 s). ADR 0005 with its 2026-11-15 amendment, ADR 0023,
ADR 0024 (all addenda), the [runtime boundary brief](m3-next-runtime-boundary.md) and the [F2 brief](m3-founder-joiner-tls-proof.md) stay binding.
No ADR is amended here. M2 open, M3 unchecked, Windows recovery Sol 3/3 / Opus 0/2; everything here is **unverified on real hardware**.

## Decision

- **L1 Ready** (after green exact main): a test-only, one-process, two-engine proof that a real enrolled joiner replica is admitted as a
  learner, catches up by Raft replication alone, and then authenticates the founder with **its own** committed registry. No production edit.
- **L2 Blocked** (production admission over mutual TLS): the joiner bootstrap trust anchor (D1) is not settled, and no membership operation
  budget exists (D2). **Promotion Blocked** on D2 and D3. The owner and architecture decisions are below; L1 does not depend on any of them.

## Two separate authorities (do not merge them)

- **Registry enrollment/revocation** is `EnrollmentState` state-machine data (`Found`, `Redeem`, `Remove`). It answers "which key may act as
  node N" and is read per connection by `peer_registry()` / `authorize_peer` on the **local applied** state (unbounded staleness, accepted).
- **Raft membership** is OpenRaft config (`initialize`, `add_learner`, `change_membership`). It answers "who receives logs and who votes".
  Per the amendment, registry `Remove` does not change Raft membership and admission does not change the registry. Pairing them is workflow
  work; today only the test helper `registry_covers` (`consensus.rs`, `#[cfg(test)]`) relates them. L1 adds no pairing claim.

## The bootstrap cycle (traced in current source)

1. A joiner replica opens `StateStore::open(dir, Init)` into an empty store: `founded == false`, empty `members`. Its `peer_registry()`
   therefore returns `NotMember` for every peer, including the founder.
2. OpenRaft is push-based. Catch-up needs the leader to open a connection to the learner (leader = TLS client, learner = TLS server).
   Under the ADR 0005 verifier, the learner's server-side `PeerVerifier` checks the founder's leaf against the learner's own (empty) registry
   and refuses. The leader-side check passes (the founder registry holds node 2 after `Redeem`).
3. The learner can only fill its registry by applying replicated entries. So under the mandated verifier, catch-up never starts.
4. The ADR shortcuts are closed: "CA-chain validation alone is insufficient authorization"; "a certificate is never the authority on its own";
   "a stolen CA key alone cannot impersonate a member". Seeding the joiner's state machine from a non-Raft source would bypass the replicated
   state machine. F2 avoided the cycle only by using the founder's view on both sides (claimed as a one-process shared view).

So ADR 0005's "add a learner, prove catch-up, then promote" is unreachable over production TLS without a new, explicit pre-catch-up rule (D1).

## L1 settled design (test-only)

One new required test `learner_catch_up_gives_joiner_own_registry` in `consensus/tls_engine_tests.rs`, `#[tokio::test(flavor =
"multi_thread", worker_threads = 4)]`, body inside one `timeout(Duration::from_secs(20), ..)`; every inner wait uses
`Some(deadline.saturating_duration_since(Instant::now()))` from that single deadline (no new constant). No `cfg`/`cfg_attr`/`ignore`.

Transport claim, stated up front: catch-up runs over the mem_net double with an **empty `authenticated` map** (its default `sender ==
source` binding, as in every existing multi-engine fixture). It is **not** authenticated catch-up, and no joiner-side acceptance is faked.

1. One shared `Arc<MemRouter>`. Two `private_temp()` dirs, each asserted `security::private`. Node 1 starts with `founder_init([1; 16])`,
   `Factory { source: 1, router }`, and its engine is inserted in `router.nodes`. `found()` issues the founder leaf; `Mint` + `enroll()`
   issue node 2 (as in F2 steps 1-3, distinct CSR keys `fkey`, `jkey`).
2. **Genesis contract.** Node 2 starts on its own dir with exactly `founder_init([1; 16])`, `Factory { source: 2, router }`, inserted in
   `router.nodes`. Assert uninitialized (`last_log_index == None`, `current_term == 0`) and its own `peer_registry()` gives `NotMember` for
   `(1, fd)` and `(2, jd)`. No `initialize`, `install`, `Found` or other write ever targets node 2 directly.
3. **Configs once, retained for the whole test.** Founder server and founder client use `pki::verifier(&ca, n1.peer_registry())`; joiner
   server and joiner client use `pki::verifier(&ca, n2.peer_registry())`. Same `FixedTime(pki::NOW / 1000)` and production builders as F2.
4. **Cycle evidence.** Founder client -> joiner server and joiner client -> founder server both fail with `NotMember` from the **joiner's**
   verifier (`pki::refusal`). This is the cycle of the section above, observed with real leaves and production configs.
5. **Leader guard, then admit.** A `fresh_read` on node 1 gives `authorize_peer([1; 16], 2, jd) == Ok`. Then
   `raft.add_learner(2, BasicNode::new("mem:2"), false)` (non-blocking: OpenRaft's `blocking = true` waits with `wait(None)`, unbounded).
   Its response log index is `L`. Node 1 metrics: voters `{1}`, learners `{2}`.
6. **Catch-up.** `n2.raft.wait(..).applied_index_at_least(Some(L), ..)`. Then node 2's locked `snapshot()` bytes and `applied()` equal
   node 1's `linearizable_state()`; node 2 metrics show `state == Learner`, `current_leader == Some(1)`, voters `{1}`, learners `{2}`.
7. **Own-view handshakes.** With the step-3 configs unchanged: founder client -> joiner server gives `(2, 1)`, joiner client -> founder
   server gives `(1, 2)`. Each side now verifies under its own registry; only replicated state changed since step 4.
8. **Learner outside write quorum.** Insert 2 into `router.isolated`; a second `Mint` on node 1 commits; voters stay `{1}`. Remove the
   isolation and wait for node 2 to apply that index again.
9. **Learner durability.** Remove node 2 from `router.nodes`, `shutdown()` is `Ok`. Reopen its dir with `founder_init([1; 16])` (2 in `router.isolated`,
   not in `router.nodes`): its `applied()` equals the pre-shutdown value, and its fresh `peer_registry()` gives `(1, fd)` and `(2, jd)` Ok and `(3, jd)`
   `NotMember` with no leader contact. Shut down both nodes `Ok`; drop configs last.

L1 uses raw `raft.add_learner` from test code, outside `propose_response`'s 64 proposal permits and `Owners`. That is acceptable only
because it is a test; it is **not** a production admission path and is claimed as engine/storage feasibility only.

## API feasibility (cached OpenRaft 0.10.0; no production edit needed)

- `add_learner(id, node, blocking)` needs `MembershipEditOf<C>: RaftMembershipEditOps<C::Membership>`. `declare_raft_types!(pub Arx: ..)`
  defaults `MembershipEdit` to `impls::ChangeMembers`, which implements it (`membership/change_members.rs:235`). If it fails to compile, stop.
- `Wait::applied_index_at_least` and `Wait::metrics` are public; `RaftMetrics.membership_config.membership().{voter_ids, learner_ids}` exist.
- `encode()` in `consensus.rs` already stores learners as nodes outside the voter configs; `"mem:2"` passes its address charset.
- `tls_engine_tests` descends from `consensus`, so it can reach `raft`, `sm`, `fresh_read` and `super::super::mem_net`, as F2 does.
- No snapshot path is exercised (5-6 entries < `snapshot_logs` 64), so catch-up is pure log replay. That is what makes the genesis contract
  load-bearing: snapshots carry `next_node_id`, but replayed `Redeem` allocates from the replica's own `Init.first_free`.

## Mutants (run once each, only against the new test; restore byte-for-byte; record the observed catch only)

- **M1** joiner server built with `n1.peer_registry()`: step 4 handshake succeeds and the test fails there.
- **M2** node 2 started with `first_free` 3: replayed `Redeem` allocates node 3 on the replica, so step 6 snapshot equality fails.
- **M3** `add_learner` replaced by `change_membership(ChangeMembers::AddVoters({2: "mem:2"}), false)`: step 5 voter assertion fails.

## Guard and inventory (`tools/ci/test_windows_cluster_quarantine.py`)

`TLS_REQUIRED` gains the new name (tokio::test count = 3 + 3) with the existing drift mutant. Generalize `assert_real_identity_body` into
a per-name required/forbidden check. For the new body, required: `founder_init(`, `.found(`, `.enroll(`, `add_learner(`, `peer_registry()`,
`pki::pump(`, `applied_index_at_least(`. Forbidden: the F2 list plus `change_membership`, `initialize(`, `.install(`, `AddVoter` and
`authenticated`. One inject/remove mutant per token. Unchanged: quarantine **27**, sampler **8** and its FILTER, all 13 founder cases,
the 13 existing TLS cases (now 14 TLS required), and F2's body check.

**Windows classification.** The new case is two engines with one voter and one learner. Learners never start elections, the founder commits
alone, and it constructs no `three_node::Cluster`. It is outside the ADR 0024 whole-fixture quarantine and is **required on all three OS**.
No Windows ignore is authorized. A natural red on any OS stops the round for disposition (no rerun, filter, ignore, deadline or source
tuning); quarantining it would need a new owner decision, not a per-test expansion.

## Allowlist (Sol high builds; Opus reviews)

`crates/arxd/src/cluster/consensus/tls_engine_tests.rs` (one test and imports), `tools/ci/test_windows_cluster_quarantine.py`,
`docs/evidence/m3-learner-catch-up.md` (new), `docs/PROGRESS.md`, and a status line here. **No** production, helper, `pki/tls_tests.rs`,
`three_node.rs`, `mem_net.rs`, Cargo/lock, workflow, sampler, nextest, timeout, retry or scheduling change. Any API gap: stop and report.

## Verification (one round) and gates

`cargo fmt --check`; workspace clippy `-D warnings` with tests; nextest of the new test plus the 13 founder and 13 TLS required cases
(record each wall time; the new test is expected to take a few seconds); the guard unittest; then M1-M3. Publication needs fresh green
exact main, independent Opus review against this brief, the five normal gates and hosted Windows `verify` <= 660 s. Root merges.

## L1 claims and limits

Claimed: one process; a real F1 founder and a real F5 joiner replica on separate durable directories; learner admission with voters
unchanged; catch-up by log replay to a byte-identical state; the joiner's own durable registry authorizes the founder in production TLS
configs after catch-up and refuses it before. **Not claimed:** authenticated catch-up, a production admission or promotion API,
registry/membership pairing, snapshot catch-up, multi-host, sockets, endpoint separation, custody, freshness, or real hardware.

## Owner and architecture decisions (block L2 and promotion; not L1)

**D1 - joiner pre-catch-up trust anchor (owner security decision; amends ADR 0005).** Why the text does not settle it: the amendment makes
CA chain plus local applied registry the only authorization, and an unfounded joiner has none, so the ADR's own learner step cannot run.
- **O1 issuer pin.** Enrollment returns the issuing leader's committed `(node, key)`; the joiner accepts only that peer until it applies
  `Found`. Narrow, but a leader change before catch-up strands the joiner and needs a fresh code.
- **O2 voter-binding anchor (recommended).** The issuing leader reads the committed bindings of the current Raft **voters** inside the
  second `issue_guard` fresh read (the one that rechecks the grant), then returns them with the leaf over the CA-fingerprint-pinned
  enrollment reply. The joiner persists them durably **outside** `EnrollmentState`, never in log, snapshot or MCP. It uses them only while
  its own registry is unfounded and only for inbound replication (`AppendEntries`, `InstallSnapshot`), never for control or execution. They are discarded the moment
  `Found` applies. This survives leader churn among existing voters, adds no owner step, and keeps "certificate never authority alone":
  a CA-key holder still needs a committed binding. Residual risk: a binding removed after the grant is honoured until catch-up, the same
  class as the accepted local staleness.
- **O3 CA-chain-only while unfounded.** Rejected: it contradicts the ADR text above.
- **O4 extra owner-pasted pin.** Rejected: it adds a second owner transfer to the one-code join flow.

**D2 - membership operation budget (gap, not guessed).** Production admission needs a bounded wait for the learner commit and for catch-up.
Existing budgets are `READ_BUDGET` 1.5 s, `ISSUE_BUDGET` 5 s, 4 issue permits, 8 read permits, 64 proposals and the 5 s shutdown. None
covers catch-up, which scales with log size. Settle by ADR before L2: admission through the 64 proposal permits and an `Owners`-spawned
`add_learner(.., false)` (like `propose_response`), plus a caller-side catch-up deadline value with its fail-closed result.

**D3 - promotion policy (ADR 0005 "Quorum and unresolved choices").** Whether a two-host cluster promotes host 2 (two voters, both needed
for writes) or keeps it a learner until host 3. Any promotion test also has 2+ voters with live election timers, so it needs its own
Windows classification. Promotion should use `change_membership_if` with `Precondition::LastMembershipLogId` to serialize intents, and a
post-commit registry re-check like `issue_guard` to reconcile a concurrent `Remove`.

**Genesis gap (for L2).** Every replica must open with `founder_init(code cluster)`; nothing enforces that today (M2 shows silent divergence).
L2 must derive it from the code and refuse any other persisted `first_free` on a joiner directory, in the same style as F1's
`FounderAllocation`.

## Next action

Root: after CI 44069931184 is green on exact main, give L1 to Sol high on that head with this brief, and take D1 (recommend O2), D2 and D3
to the owner in one message. Do not start L2 or promotion until D1 and D2 are recorded in an ADR 0005 amendment.


## Root L1 contract check - 2026-11-17

L1 accepted for test-only implementation on exact fa83788: CI44069931184 attempt1
is green, wall472s / Windows467s, and the F2 test passed on every OS. L2 and
promotion production APIs are not approved. Compute one deadline at entry and
use timeout_at plus its remaining duration for every wait. Before shutdown,
remove both engines from router.nodes so router-held Raft handles cannot retain
stores. Retained TLS configs remain alive through the shutdown checks.

For M3, first complete normal learner catch-up, then insert one valid AddVoters
promotion in the temporary mutant before the existing post-catch-up metrics
assertions. Assert node1 voters remain {1} in normal code after catch-up as well
as at admission. This isolates an unintended promotion from NotLearner API
preconditions. No new assertion is added only in the mutant; restore source
byte-for-byte and report the actual observed catch.

D1's reply-only O2 recommendation needs security refinement before an owner
choice: a stolen CA key must not be able to forge both an enrollment endpoint
and its returned anchor. Also, discarding pins at Found alone can strand a learner
when the current leader's registry binding appears later in replay. Consider a
one-code-bound anchor and an applied fence without adding a second owner step.
No D1, D2 or D3 production decision is selected by this L1 approval.

Root lint disposition: OpenRaft0.10.0 learner_ids() is a deprecated alias that
delegates directly to non_write_voter_ids(). Use the latter in the two test
assertions; semantics are identical. No behavior test ran before the lint stop.
Root authorizes the mechanical correction and the unfinished verification once,
with no runtime tuning, warning suppression or retry of a failed engine test.
