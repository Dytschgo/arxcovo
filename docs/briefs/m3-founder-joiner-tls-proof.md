# Brief: M3 F2, founder and enrolled joiner prove mutual TLS with real identities

Status: **proposed** (Opus 5.5 high, 2026-11-17) on `a560c30c`, source = main `67cc62d5` (PR159 TLS merge). Docs only; nothing built, run or
resolved. Root reviews this contract; Sol (high) implements only after main CI run 44064366449 is green. No ADR is amended and no deviation is added.
ADR 0005 (2026-11-15 amendment), ADR 0023, ADR 0024 and the [runtime boundary brief](m3-next-runtime-boundary.md) (F1, "After F1: F2") stay binding.
M2 open, M3 unchecked, Windows recovery Sol 3/3 / Opus 0/2, no M4 work; everything here is **unverified on real hardware**.

## Goal and smallest vertical

Prove, in one process, that the leaf from the actual `ConsensusNode::found()` (node 1, founder CSR key) and the leaf from the actual `enroll()` on that founded
node (node 2, joiner CSR key) complete a full rustls TLS 1.3 mutual handshake under the PR159 `client_config`/`server_config`/`PeerVerifier`, with
`AuthenticatedPeer` giving 1 and 2 from the verified certificates. No `AuthenticatedPeer::assume`, `test_ticket`, `pki::leaf`, `pki::registry`,
`pki::connections` (one cert on both sides), fake founder digest (`of_spki(&[250, n])`) or three-node `Cluster`. One CA, one real single-voter OpenRaft
0.10.0 node (mem_net `Factory`, private temp dir), cluster `[1; 16]` (as in `found_tests`), in-memory handshake with no sockets.

## Registry-view constraint (resolved explicitly)

The joiner has **no** Raft node, store or local replica yet; learner add and catch-up are later work. Both verifiers (founder side and joiner side) therefore
use **one** registry: `founder.peer_registry()`, the founder's committed local view. This is claimed only as a one-process shared view. It proves that both real
leaves chain to the CA, carry the committed key bindings and are accepted by the founder's committed registry in both TLS roles. It does **not** prove a
joiner-side registry, replication to the joiner, a second host or freshness. No second `ConsensusNode`, joiner store or replicated state is faked, and no new
bootstrap or issuing authority is added. The only writes are F1's `Found` and F5's `Mint`/`Redeem` plus one registry `Remove` (registry revocation, not a Raft
membership change, per the ADR 0005 amendment).

## API feasibility (inspected at `67cc62d5`; no production edit needed)

- `tls_engine_tests.rs` is `#[cfg(test)] #[path] mod tls_engine_tests;` in `consensus/issue.rs`, so it descends from `issue` and `consensus`. It can name
  `super::found::{founder_init, FOUNDER}` (`pub(in crate::cluster) mod found`) and the `pub` methods `ConsensusNode::start`, `found`, `propose`, `enroll`,
  `peer_registry` and `shutdown` (`Result<(), Orphaned>`, `Ok` only when `sm_strong == 0`). Its existing `Clock` returns `pki::NOW` (1 000 000 ms).
- `peer_registry()` captures only `sm_weak` (`Weak<Mutex<StateStore>>`), locks the std mutex per call, and maps `PeerRefusal` to `VerifyRefusal`. Handshakes
  and direct registry calls therefore run inside `spawn_blocking`, as the existing tests do.
- `pki::{verifier, pump, refusal, FixedTime, NOW, node}` are `pub(in crate::cluster)`. `verifier` hardcodes cluster `[1; 16]`, which matches here. `pump`
  takes any `(ClientConnection, ServerConnection)`. `client_config`, `server_config`, `PeerVerifier::{client, server}` and `SERVER_NAME` are `pub`. So
  distinct client and server certs and keys can be wired in the test without changing `pki/tls_tests.rs` or any helper.
- `JoinCode::generate([1; 16], ca.fingerprint(), fill)`, `Command::Mint { .. }` and `RedeemInput` are used exactly as in `found_tests::f5_joiner_two`.
- Time: rustls verify time is `Arc::new(pki::FixedTime(pki::NOW / 1000))`, the signer clock's second (leaf not-before = 940 s, CA not-before = 880 s). No
  settings, clock or validity change.

## The test (one new required case, all OS)

`founder_joiner_mutual_tls_real_identities` in `consensus/tls_engine_tests.rs`, `#[tokio::test(flavor = "multi_thread", worker_threads = 4)]`, body
inside `timeout(Duration::from_secs(20), ..)`, no `cfg`/`cfg_attr`/`ignore` and no word "ignore" or `Cluster` in its text. Steps:
1. `private_temp()`, assert `security::private`. Start node 1 with `founder_init([1; 16])`. `ca = ClusterCa::ephemeral([1; 16], pki::NOW)`.
2. Founder: `fkey = KeyPair::generate()`, CSR from it, `found(&ca, &Clock, BasicNode::new("mem:1"), csr)` gives node `FOUNDER`.
3. Joiner: `jkey`, CSR; generate the code, `propose(Mint)`, `enroll(..)` gives node 2. Joiner CSR key ≠ founder CSR key.
4. **Chain/key bindings.** For each leaf: SPKI raw == its `KeyPair::subject_public_key_info()`; SAN == `node_uri([1; 16], n)`; the two serials differ.
   Through `peer_registry()` itself: `(1, fd)` and `(2, jd)` Ok, `(1, jd)` and `(2, fd)` `KeyMismatch`, `(3, jd)` `NotMember`.
5. Build four configs once and keep them: founder server (founder leaf/key, `verifier(&ca, reg).server()`), joiner client (joiner leaf/key,
   `.client(node(1))`), founder client (`.client(node(2))`) and joiner server, all with the same `FixedTime` and `reg = node.peer_registry()`.
