# M3 PR B certificate verifier: readiness check

2026-11-16. Anthropic / Claude Opus 5.5 / high, research sub-agent, no children. Docs only. Base `c1cdecd` (pending PR157 A head; A gates still open).
Read: `AGENTS.md`, `docs/KICKOFF.md`, ADR 0005, the issuance brief (PR B section, decisions 1-7, E1-E7), and the actual A source: `cluster/pki.rs`, `pki/ca.rs`,
`pki/csr.rs`, `consensus/issue.rs`, `consensus.rs` (`sm_weak`, shutdown, `local_authorize`), `consensus/mem_net.rs`, `consensus/three_node.rs` (`Cluster::start`,
`founders`), `enrollment.rs` (`KeyDigest`), `enrollment/registry.rs` (`authorize_peer`, `PeerRefusal`), the quarantine guard and the sampler filter. rustls `0.23.46`,
rustls-webpki `0.103.16` and rustls-pki-types `1.15.1` were read from the local cargo registry cache (no fetch needed). No source, brief, PROGRESS or ADR edit. No build,
resolve, install, native, VM, admin, CI, push, PR, status, merge or watch.

**Verdict: READY once the coordinator applies V1-V9 below to the brief's PR B section. No redesign, no ADR deviation, no new scope.** F1 is a real security conflict
(the brief's "ignoring the server name" would let any enrolled member impersonate another to a client). F2-F6 are real API or seam conflicts that would fail to compile,
fail every handshake, or break shutdown. **B source starts only after:** PR157 (A) merges, exact-main CI on its merge SHA is green, and the root Sol contract check
accepts V1-V9. A's new Windows cost is still unmeasured; the 660 s rule applies to A's merge run and again to B.

## Verified against source

- **A types.** `pki::node_uri`/`parse_node_uri` (strict, ≤ 77 B, lowercase hex, no leading zero, id ≥ 1) and `MAX_CSR = 4096` exist. `KeyDigest::of_spki` hashes exact
  SPKI DER (1..=2048 B). Leaves: empty subject, one SAN URI, KU digitalSignature, EKU {serverAuth, clientAuth}, `IsCa::NoCa`, 16-byte positive serial, P-256 CA with
  `Constrained(0)`. A tests assert leaf SPKI raw == CSR SPKI raw, so `of_spki(leaf.subject_pki.raw)` equals the committed digest.
- **CA private boundary.** `ClusterCa` has no Debug/Clone; constructors are `#[cfg(test)]`; `cert_der()` is the only public byte accessor; `private_marker()` is
  `#[cfg(test)]` for I9 scans only. B needs nothing beyond `cert_der()`.
- **Registry.** `authorize_peer` is a pure `BTreeMap` lookup (≤ `MAX_MEMBERS`) on local applied state, no quorum. Reaching it means locking
  `SmAdapter(Arc<std::sync::Mutex<StateStore>>)`; that wait is as long as the lock holder (apply, snapshot, a test gate). Shutdown fails `StoreRelease` (→ `Orphaned`)
  while any strong `Arc` to the store survives (`consensus.rs:578`).
- **mem_net.** `MemNetwork::remote(sender)` compares the claimed leader ID with `Factory.source` and increments `MemRouter.mismatch`. `Factory { source, router }` is
  built by struct literal in `three_node.rs:87` and four sites in `consensus/tests.rs`; `MemRouter` derives `Default`.
- **Founders.** `three_node::founders()` registers `of_spki(&[250, n])` placeholder digests for nodes 1-3, so no real certificate can ever match a founder.
- **Engine test clock.** `issue_tests.rs` `NOW = 1_000_000` ms (1970). A leaf from an engine test is valid around 1970, not the wall clock.
- **rustls 0.23.46** (`src/verify.rs`, `webpki/*.rs`, `lib.rs`). `ServerCertVerifier` and `ClientCertVerifier` are `Debug + Send + Sync`; both require
  `verify_tls12_signature` (no default). Paths: `rustls::client::danger::ServerCertVerifier`, `rustls::server::danger::ClientCertVerifier`,
  `rustls::client::verify_server_cert_signed_by_trust_anchor(&ParsedCertificate, &RootCertStore, intermediates, now, algs)` (webpki `KeyUsage::server_auth()`, chain, time;
  no name, no revocation), `rustls::server::ParsedCertificate`, `rustls::crypto::verify_tls13_signature` (rejects schemes not valid in TLS 1.3, tries the first mapped
  algorithm), `WebPkiClientVerifier::builder_with_provider(roots, provider).build()` → `Arc<dyn ClientCertVerifier>` (default anonymous policy Deny = mandatory;
  `KeyUsage::client_auth()`; passes caller intermediates to webpki). `ClientConfig`/`ServerConfig::builder_with_details(provider, Arc<dyn TimeProvider>)`,
  `.with_protocol_versions(&[&version::TLS13])`, `ClientConfig.enable_sni` (pub), `peer_certificates()`, `CertificateError::Other(OtherError(pub Arc<..>))`.
  `WebPkiClientVerifier::builder` (no provider) calls `get_default_or_install_from_crate_features` (process-global). The ring provider maps TLS 1.3
  `ECDSA_NISTP256_SHA256` and `ED25519`. webpki does not check trust-anchor validity; A's `CaExpiring` keeps every leaf inside the CA window.
- **Features.** rustls `ring = [dep:ring, webpki/ring]`, `std = [webpki/std, pki-types/std, once_cell/std]`; `tls12`, `logging`, aws-lc stay off. Non-optional deps:
  pki-types `^1.12`, webpki `^0.103.14`, subtle, once_cell, zeroize. webpki 0.103.16 deps: pki-types `^1.12`, untrusted `0.9`, optional ring.
- **Lock (read only, not resolved).** Current lock: 281 packages (= 243 + 38). Already present: ring 0.17.14, untrusted 0.9.0, rustls-pki-types 1.15.1, subtle 2.6.1,
  once_cell 1.21.4, zeroize 1.9.1, log 0.4.35. Absent: rustls, rustls-webpki. So +2 (→ 283) is consistent; webpki needs `--precise 0.103.16` because 0.103.17 is newest.
- **Guard/sampler.** The guard binds `issue_tests` in `issue.rs` and `tests` in `pki.rs` by exact declaration text and scans only those two test files; the
  exact-8 check reads only `three_node.rs`. Sampler FILTER `test(/^cluster::consensus::three_node::/)`.

## Findings

- **F1 (security) Client side must bind the intended peer.** A client verifier that accepts any registered member lets enrolled node X answer a dial to node Y and,
  for example, return vote responses counted as Y. ADR 0005 requires the cluster/node/key binding on every connection. The server name is *replaced* by an
  expected `NodeId`, not dropped.
- **F2 (API) Handshake time.** Configs built without a time provider use the system clock and reject every engine-issued leaf as expired.
- **F3 (shutdown) Registry closure lifetime.** A closure capturing `SmAdapter` (strong `Arc`) makes `shutdown()` return `Orphaned`.
- **F4 (honesty) Blocking.** Verifier callbacks are synchronous; the registry step can block on the store mutex for as long as its holder. That is not bounded.
- **F5 (visibility) Test placement.** `Cluster`, `enroll` helpers and `mem_net` are private to `consensus`; `pki/tls_tests.rs` cannot reach them.
- **F6 (seam) Factory field.** Adding a field to `Factory` forces edits to `three_node.rs` (quarantined file) and `tests.rs`.
- **F7 (proof) Founder identities.** Placeholder founder digests mean a 3-node sender test cannot use real certificates for nodes 1-3.
- Precision only: `Debug` supertrait, required TLS 1.2 method, global provider, and size check order (V5, V6).

## Mandatory brief edits (replace/append in "PR B files and tests")

- **V1 (F1).** Replace "ignoring the server name" with: "`PeerVerifier::client(expected: NodeId, ..)` is the `ServerCertVerifier`; it ignores the `ServerName`
  value and instead requires the verified identity's node == `expected` (`TargetMismatch`). One client config per target. `enable_sni = false`; the
  `ServerName` passed to `ClientConnection::new` is a fixed constant with no node, cluster or owner data. `PeerVerifier::server(..)` is the `ClientCertVerifier` and
  accepts any registered member, reporting its identity after the handshake (V7). Test: two real enroll leaves (nodes 4 and 5); a client expecting 5 is refused by
  the node-4 server, and expecting 4 passes."
- **V2 (F2).** Append: "Every config uses `builder_with_details(ring provider, time)`. Engine-leaf handshakes use a test `TimeProvider` fixed at the signer reading
  (+1 s); the verifier uses only rustls's `now` argument, never `SystemTime` or `SignerClock`. Expiry: `not_before - 1 s` gives NotValidYet and `not_after + 1 s` gives
  Expired, through the time provider and through direct verifier calls; both exact bounds pass."
- **V3 (F3).** Append: "`ConsensusNode::peer_registry()` (in `issue.rs`) captures `sm_weak.clone()` only, upgrades per call, and drops the `Arc` before returning;
  a dead store or poisoned mutex is refused (`Unavailable`). Test: with a live `PeerVerifier`, `shutdown()` is `Ok(())` (not `Orphaned`), and the next verification refuses."
- **V4 (F4).** Append: "Bounded pure work per verifier call: size and intermediate checks before any parse, one x509 parse, one webpki chain verification, one SHA-256,
  one map lookup; one TLS 1.3 signature check per side. The store-lock wait is **not** bounded (it lasts as long as the holder, as in A0). Tests pump handshakes only
  inside `spawn_blocking` or a `std::thread`, never on a runtime worker. Gate test: with the verifying node's store mutex held by a `std::thread`, the handshake does
  not complete for 300 ms while a concurrent `interval(10 ms)` counts ≥ 20 ticks; on release it completes. No `try_lock`, timeout or freshness claim is added. Transport-slice
  callers must decide their own threading."
- **V5.** Replace the verifier order with: "(1) `intermediates.is_empty()` and `end_entity.len() <= 4096`, else refuse, before any parse; (2) chain: client
  side `verify_server_cert_signed_by_trust_anchor(&ParsedCertificate::try_from(ee)?, roots, &[], now, provider.signature_verification_algorithms.all)`, server side the inner
  `WebPkiClientVerifier` (built with `builder_with_provider`, mandatory; `client_auth_mandatory()` and `offer_client_auth()` return true); (3) `peer_identity(der)`:
  x509-parser `from_der` with empty remainder, exactly one SAN extension holding exactly one `GeneralName`, a URI, strict `parse_node_uri`, same cluster,
  `KeyDigest::of_spki(subject_pki.raw)`; (4) registry. `verify_tls13_signature` on both sides calls `rustls::crypto::verify_tls13_signature` with the ring provider's
  algorithms; `verify_tls12_signature` always refuses. `PeerVerifier::new` takes the root as `CertificateDer<'static>` from `ClusterCa::cert_der()`, never `&ClusterCa`."
- **V6.** Append: "`PeerVerifier` implements `Debug` by hand as the fixed text `PeerVerifier`, with no closure, cluster, root or certificate bytes. Refusals are a
  unit-variant enum with static Display/Debug, carried as `CertificateError::Other(OtherError(Arc::new(refusal)))`; tests compare variants through direct verifier
  calls and check the alert/`Error::InvalidCertificate` through handshakes. Never call `CryptoProvider::install_default`, `WebPkiClientVerifier::builder` or
  `ClientConfig::builder()`/`ServerConfig::builder()`. After every successful handshake, assert `protocol_version() == Some(TLS13)`."
- **V7 (F6, F7).** Replace the sender paragraph with: "`check_sender(authenticated: NodeId, claimed: u64)` lives in `pki/verify.rs`. `AuthenticatedPeer` has a private
  constructor used only by `authenticated_peer(peer_certificates())` after a completed handshake (re-running `peer_identity` on the verified certificate) and a
  `#[cfg(test)]` `assume(NodeId)` for placeholder founders. `MemRouter` (Default) gains `authenticated: Mutex<BTreeMap<u64, AuthenticatedPeer>>` keyed by source;
  `remote` compares the claimed ID with that identity, else `source`. No `Factory` field, and no edit to `three_node.rs` or `consensus/tests.rs`. Tests: (a) the
  identity from a real node-4 handshake is node 4; (b) mismatch proof: map source = leader L to that genuine node-4 identity; L's RPCs are refused even though
  `source == claimed`, `mismatch` increases and a different leader is elected; (c) map L to `assume(L)` and the cluster converges normally."
- **V8 (F5).** Replace the tests file line with: "Pure verifier and handshake tests (test CA from `ClusterCa::ephemeral` and `test_ticket` leaves, joiner key held by
  the test) go in `pki/tls_tests.rs`, bound `#[cfg(test)]\nmod tls_tests;` in `pki.rs`. Engine-dependent tests (real 3-node `enroll` leaf, removed node after local
  apply, stale isolated verifier, V3, V4 gate, V7) go in `consensus/tls_engine_tests.rs`, bound `#[cfg(test)]\n#[path = "tls_engine_tests.rs"]\nmod tls_engine_tests;`
  in `issue.rs`. The joiner generates its own rcgen `KeyPair` and CSR; its PKCS#8 bytes go to rustls only inside the test and never reach `enroll`, stores, logs or errors."
