# M3 PR B TLS verifier: independent Opus review

2026-11-16. Anthropic / Claude Opus 5.5 / high, independent review sub-agent of a Sol-built change, single agent, no children. Only this file is
committed. No source, PROGRESS, brief or ADR edit; no push, PR, status, comment, merge, watch, hosted dispatch, install, native M2, VM/admin or old
Lab Manager action.

**Verdict: CHANGES REQUIRED.** One blocking security defect (B1: TLS 1.3 session resumption bypasses the registry and expiry), plus two required
test gaps (R1 server-side chain/time has no independent evidence, R2 V2 exact native time variants not asserted). The rest of the slice matches the
brief. The private-fixture correction and the helper rename are accepted.

## Pins

- Reviewed head `4186d102dc8b413e8e186b3c8b287d0723897da6` (`work/m3-tls-verifier-boundary`), checked out in an isolated worktree on branch
  `t3/review-tls-verifier-pr-b`. Clean tree. `git merge-base --is-ancestor` confirmed for live/main prepared ancestor
  `6231da98b4aa902669e268be89de86b273d1bd16` and prepared docs base `71607f53caf69f1f6a4582d61fa8e993076b717a`.
- Source diff `71607f5..4186d10`: `verify.rs` (+315), `pki/tls_tests.rs` (+321), `consensus/tls_engine_tests.rs` (+334), `issue.rs` (+27),
  `mem_net.rs` (+14/-1), `pki.rs` (+6), `arxd/Cargo.toml` (+1), `Cargo.lock` (+26), quarantine guard (+8/-1), plus docs.
  Commits: `bacb3e4` feat, `40300f9` docs, `34902ca` fixture, `d1c207d` rename, `4186d10` docs.
- Read: AGENTS.md, KICKOFF routing (Sol-built → Opus reviews), CONTRIBUTING.md, the review skill, the issuance brief (decisions 1-7, PR B,
  V1-V9, root API/trust clarifications, private-fixture correction), the B readiness review, the verifier evidence, and the actual rustls 0.23.46
  / rustls-webpki 0.103.16 sources in the local cargo registry.
- Pre-mutation source SHA-256: `verify.rs 81b7f15a…06b2`, `issue.rs adc81bce…f293458`, `mem_net.rs ca8501ac…6b66e2`,
  `tls_tests.rs 11a6bac4…3991b`, `tls_engine_tests.rs d3e4f878…f05b`. Every mutant and probe restored its file to the same hash, and
  `git status` was clean after each one.

## Actual runs (this review) versus reused evidence

Fresh target `D:/arxcovo-opus-tlsb-4186d10/target`, TEMP/TMP `D:/arxcovo-opus-tlsb-4186d10/tmp`, rustc/cargo 1.95.0, `--locked --offline`.
Command for each run: `cargo test -p arxd --locked --offline --lib -- tls_tests tls_engine_tests`. Logs in `D:/arxcovo-opus-tlsb-4186d10/logs`
(not committed).

- **Unmodified control, once:** 9 passed / 0 failed / 566 filtered, 2.00 s test time, 68 s including the fresh build (`control.log`
  `e2db82a7…c263`). No natural red occurred.
- **13 behavioral mutants** and **2 resumption probes** (tables below). Each was one isolated edit with a hash-verified restore.
- Lock identities: Python `tomllib` comparing (name, version, source, checksum) of `6231da9:Cargo.lock` against HEAD gives 281 → 283.
  Added: exactly rustls 0.23.46 `48e13bd8…8fdf` and rustls-webpki 0.103.16 `4443b1b6…b281`. Removed or changed: none.
- Quarantine guard `python -m unittest tools/ci/test_windows_cluster_quarantine.py`: 2 passed. (`-I` cannot import a path module; that run
  reported an import error, not a test failure.) `three_node.rs` and `consensus/tests.rs` are unchanged since `6231da9`. `three_node.rs` still has
  8 `ignore =` attributes. The sampler FILTER `windows_diag_sample.py:58` is unchanged.
- **Reused, not rerun:** root's 145/145 cluster nextest, 145 in-process tests, Clippy, fmt, sampler 58 and Node 29 (logs `D:/Temp/arx-tls-*`, present).
  I also reused the focused privacy-precondition red (`D:/Temp/arx-tls-private-precondition-red.log`: `tls_single_node_directory_precondition`
  FAILED, 0/1) and the builder's first `Open(Io)` red. Both stay historical, with no natural-cause claim from me. No broad, cluster-wide,
  three_node, D1 or quarantined test was run.

