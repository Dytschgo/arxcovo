# Brief: M3 next runtime boundary, founder identity bootstrap (F1)

Status: **proposed** (Opus 5.5 high, 2026-11-16, branch base `b52cd9c`, main `688f1ad`). Docs only. Nothing here is built or run, and root Sol checks it
before any source work. It amends no ADR and adds no architectural deviation. ADR 0005 (with its 2026-11-15 amendment), ADR 0023 and the
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
  initializes exactly one voter, or founds the registry from a proven key.
- `JoinCode::generate(cluster, ca_fp, ..)` takes a caller-supplied `CaFingerprint`, and nothing derives that value from a CA certificate.

So the product flow ("a host initializes; others redeem codes") has no first host, and the first real mTLS connection has no server identity. The alternatives
depend on this slice or are blocked:
- **Endpoint separation** needs a server leaf for the enrollment endpoint, and the code's CA fingerprint must hash a real founding CA.
- **Key custody** needs the daemon identity separated from the agent build account. That is M2's dedicated-account work, which stays open: A7 FINAL NO-GO, the
  A12 attempt consumed with cleanup unconfirmed. Custody is blocked on owner prerequisites, while founding is not.
- **Freshness** is a policy decision for control and execution traffic. It is not needed for the first authenticated Raft connection, and Raft traffic must
  not need a quorum.

F1 has no owner prerequisite, adds no dependency, and runs on a **single-voter** actual OpenRaft node. That avoids the three-node election paths behind the
Windows quarantine and the PR159 holds.

## Settled design for F1 (source starts only after the parent prerequisites below)

1. **Cluster ID.** `found::generate_cluster_id(fill) -> Result<ClusterId, FoundError>`. The caller supplies randomness, as in `JoinCode::generate` (the
   production adapter is `getrandom::fill`). A fill error gives `Rng`, and an all-zero ID gives `Rng`. `found::founder_init(cluster) -> Init { cluster,
   first_free: 2 }`. The founder is always `FOUNDER = NodeId(1)`, and node IDs stay never-reused.
2. **CA fingerprint.** `ClusterCa::fingerprint() -> CaFingerprint` is SHA-256 of `cert_der`. The pure `ca::fingerprint_of(der: &[u8])` computes the same value.
   Codes minted on a founded cluster use it. Joiner-side pinning is endpoint work and is out of scope here.
3. **Leaf subject, sealed.** `ca.rs` gains `pub(in crate::cluster) trait LeafSubject: sealed::Sealed { cluster(), node(), key() }`. The `Sealed` module is
   private to `issue.rs`, and only `IssueTicket` and `FounderTicket` implement it. `sign_leaf(&self, subject: &impl LeafSubject, csr, now_ms)` keeps its body.
   Its validity arithmetic moves into a pure `ClusterCa::leaf_fits(now_ms) -> bool`, which `sign_leaf` calls (one formula, so a mutant cannot split the two).
   `enroll`, `committed`, `issue_guard` and `IssueTicket` stay byte-identical apart from the trait impl.
4. **Founding call.** `ConsensusNode::found(&self, ca: &ClusterCa, clock: &dyn SignerClock, addr: BasicNode, csr: &[u8]) -> Result<IssuedLeaf, FoundError>`
   lives in `consensus/found.rs`, a `#[path]` child of `issue` so it can implement the sealed trait. Order (every await bounded by `deadline = now + ISSUE_BUDGET`):
   a. `try_acquire` an issue permit (`Busy`). Require `self.id == 1` (`NotFounder`). Parse `ProvenCsr` (`BadCsr`). Require `ca.leaf_fits(clock.now_ms())`
      (`CaExpiring`). **All four happen before any Raft call**, so a refusal leaves the node uninitialized.
   b. `raft.initialize({1: addr})`. `NotAllowed` gives `AlreadyInitialized` with nothing proposed. This is the only initialize call in production code, and
      it never re-initializes. `addr` is opaque here, and address validation belongs to the endpoint slice.
   c. Await the metrics until `current_leader == Some(1)` within the deadline (else `Unavailable`), then
      `propose_response(Found { cluster: self.cluster, members: [(1, csr.digest())], proposed_at_ms: t0 = clock.now_ms() })`.
   d. `committed_founder(resp)` requires that this node is the leader node and that the outcome is exactly `Founded { replay: false }`. `replay: true` gives
      `Rejected(AlreadyFounded)`, `Rejected(r)` gives `Rejected(r)`, and anything else gives `Rejected(UnknownCode)`. It records `(leader, index)` in a
      `FounderTicket { cluster, key, leader, index }` whose fields and constructor are private to `found.rs`.
   e. Guard 1 is `fresh_read` with leader == `ticket.leader` (`StaleIssuer`) and `authorize_peer(cluster, 1, key)`. `WrongCluster` gives `Rejected`, and the
      rest give `Revoked`. Only after it returns, sample `pre_now`, require `pre_now >= t0` (`ClockRollback`), and sign with no await in between.
   f. Guard 2 repeats the guard. Then sample `post_now >= pre_now` (`ClockRollback`) and return `IssuedLeaf { der, node: 1 }`. A failed guard drops the DER.
   `FoundError` = `Busy | Unavailable | NotFounder | AlreadyInitialized | Rng | BadCsr(CsrError) | Rejected(Reject) | WrongNode | StaleIssuer | Revoked |
   ClockRollback | CaExpiring | Sign`, with static Display. `ConsensusNode` gains `cluster: ClusterId`, set from `Init` in `start`.
