# Brief: M3 next runtime boundary, founder identity bootstrap (F1)

Status: **proposed, corrected** (Opus 5.5 high, 2026-11-16). Original `3b4919f` on base `b52cd9c` (main `688f1ad`). This revision closes B1, B2 and the
test and wording corrections of the [Sol contract review](../reviews/m3-founder-bootstrap-sol-contract.md) (`c925d9c`). Docs only, nothing built or run. Root Sol
rechecks it before any source work. It amends no ADR and adds no architectural deviation. ADR 0005 (with its 2026-11-15 amendment), ADR 0023 and the
[issuance brief](m3-certificate-issuance-first-slice.md) with E1–E7 and V1–V9 stay binding. M2 stays open, M3 stays unchecked, and no M4 work follows.
Full M3 stays **unverified on real hardware**.

## Decision: founder identity comes before transport

Goal: M3's end state is real multi-host join, mTLS-authenticated Raft and remote execution. PR159 (the verifier, held) can authenticate a peer only if that
peer holds a CA-issued leaf whose SPKI the committed registry binds. Current source gives **no member that is not a joiner** any way to get one:
- `three_node::founders()` registers founders with fake digests `KeyDigest::of_spki(&[250, n])`. No key pair exists behind them.
- `IssueTicket` is built only in `issue.rs::committed()` from `Outcome::Enrolled(grant)`. `Command::Found` yields `Outcome::Founded { replay }`, which carries no
  grant. `ClusterCa::sign_leaf(&IssueTicket, ..)` therefore cannot sign a founder leaf at all.
- PR159 V7 fills that gap with the test-only `AuthenticatedPeer::assume(NodeId)` for founders. Every real handshake today needs a joiner on both sides.
- `Init { cluster, first_free }` and `raft.initialize(..)` exist only in test fixtures (`[1; 16]`, `first_free = 4`, three voters). No code creates a cluster ID,
  initializes exactly one voter, or founds the registry from a proven key. `StateStore::open` persists `Init` only into an empty store; on reopen it checks
  the cluster and ignores `first_free`.
- `JoinCode::generate(cluster, ca_fp, ..)` takes a caller-supplied `CaFingerprint`, and nothing derives that value from a CA certificate.

So the product flow ("a host initializes; others redeem codes") has no first host, and the first real mTLS connection has no server identity. Endpoint
separation needs a founding CA and server leaf. Key custody needs M2's dedicated daemon account (A7 FINAL NO-GO, A12 attempt consumed with cleanup
unconfirmed), so it is blocked on owner prerequisites while founding is not. Freshness policy is not needed for the first authenticated Raft connection.
F1 has no owner prerequisite, adds no dependency, and runs on a **single-voter** actual OpenRaft 0.10.0 node, avoiding the three-node election paths behind
the Windows quarantine and the PR159 holds.

## Settled design for F1 (source starts only after the parent prerequisites below)

1. **Cluster ID.** `found::generate_cluster_id(fill) -> Result<ClusterId, FoundError>`. The caller supplies randomness, as in `JoinCode::generate` (the
   production adapter is `getrandom::fill`). A fill error or an all-zero ID gives `Rng`. `found::founder_init(cluster) -> Init { cluster, first_free: 2 }` is a
   convenience only; enforcement is item 4a. The founder is always `FOUNDER = NodeId(1)`, and node IDs stay never-reused.
2. **CA fingerprint.** `ClusterCa::fingerprint() -> CaFingerprint` is SHA-256 of `cert_der`. The pure `ca::fingerprint_of(der: &[u8])` computes the same value.
   Codes minted on a founded cluster use it. Joiner-side pinning is endpoint work and is out of scope here.