## Security

- **B1 (blocking): resumption bypasses every per-connection check.** `client_config` and `server_config` are non-test public builders, and they
  keep rustls defaults: client `Resumption::in_memory_sessions(256)`, server `ServerSessionMemoryCache::new(256)` with `send_tls13_tickets: 2`
  (`rustls/src/server/builder.rs:113,123`, `client/client_conn.rs:533`). On a TLS 1.3 PSK resumption the server copies
  `resume.client_cert_chain` into `peer_certificates` without calling `verify_client_cert` (`server/tls13.rs:371-375`). The client
  "*don't* reverify the certificate chain" (`client/tls13.rs:573-581`). V1 requires one client config per target, so reusing a config is the
  intended pattern.

  Probe P1 reused one Arc'd config pair. After a full handshake, the registry was flipped to `NotMember`. Fresh configs then refused with
  `InvalidCertificate(Other(NotMember))`, but the same configs gave `Ok((4, 4, Resumed, Resumed))`. Probe P2: a full handshake at
  `not_after - 10 s`, then a resume at `not_after + 3600 s`, gave `Ok((4, 4, Resumed, Resumed))`, while fresh configs gave
  `ExpiredContext{time: 2596540, not_after: 2592940}`.

  So a removed member, or one whose certificate has expired, is re-authenticated, and `AuthenticatedPeer::from_*` returns the cached identity. This
  contradicts the ADR 0005 every-connection binding, the "removed node after local apply" refusal, V3's "next verification refuses", and the
  decision 5 stale-acceptance bound. Each resumption issues new tickets, so the chain plausibly continues without limit. That last point is
  inferred from source, not run. There is no live exploit in this slice (no transport), but these builders are the API the transport will use.

  **Required fix** (source-close work, not implemented here): set `config.resumption = Resumption::disabled()` on the client and
  `session_storage = Arc::new(NoServerSessionStorage {})`, `send_tls13_tickets = 0` on the server. Add a test that reuses one config pair and asserts
  `handshake_kind() == Some(Full)` on the second connection, plus `NotMember` after the registry flip and `Expired*` after expiry.
- **Order is correct.** Size and intermediate checks run before any parse (`verify.rs:125-137`). Native webpki chain, EKU and time checks run
  before `peer_identity` on both sides (`:159-170`, `:216-218`). The TLS 1.3 CertificateVerify runs natively after `verify_*_cert` returns, as
  rustls sequences it. Peer certificates are published only after CertificateVerify on full handshakes (`client/tls13.rs:1219`,
  `server/tls13.rs:1214`), and identity is exposed only via completed TLS13 connections. **Limit:** the registry callback (store lock) runs before
  proof of possession, so a party replaying a public cluster-issued certificate can cause one bounded lock acquisition. That is not a bypass.
- Target binding (V1) is enforced after the registry. A `server()` verifier used as a client verifier fails closed (`TargetMismatch`).
- `peer_registry` captures only the weak store reference, maps poison and dead stores to `Unavailable`, maps `PeerRefusal` exhaustively, and drops
  the guard and then the Arc before returning. A shutdown racing a live verify can observe a transient strong ref (an honest `Orphaned`, not a leak).
- Full DER parse with empty remainder, one SAN extension, one URI GeneralName, strict URI, cluster check and raw-SPKI digest are all present.
  Mandatory client auth is set; the public-CA-only root store has no system roots; TLS 1.2 signatures refuse; there is no global provider and no
  implicit builder. `Debug` and refusals are static. Sender provenance (`check_sender`, `MemRouter.authenticated`) is test-only (`mem_net` is
  `#[cfg(test)]`).

### Mutants (S = survived, K = killed)

| ID | Mutation | Result |
| --- | --- | --- |
| M1 | drop server-side `inner.verify_client_cert` (chain/EKU/time) | **S** 9/9 |
| M2 | drop server-side `authorize` | K: only the poison test |
| M3 | drop `is_handshaking()` in both `from_*` | **S** |
| M4 | drop the duplicate SAN extension check | **S** (webpki `ExtensionValueInvalid` runs first) |
| M5 | drop the empty-remainder check | **S** (webpki `TrailingData` runs first) |
| M6 | drop the verifier's own cluster check | **S** (every test registry also checks cluster) |
| M7 | disable the target check | K: pure and engine target tests |
| M8 | registry captures a strong store Arc | K: `shutdown` → `Orphaned{sm_strong:1}` |
| M9 | poison fails open | K: `NotMember` ≠ `Unavailable` |
| M10 | `mem_net` ignores the authenticated identity | K: sender test times out waiting for a new leader |
| M11 | `client_auth_mandatory` false | K: `NoCertificatesPresented` assertion |
| M12 | registry result ignored | K: 3 tests |
| M13 | server-cert TLS13 signature accepts all | K: tampered-signature test |