5. **Not resumable, stated honestly.** Once `initialize` succeeds, a later failure (budget, sign, guard) leaves a founded or half-founded directory that
   `found` refuses with `AlreadyInitialized`. Recovery is a fresh directory and a new cluster ID. With no custody, the founder key and CA live only in memory and
   are lost at process exit. Restart persistence of the key belongs to the custody slice.

## Security invariants (each one a review item)

- **No production CA.** `ClusterCa` keeps only `#[cfg(test)]` constructors. `found` takes `&ClusterCa`, so production builds cannot call it yet. No CA bytes
  appear in `Found`, entries, forwards, state, snapshots, logs, errors, evidence or MCP. No custody, keychain or daemon isolation is claimed or simulated.
- **Founder binding.** The founder leaf is signed only for the single committed `(1, digest)` pair from `ProvenCsr`, only after this node's own commit in
  this term, and between two fresh guards. The signer still builds every certificate parameter itself.
- **Founder key.** The founder's private key is the caller's (here, the test's). `found` receives CSR bytes only.
- **One initialization.** Initialize happens only on an empty node, never again. Exactly one founder exists. Multi-founder `Found` stays a test fixture.
- **Unchanged.** Per-connection registry binding (PR159), unbounded local staleness, and Raft peer traffic needing no quorum. F1 adds no endpoint, listener,
  socket, owner approval, membership change or renewal.

## Allowlist (F1, Sol high; Opus reviews)

`consensus/found.rs` (new), `consensus/found_tests.rs` (new; `#[cfg(test)] #[path = "found_tests.rs"] mod found_tests;` in `found.rs`), `consensus/issue.rs`
(sealed trait, impl, `#[path]` binding of `found` only), `consensus.rs` (`cluster` field), `pki/ca.rs` (`LeafSubject` bound, `fingerprint`, `fingerprint_of`,
`leaf_fits`), `pki/tests.rs`, `tools/ci/test_windows_cluster_quarantine.py` (scan `found_tests.rs` with the same `ignore`/platform-`cfg` mutation set, and
assert its binding exactly once with a remove-binding mutant), `docs/evidence/m3-founder-bootstrap.md` and `docs/PROGRESS.md`. **No Cargo or lock change.**
`three_node.rs`, `mem_net.rs`, `pki.rs` and `pki/verify.rs` are untouched, which keeps conflicts with PR159 to the `issue.rs` tail.

## Tests (actual OpenRaft, one voter, mem_net `Factory`, private temp dir, each ≤ 20 s; ordinary required tests on all three OS)

Each test fails on the stated mutant.
- **F1 happy.** On an empty directory, `found` issues node 1. The leaf verifies (x509-parser `verify_signature`) under the CA and fails under a second CA. It has
  SAN `node_uri(cluster, 1)`, leaf SPKI raw == CSR SPKI raw, and DER ≤ 4096 B. A linearizable read gives `authorize_peer(1, digest)` Ok, `(1, other)` KeyMismatch and
  `(2, digest)` NotMember, and the Raft voters are `{1}`. *Mutant: founding with the fake digest or without Found fails the registry assertion.*
- **F2 once.** A second `found` (same or different CSR) gives `AlreadyInitialized`, with the last log index and `ca.signs` unchanged. *Mutant: skipping the
  `NotAllowed` check proposes a second Found, which the index assertion catches.*
- **F3 refuse before initialize.** Node id 2 gives `NotFounder`. A bad CSR gives `BadCsr`. A CA from `ephemeral_with_validity(.., 30 d + 59 s)` gives
  `CaExpiring`. Each leaves the node uninitialized (no vote, no log), and a following correct `found` succeeds. *Mutant: moving parse or `leaf_fits` after
  initialize makes the follow-up `AlreadyInitialized`.*
- **F4 durable founding.** Shut down (`Ok`), restart on the same directory with the same `Init`, and `authorize_peer(cluster, 1, digest)` passes on a fresh read.
  `found` gives `AlreadyInitialized`. This restart is test-only, and store advisory 1 stays as recorded.
- **F5 join on the founded cluster.** Mint `JoinCode::generate(cluster, ca.fingerprint(), ..)`, and `enroll` a joiner CSR on node 1, which issues node 2.
  `code.ca_fingerprint() == fingerprint_of(ca.cert_der())`, which differs from a second CA's. Both leaves chain to the same CA, with distinct SANs and serials,
  and the registry shows `{1, 2}`.
- **F6 adapter units.** Crafted responses: another leader node gives `WrongNode`, `Founded { replay: true }` gives `Rejected(AlreadyFounded)`, `Enrolled` gives a
  refusal, and a ticket with a different leader term gives `StaleIssuer` from the guard.
- **F7 clock.** A `FakeClock` stepped back between `t0` and `pre_now`, and again between `pre_now` and `post_now`, gives `ClockRollback` with no leaf returned.
- **F8 privacy.** Store files contain no `ca.private_marker()`, no founder PKCS#8 and no CSR signature bytes. A positive control finds the founder digest bytes.
- **Pure (`pki/tests.rs`).** For `generate_cluster_id`, deterministic fill bytes pass through, an all-zero fill gives `Rng` and a fill error gives `Rng`.
  `leaf_fits` returns false at `30 d + 59 s` and true at `+ 60 s`, matching `sign_leaf`. `fingerprint` is stable and distinct per CA. Display text has no markers.
- Existing T1–T4, U6, crash probes, I1–I9 and A0 R1–R5 pass unmodified. Module path `cluster::consensus::issue::found::found_tests` stays outside
  `^cluster::consensus::three_node::`, so the sampler's exact 8 and the 8 `ignore` attributes are unchanged. No retry, `ignore`, filter, deadline, helper or
  quarantine change, and a red follows the pre-registered red handling with no rerun. Measure added wall time and Windows `verify` time, and keep the 660 s rule.

## Parent prerequisites and acceptance limits

- Source starts only after PR161 merges (no merge ahead of it), exact-main CI on that merge SHA is green, and root Sol accepts this contract. F1 does not need
  PR159. It must not change PR159, `work/m3-windows-phase-attribution` (`0a81f12`), the hold policy, or retry/deadline/quarantine state.
- Acceptance claims for F1 are limited to single-process founding on a temp directory. There is no multi-host behavior, transport, custody or restartable
  founder key, and nothing is verified on real hardware.

## After F1 (ordered, not authorized here)

- **F2** (after PR159 merges): a real-identity mutual TLS 1.3 handshake between F1's founder leaf and an F5 joiner leaf under PR159 configs, with
  `AuthenticatedPeer` giving 1 and 2 and no `assume`.
- **Membership workflow:** learner add, catch-up and guarded promotion, using authenticated mem_net identities.
- **Custody:** CA sealing and node key persistence under the daemon identity, once owner prerequisites exist.
- **Endpoint separation** and real sockets.
- **Freshness policy** for control and execution traffic.
- **Remote execution:** one host running a build on another.

## Unresolved owner and runtime prerequisites

- A daemon identity separated from the agent build account, with keychain/DPAPI/Secret Service access under it. This needs a qualifying owner environment and
  explicit scoped authorization. No agent performs owner-admin setup.
- Production CA and leaf lifetime, renewal, rotation, and trusted-clock/skew assumptions.
- Membership policy (when to promote, voter count) and the freshness policy for control and execution traffic.
- Real multi-host acceptance: CI containers or VMs are allowed by KICKOFF, and LAN/Tailscale paths stay **unverified on real hardware**. Windows three-node
  engine stability is unresolved (Sol fixes 3/3 exhausted, Opus recovery 0/2).

## First builder task

Sol (high) implements F1 exactly as above in one PR on the post-PR161 main: the allowlisted files only, all F1–F8 and pure tests, and evidence recording the
measured test and Windows times. Opus reviews it against this brief.