3. **Leaf subject, sealed (B1).** The trait lives in `issue.rs` beside its seal: a private `mod sealed { pub trait Sealed {} }` and
   `pub(in crate::cluster) trait LeafSubject: sealed::Sealed { fn cluster(&self) -> ClusterId; fn node(&self) -> NodeId; fn key(&self) -> KeyDigest; }`.
   `ca.rs` imports `issue::LeafSubject` (as it already imports `IssueTicket`) and never names `sealed`. Only `issue.rs` (`IssueTicket`) and its `#[path]` child
   `found.rs` (`FounderTicket`) can name `issue::sealed`, so only they implement it. Unrelated `cluster` modules can call `LeafSubject` methods but cannot
   implement it. `IssueTicket`'s existing inherent accessors stay; its trait impl delegates to them. `IssueTicket` is still constructed only in `committed()` and
   the existing `#[cfg(test)] test_ticket`. `sign_leaf<S: LeafSubject>(&self, subject: &S, csr, now_ms)` keeps its body. Its validity arithmetic moves into a pure
   `ClusterCa::leaf_fits(now_ms) -> bool`, which `sign_leaf` calls (one formula, so a mutant cannot split the two). `enroll`, `committed`, `issue_guard`
   and the generic `ConsensusNode::start(node, dir, init, params, network)` stay byte-identical. `consensus.rs` is not edited and gains no field.
4. **Founding call.** `ConsensusNode::found(&self, ca: &ClusterCa, clock: &dyn SignerClock, addr: BasicNode, csr: &[u8]) -> Result<IssuedLeaf, FoundError>`
   lives in `consensus/found.rs`, declared in `issue.rs` as `#[path = "found.rs"] mod found;`. Like `enroll`, the first statement is
   `let deadline = Instant::now() + ISSUE_BUDGET;`, then `try_acquire` an issue permit (`Busy`). Everything after the permit is one `work` future, finished by
   `match timeout_at(deadline, work).await { Ok(r) if Instant::now() < deadline => r, _ => Err(FoundError::Unavailable) }`. Inline CSR parsing, signing and
   clock calls are not preemptible; a result completed at or after the deadline is discarded, so no leaf DER is ever returned late.
   a. **Pre-initialize checks (B2).** Require `self.id == 1` (`NotFounder`). Then one bounded `blocking` read of the local `StateStore` (`self.sm`, lock held
      only for the read) takes `cluster = codec::cluster(state)`, `state.founded()`, `state.next_node_id()`, `applied()` and `membership()`. Founded, or any
      applied position or membership, gives `AlreadyInitialized`. `next_node_id() != 2` gives `FounderAllocation`. Parse `ProvenCsr` (`BadCsr`). Require
      `ca.leaf_fits(clock.now_ms())` (`CaExpiring`). **All happen before any Raft call**, so a refusal leaves the node uninitialized. The invariant is
      checked against persisted state on every call, not a caller convention or startup metadata: a directory created with any other `first_free`
      (1, 4, ...) is refused forever, and a restart with any matching-cluster `Init` sees the persisted value because reopen ignores `first_free`.
   b. `raft.initialize(BTreeMap::from([(1, addr)]))`. `Err(RaftError::APIError(InitializeError::NotAllowed(_)))` gives `AlreadyInitialized` with nothing
      proposed. Any other error gives `Unavailable`. This is the only initialize call in production code, and it never re-initializes. If the outer deadline
      drops this await, the engine may still initialize; a later `found` then refuses (item 5). `addr` is opaque here; address validation is endpoint work.
   c. Leader wait, OpenRaft 0.10.0 API as cached locally: `raft.wait(Some(deadline.saturating_duration_since(Instant::now()))).metrics(|m| m.current_leader
      == Some(1) && m.state == openraft::ServerState::Leader, "founder leader")`. `WaitError::Timeout` or `ShuttingDown` gives `Unavailable`. Then
      `propose_response(Found { cluster, members: [(1, csr.digest())], proposed_at_ms: t0 = clock.now_ms() })`, where `Busy` gives `Busy` and
      `NotLeader`, `Stopped` or `Unknown` give `Unavailable`.
   d. `committed_founder(resp)` requires `resp.log_id.leader_id.node_id == self.id` (`WrongNode`) and exactly `Founded { replay: false }`. `replay: true` gives
      `Rejected(AlreadyFounded)`, `Rejected(r)` gives `Rejected(r)`, and anything else gives `Rejected(UnknownCode)`. It records `(leader, index)` in a
      `FounderTicket { cluster, key, leader, index }` whose fields and constructor are private to `found.rs`.
   e. Guard 1 is `fresh_read` with leader == `ticket.leader` (`StaleIssuer`) and `authorize_peer(cluster, 1, key)`. `WrongCluster` gives `Rejected`, and the
      rest give `Revoked`. Only after it returns, sample `pre_now`, require `pre_now >= t0` (`ClockRollback`), and sign with no await in between.
   f. Guard 2 repeats the guard. Then sample `post_now >= pre_now` (`ClockRollback`) and return `IssuedLeaf { der, node: 1 }`. A failed guard drops the DER.
   `FoundError` = `Busy | Unavailable | NotFounder | AlreadyInitialized | FounderAllocation | Rng | BadCsr(CsrError) | Rejected(Reject) | WrongNode |
   StaleIssuer | Revoked | ClockRollback | CaExpiring | Sign`, with static Display. `registry.rs` gains only read-only
   `pub(in crate::cluster) fn founded(&self) -> bool` and `fn next_node_id(&self) -> NodeId` on `EnrollmentState`.