## Spec

- **R1 (required): the server-side chain/time check has no independent evidence (M1).** All handshakes present the same valid leaf on both sides,
  so the client side refuses first. Direct `verify_client_cert` tests cover only `Intermediates`, `Oversized` and the poison path (M2's sole
  killer). Needed: direct server-side refusals for a foreign same-DN CA client certificate, a self-signed one, expired and not-yet-valid, and
  `NotMember`/`KeyMismatch`/`WrongCluster` through `verify_client_cert` (brief "Refused" list and V2 "through direct verifier calls").
- **R2 (required): V2 exact variants are not asserted.** V2 says "`not_before - 1 s` gives NotValidYet and `not_after + 1 s` gives Expired".
  `bounds_root_key_time_and_static_errors` asserts only `InvalidCertificate(_)`, which a `TargetMismatch` or any other refusal also satisfies.
  rustls 0.23.46 yields `NotValidYetContext`/`ExpiredContext` (seen in P2). The exact-bound passes are correct (940 and 940 + 30 d).
- A1 (advisory): the self-signed and trailing/truncation tests assert only `is_err()`, and M4/M5 show the custom checks are unreachable behind
  webpki. Acceptable as defense in depth, but they should not be credited as tested. M6 could be covered with a cluster-agnostic test registry.
- A2 (advisory): M3 shows the `!is_handshaking()` precondition is untested. The incomplete test runs before ServerHello, where
  `protocol_version()` alone already refuses. On client resumption, peer certificates are set at ServerHello, before Finished.
- Met: V1 (real nodes 4/5 enrolled under one root, refusal and pass); V3 (M8/M9 killed); V4 gate (300 ms, ≥ 20 ticks, completes on release);
  V5 order and API; V6; V7 (a)-(c) with an explicit `assume` control and the E7 term wait; V8 bindings and placement; V9 guard and bindings (2/2);
  root clarifications (`VerifyRefusal` incl. `Unavailable`, explicit `PeerRefusal` map, completed-connection-only adapters, test-only `assume`).
  Stale isolated acceptance and removal after local apply are asserted. Lock +2 is exact.
- `34902ca` is accepted as an ordinary new-fixture precondition correction. It uses the existing `private_temp`, adds an explicit privacy
  assertion and a standalone precondition test, and changes no production privacy check, shared helper, timeout or quarantine. The focused red log
  exists. `d1c207d` is a pure rename (`authenticated_peer` → `from_verified_certificates`), with identical body and callers, so it is behavior-neutral.

## Standards (AGENTS.md / CONTRIBUTING.md plus the smell baseline)

- Hard: "Every behavior needs a meaningful test" is breached for server-side chain/time (R1) and for resumption behavior (B1). Evidence honesty
  is good: the evidence doc labels coordinator runs separately, names unrun mutants and makes no transport, CA or hardware claim.
- Possible Refused Bequest / Primitive Obsession: one `PeerVerifier` with `expected: Option<NodeId>` serves both roles. A `client(n)` verifier
  installed in a server config silently ignores `n` (`verify_client_cert` passes `None`). Separate client and server types would make that
  unrepresentable.
- Possible Mysterious Name: `MAX_CSR` bounds certificates (`verify.rs:58,133`); `check_sender` reports `TargetMismatch` for a sender mismatch.
- Possible Duplicated Code: the signature, TLS 1.2 and scheme methods repeat across both trait impls, and `from_client`/`from_server` repeat.
  This is low priority, since the trait boundary forces most of it.

## Limits

No production transport, listener, CA, renewal or membership work exists or is credited. No Windows stability, quarantine recovery or historical
fix credit is given. Local timings are not hosted Windows `verify` timing and do not establish the 660 s rule. Probe results are in-memory only.
All hardware paths are **unverified on real hardware**. M2 remains open and M3 unchecked.

Findings: Security 1 blocking (B1 resumption bypass). Spec 2 required plus 2 advisory (worst: R1 server-side chain/time untested). Standards 1 hard
plus 3 judgement smells (worst: untested security behavior).