6. **Positive, both roles.** Joiner client → founder server: `pump` gives `(from_client, from_server) == (1, 2)`. Founder client → joiner server gives `(2, 1)`.
   Both complete TLS 1.3 (asserted inside `complete`) with full certificate verification and the peer's CertificateVerify under the CSR key.
7. **Target counterfactual.** A joiner client built with `.client(node(2))` against the founder server gives `TargetMismatch` (the actual node-1 SAN).
8. **Registry counterfactual and replay.** `propose(Remove { node: 2 })` gives `Removed { replay: false }`. New connections from the **same retained**
   step-5 configs: joiner client → founder server gives `NotMember` (server-side client verification); founder client → joiner server gives `NotMember`;
   `(1, fd)` stays Ok. Same certs, keys, CA and configs, so only committed registry state changed, and nothing cached or resumed authorization.
9. **Cleanup while configs are held.** With all retained configs alive, `shutdown().await` is `Ok(())` (verifiers hold only `Weak`). A new connection from
   the retained joiner client and founder server then gives `Unavailable`. Then drop the configs. Nothing is detached and no helper owns the store.

## Guard changes (`tools/ci/test_windows_cluster_quarantine.py`)

- `TLS_REQUIRED` gains the new name, so it is pinned as a required `tokio::test` with no gate. The existing loop's gate mutant then covers it, and
  `tokio::test` count = 3 + 2. Add the inventory-drift mutant `('TLS_REQUIRED', TLS_REQUIRED[:-1], ..)`.
- New `assert_real_identity_body(source)`, called from `assert_tls_engine_quarantine`. It takes the new test's segment (same `fn` split used for
  constructors) and requires `founder_init(`, `.found(`, `.enroll(`, `peer_registry()` and `pki::pump(`. It forbids `assume(`, `test_ticket`,
  `pki::leaf(`, `pki::registry(`, `pki::connections(`, `handshake(`, `Cluster` and `[250`. Mutants: each forbidden token injected into the body, and each
  required token removed, must fail. The `three_node` import count stays 1 and the constructors stay `TLS_FIXTURE`.
- Unchanged: quarantine total **27** (8 + 5 + 11 + 3), sampler exactly **8**, the 8 `REASON`/`REASON_FIXTURE` gates, all 13 founder tests (F1–F10 incl.
  F3b and three F10), and the 12 existing required/pure TLS tests (10 `pki/tls_tests.rs`, `live_verifier_..`, `tls_single_node_directory_precondition`).

## Mutant and replay proof

Run once each, only against the new test; each must fail at the named step; revert so production files show no diff, and record the result in evidence:
- **N1** `peer_registry` closure returns `Ok(())` unconditionally: step 4 `KeyMismatch` and step 8 `NotMember` fail. (On Windows, the only required catch.)
- **N2** `AuthenticatedPeer::from_verified_certificates` returns a constant node 1: step 6 `(.., 2)` fails. Existing tests use node 4 on both sides and miss it.
Already caught by unchanged tests and **not re-run**: registry call skipped in `authorize`, target check dropped, strong store capture in `peer_registry`
(`live_verifier_..`), resumption enabled (`reused_configs_..`/`production_*`), fake founder digest (F1), wrong SAN node (F5). Replay proof = step 8 (same
configs re-verify against changed committed state) plus step 9 (same configs give `Unavailable` after store release).

## Allowlist (Sol high; Opus reviews)

`crates/arxd/src/cluster/consensus/tls_engine_tests.rs` (`#[cfg(test)]` file: the one test and its imports only), `tools/ci/test_windows_cluster_quarantine.py`,
`docs/evidence/m3-founder-joiner-tls.md` (new), `docs/PROGRESS.md`, and a status line in this brief. **No** production feature code, `pki/tls_tests.rs`
helpers, Cargo/lock, workflow, sampler, nextest config, timeout, retry, transport, helper or scheduling change. If implementation finds a concrete API gap,
stop and report it before building; do not work around it with a forged ticket or production edit.

## Claims and limits

Claimed: one process, one founder node, real F1 founder and F5 joiner leaves from distinct CSR keys under one test CA authenticate each other as 1 and 2 over
in-memory TLS 1.3, both against the founder's committed registry, which revokes the joiner on `Remove`. **Not claimed:** joiner replica or verifier state,
multi-host or LAN/Tailscale, sockets or endpoint separation, production CA, custody, learner/membership change, freshness policy, renewal, or anything on
real hardware.

## Local verification (one round) and gates

One round on the isolated branch: `cargo fmt --check`, workspace clippy `-D warnings` with tests, nextest of the new test plus the 13 founder and 12 TLS tests
(record each wall time; the new test is expected well under 20 s), `python -m unittest tools/ci/test_windows_cluster_quarantine.py`, then N1/N2. A natural
red stops the round for disposition, with no rerun, filter, deadline or source tuning. Publication needs: fresh green exact main (run 44064366449 or later),
independent Opus review of the diff against this brief, and the normal five required gates, with Windows `verify` ≤ 660 s. Root merges only after all pass.


## Root contract check - 2026-11-17

Accepted for test-only implementation after green exact main. For N1, preserve
Weak upgrade, lock and poison refusals, replacing only the locked registry
`authorize_peer` result with Ok. This isolates authorization bypass from existing
Unavailable ownership tests. Run each mutant only against the new test and
claim only the observed catch; do not claim it is the only catching test without
running the other tests. Restore both files byte-for-byte afterward. Existing
resumption tests already pin full handshakes; the new pump path must use the
unchanged production config builders and distinct real leaves. No helper change.

Main prerequisite satisfied: root verified exact67cc62d CI44064366449 attempt1
SUCCESS all3platforms, wall610s / Windows606s. Local implementation may start.