- **V9 (guard/CI).** Append: "Both new files are ordinary required tests on all three OS, outside `^cluster::consensus::three_node::`, so the sampler's exact 8 and
  the 8 `ignore` attributes are unchanged. The guard adds both files to the scanned list with the same mutation set, and asserts each binding by exact text exactly
  once, with remove-binding mutations that must fail. No new quarantine, `ignore`, platform `cfg`, filter, retry or timeout. A red follows the pre-registered red handling.
  Cargo evidence diffs name/version/source/checksum lists: exactly +2 (rustls 0.23.46, rustls-webpki 0.103.16), zero changed or removed."

## Unchanged and out of scope

The pins and `+2` allowance stand. In-memory `ClientConnection`/`ServerConnection` pumping (≤ 32 rounds) with no sockets, listeners or production transport.
`authorize_peer` stays local with unbounded staleness (no membership freshness claim); the stale isolated verifier test asserts it. Deferred: transport ingress,
sealed CA, renewal, voter membership, real hosts. All are **unverified on real hardware**.

## Next action

Coordinator: apply V1-V9 to the brief's PR B section (docs-only) and route the edited brief to the root Sol contract check. Wait for PR157's gates, merge and an
exact-main green with measured Windows `verify` time under 660 s. Then Sol builds B and Opus reviews it against the edited brief.