5. **Not resumable, stated honestly.** Once `initialize` succeeds, a later failure (budget, sign, guard) leaves a founded or half-founded directory that
   `found` refuses with `AlreadyInitialized`. Recovery is a fresh directory and a new cluster ID chosen by the caller. No code path deletes, truncates, renames
   or overwrites the old directory, automatically or on error. With no custody, the founder key and CA live only in memory and are lost at process exit.

## Security invariants (each one a review item)

- **No production CA.** `ClusterCa` keeps only `#[cfg(test)]` constructors. `found` takes `&ClusterCa`, so production builds cannot call it yet. No CA bytes
  appear in `Found`, entries, forwards, state, snapshots, logs, errors, evidence or MCP. No custody, keychain or daemon isolation is claimed or simulated.
- **Founder binding.** The founder leaf is signed only for the single committed `(1, digest)` pair from `ProvenCsr`, only after this node's own commit in
  this term, and between two fresh guards. The signer still builds every certificate parameter itself.
- **Sealed subjects.** Only the two trusted adapters can produce a `LeafSubject`; ticket construction stays restricted to `issue.rs` and `found.rs`.
- **Founder key.** The founder's private key is the caller's (here, the test's). `found` receives CSR bytes only.
- **One initialization.** Initialize happens only on an empty node with persisted `next_node_id == 2`, never again. Exactly one founder exists, and the first
  joiner is node 2. Multi-founder `Found` stays a test fixture.
- **Unchanged.** Per-connection registry binding (PR159), unbounded local staleness, and Raft peer traffic needing no quorum. F1 adds no endpoint, listener,
  socket, owner approval, membership change or renewal.

## Allowlist (F1, Sol high; Opus reviews)

`consensus/found.rs` (new), `consensus/found_tests.rs` (new; `#[cfg(test)] #[path = "found_tests.rs"] mod found_tests;` in `found.rs`), `consensus/issue.rs`
(sealed module, `LeafSubject`, `IssueTicket` impl, `found` binding only), `enrollment/registry.rs` (the two accessors), `pki/ca.rs` (`LeafSubject` bound,
`fingerprint`, `fingerprint_of`, `leaf_fits`), `pki/tests.rs`, `tools/ci/test_windows_cluster_quarantine.py`, `docs/evidence/m3-founder-bootstrap.md` and
`docs/PROGRESS.md`. **No Cargo or lock change** (so no `trybuild`). `consensus.rs`, `three_node.rs`, `mem_net.rs`, `pki.rs` and `pki/verify.rs` are untouched,
which keeps conflicts with PR159 to the `issue.rs` tail. The Python file scans `found_tests.rs` with the same `ignore`/platform-`cfg` mutation set, asserts
its binding exactly once with a remove-binding mutant, and pins the seal: `issue.rs` holds exactly one non-`pub` `mod sealed`, and across `crates/arxd/src`
the only `Sealed for` impls are `IssueTicket` in `issue.rs` and `FounderTicket` in `found.rs`. Mutants `pub mod sealed`, `pub(crate) mod sealed`, a
`LeafSubject` moved out of `issue.rs`, and an extra impl in another file must fail it. Rust privacy is the guarantee; the scan guards its preconditions.

## Tests (actual OpenRaft, one voter, mem_net `Factory`, private temp dir, each ≤ 20 s; ordinary required tests on all three OS)

Each test fails on the stated mutant. "Uninitialized" means engine metrics show `last_log_index == None`, `current_term == 0` and `current_leader == None`,
and the state store shows no applied position and `founded() == false`.
- **F1 happy.** On an empty directory started with `founder_init`, `found` issues node 1. The leaf verifies (x509-parser `verify_signature`) under the CA and
  fails under a second CA. It has SAN `node_uri(cluster, 1)`, leaf SPKI raw == CSR SPKI raw, and DER ≤ 4096 B. A linearizable read gives `authorize_peer(1,
  digest)` Ok, `(1, other)` KeyMismatch and `(2, digest)` NotMember, `next_node_id() == 2`, and the Raft voters are `{1}`. *Mutant: founding with the fake
  digest or without Found fails the registry assertion.*
- **F2 once.** A second `found` (same or different CSR) gives `AlreadyInitialized`, with the last log index and `ca.signs` unchanged. Two concurrent
  `found` calls (`tokio::join!`) give exactly one leaf, one `AlreadyInitialized` and one Found entry. *Mutant: dropping the founded pre-check and treating
  `NotAllowed` as success proposes a second Found, which the index assertion catches; F6 pins the `NotAllowed` mapping alone.*
- **F3 refuse before initialize.** On node id 2, `found` gives `NotFounder` and the node stays uninitialized; IDs are immutable, so no same-node follow-up.
  On node 1, a bad CSR gives `BadCsr` and a CA from `ephemeral_with_validity(.., 30 d + 59 s)` gives `CaExpiring`. Each leaves it uninitialized, and a following
  correct `found` on that node succeeds. *Mutant: moving parse or `leaf_fits` after initialize makes the follow-up `AlreadyInitialized`.*
- **F3b allocation (B2).** Node 1 started through the generic `start` with `first_free` 1, and separately 4, gives `FounderAllocation` and stays
  uninitialized; a restart of that directory with `founder_init` still gives `FounderAllocation`. *Mutant: removing the check founds with first_free 4.*
- **F4 durable registry.** Shut down (`Ok`), restart on the same directory, and `authorize_peer(cluster, 1, digest)` passes on a fresh read; `found` gives
  `AlreadyInitialized`. This proves registry and Raft state only. The CA and founder key are test values kept in memory across the restart; no CA or key
  custody or recovery is claimed. Test-only restart; store advisory 1 stays as recorded.
- **F5 join on the founded cluster.** Mint `JoinCode::generate(cluster, ca.fingerprint(), ..)`, and `enroll` a joiner CSR on node 1, which issues node 2.
  `code.ca_fingerprint() == fingerprint_of(ca.cert_der())`, which differs from a second CA's. Both leaves chain to the same CA, with distinct SANs and serials,
  and the registry shows `{1, 2}`.
- **F6 adapter units.** Crafted responses: another leader node gives `WrongNode`, `Founded { replay: true }` gives `Rejected(AlreadyFounded)`, `Enrolled` gives a
  refusal, and a ticket with a different leader term gives `StaleIssuer` from the guard. A pure `init_refusal` maps a crafted `NotAllowed { last_log_id:
  None, vote }` to `AlreadyInitialized` and `NotInMembers` to `Unavailable`.
- **F7 clock, deterministic.** A `ScriptClock` returns a fixed value per call index (calls: `leaf_fits`, `t0`, `pre_now`, `post_now`) and is never stepped
  externally. At each call it records the node's `read_blocking` count (shared `Arc`), so the test proves `pre_now` is read only after guard 1's read completed
  (count 1) and `post_now` only after guard 2's (count 2). Script `[n, 1000, 999]` gives `ClockRollback` with `ca.signs` unchanged;
  `[n, 1000, 1000, 999]` gives `ClockRollback` with one sign and no leaf. Exact call counts are asserted. *Mutant: sampling `pre_now`
  before the guard await fails the recorded count.*
- **F8 privacy.** Store files contain no `ca.private_marker()`, no founder PKCS#8 and no CSR signature bytes. A positive control finds the founder digest bytes.
- **F9 post-deadline.** The `post_now` call blocks the thread for `ISSUE_BUDGET + 200 ms` (non-preemptible inline work) and then returns a valid time:
  `found` gives `Unavailable`, `ca.signs == 1`, and no leaf. *Mutant: dropping the final elapsed check returns the leaf.*
- **Pure (`pki/tests.rs`).** For `generate_cluster_id`, deterministic fill bytes pass through, an all-zero fill gives `Rng` and a fill error gives `Rng`.
  `leaf_fits` returns false at `30 d + 59 s` and true at `+ 60 s`, matching `sign_leaf`. `fingerprint` is stable and distinct per CA. Display text has no markers.
- Existing T1–T4, U6, crash probes, I1–I9 and A0 R1–R5 pass unmodified. Module path `cluster::consensus::issue::found::found_tests` stays outside
  `^cluster::consensus::three_node::`, so the sampler's exact 8 and the 8 `ignore` attributes are unchanged. No retry, `ignore`, filter, deadline, helper or
  quarantine change, and a red follows the pre-registered red handling with no rerun. Measure added wall time and Windows `verify` time, and keep the 660 s rule.

## Parent prerequisites and acceptance limits

- Source starts only after PR161 merges (no merge ahead of it), exact-main CI on that merge SHA is green, and root Sol accepts this corrected contract. F1
  does not need PR159. It must not change PR159, `work/m3-windows-phase-attribution` (`0a81f12`), the hold policy, or retry/deadline/quarantine state.
- Acceptance claims for F1 are limited to single-process founding on a temp directory. There is no multi-host behavior, transport, custody or restartable
  founder key, and nothing is verified on real hardware.

## After F1 (ordered, not authorized here)

- **F2** (after PR159 merges): a real-identity mutual TLS 1.3 handshake between F1's founder leaf and an F5 joiner leaf under PR159 configs, with
  `AuthenticatedPeer` giving 1 and 2 and no `assume`.
- **Membership workflow:** learner add, catch-up and guarded promotion, using authenticated mem_net identities.
- **Custody:** CA sealing and node key persistence under the daemon identity, once owner prerequisites exist.
- **Endpoint separation** and real sockets; **freshness policy** for control and execution traffic; **remote execution** of a build on another host.

## Unresolved owner and runtime prerequisites

- A daemon identity separated from the agent build account, with keychain/DPAPI/Secret Service access under it. This needs a qualifying owner environment and
  explicit scoped authorization. No agent performs owner-admin setup.
- Production CA and leaf lifetime, renewal, rotation, and trusted-clock/skew assumptions.
- Membership policy (when to promote, voter count) and the freshness policy for control and execution traffic.
- Real multi-host acceptance: CI containers or VMs are allowed by KICKOFF, and LAN/Tailscale paths stay **unverified on real hardware**. Windows three-node
  engine stability is unresolved (Sol fixes 3/3 exhausted, Opus recovery 0/2).

## First builder task

Sol (high) implements F1 exactly as above in one PR on the post-PR161 main: the allowlisted files only, all F1–F9 and pure tests, and evidence recording the
measured test and Windows times. Opus reviews it against this brief.
