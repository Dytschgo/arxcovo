# Brief: M3 certificate issuance, first slice

Status: **proposed, corrected** (Opus 5.5 high, 2026-11-16, main `6e9edd5`). It closes C1-C3 of the [Sol review](../reviews/m3-certificate-issuance-slice-sol-review.md),
and Sol rechecks it before any source work. Nothing here is built or run. There is no M3 acceptance, transport, listener, sealed CA, owner keystore, voter
membership workflow or real-host claim, and full M3 stays **unverified on real hardware**. M2 stays incomplete: A7 is FINAL NO-GO, and the consumed A12 attempt
with unconfirmed cleanup gets no retry or native rerun. The settled handoff in [the registry brief](m3-enrollment-certificate-next-slice.md#ca-handoff-next-slice-settled-here)
and ADR 0005 (amended 2026-11-15) bind this slice. **ADR 0005 is not amended**, and no new architectural deviation is introduced. Two corrections stay inside the
handoff's scope: the SPKI source (decision 3) and the test-slice certificate lifetime (decision 5).

## Size decision: three PRs, in this order

Full issuance plus the TLS verifier is about 1.5k lines with tests, and Windows CI has almost no headroom (see CI below). One PR is too large to review well.
- **PR A0, bounded fresh read** (no dependencies). Fixes advisory 2 (the existing 1500 ms limit bounds only the quorum wait) before any caller other than tests
  exists. It is one concern, ships fast, and is not held up by dependency or CI-time risk.
- **PR A, issuance: the first usable slice.** CSR proof of possession, the ephemeral in-memory CA, leaf signing, and the leader-only path that issues from the
  committed response with fresh guards, tested on three real OpenRaft nodes. It adds rcgen, x509-parser and time, and **no rustls**. Leaf checks use x509-parser
  `verify_signature`.
- **PR B, verifier (the follow-up that comes right after A).** It adds rustls, the cluster-root-only client and server verifiers bound to the registry, in-memory TLS
  1.3 mutual-auth handshakes and the sender check. B is specified here so A builds the right seams. It starts only after A merges.

## Verified facts (primary sources, 2026-11-16, read only; nothing pinned in the repo yet)

- crates.io API and static.crates.io tarballs, sha256 recomputed locally and equal to the registry checksum. rcgen `0.14.10` (2026-08-28, MSRV 1.88, `8774e05a…8ad8`) is the
  max stable. rustls `0.23.46` (2026-10-14, `48e13bd8…8fdf`) is the max stable, and `0.24.0-dev.*` is excluded. x509-parser `0.18.1` (`d43b0f71…5202`). rustls-webpki
  `0.103.17` was released 2026-11-12 (`e3a0760a…53fd5`), and its source diff against `0.103.16` (`4443b1b6…b281`, 2026-10-20) adds only SCT parsing and
  `spki_for_anchor`, so it is not a security fix. rustls-pki-types `1.16.0` was released 2026-11-10, and `1.15.1` (`2f492502…0a96`) dates from 2026-07-23. ring is `0.17.14`.
- **rcgen `src/csr.rs` `from_der`.** It parses, then runs x509-parser `verify_signature()`, but it **discards trailing bytes** (`.1`). `PublicKey.alg` comes from the
  CSR **signature** algorithm OID, and `raw` is only the BIT STRING. `subject_public_key_info()` re-encodes the SPKI with that signature algorithm's key OIDs.
  x509-parser `verify.rs` picks the curve from the SPKI parameters, so `ecdsa-with-SHA256` over a **P-384** key verifies (`ECDSA_P384_SHA256_ASN1`), and rcgen then
  re-encodes it as a prime256v1 SPKI. The settled formula `of_spki(public_key.subject_public_key_info())` can therefore hash an SPKI that is not the requested one.
  It accepts RSA and SHA-1 signature algorithms. It copies the subject, SAN, KU, EKU and BasicConstraints, and an unknown extension gives `UnsupportedExtension`. rcgen
  does not re-export `time`. `CertificateParams::signed_by(&impl PublicKeyData, &Issuer)` and `SerialNumber::from_slice` exist. The rcgen `ring` feature enables
  `x509-parser?/verify`. x509-parser's `default` feature set is empty.
- **OpenRaft 0.10.0** (local registry source). `ensure_linearizable` calls `await_ready` with **no timeout**, and `wait_timeout` bounds only the wait for a quorum
  acknowledgement. That wait starts once RaftCore dequeues the request, so time spent in the queue is unbounded. `get_read_linearizer` and
  `Linearizer::try_await_ready(&raft, Some(d))` return `Ok(Err(state))` on timeout. `ReadLogId::committed_leader_id()` is the **current leader's** (term, node),
  because the read log ID is the max of the leader's noop and the committed ID. `ClientWriteResponse` has pub fields (`log_id`, `data`, `membership`).
  **Read queue** (`raft_inner.rs`, `raft_core.rs`, `pending_read_queue.rs`): `get_read_linearizer` awaits `tx_api.send(GetLinearizer)` on a **bounded** mpsc
  (`api_channel_size` 1024 here, shared with writes), then a oneshot. A drop before the send enqueues nothing, and once enqueued **nothing withdraws it**. At dequeue
  it answers or becomes a `PendingRead` (`deadline = dequeue + wait_timeout`), which drain_satisfied, drain_expired or drain_all_with_error resolves with
  `tx.send(..).ok()`. A stop gives `Fatal`. `try_await_ready` is a droppable watch wait. Tokio cannot abort a running `spawn_blocking` closure.
- **rustls 0.23.46.** The default features are `aws_lc_rs`, `logging`, `prefer-post-quantum`, `std` and `tls12`, and `ring`/`std` are separate.
  `WebPkiClientVerifier::builder_with_provider`, `rustls::client::verify_server_cert_signed_by_trust_anchor(&ParsedCertificate, roots, intermediates, now, algs)`
  (which checks no name) and `verify_tls13_signature` are public.
- **Lock resolution** (a throwaway D: copy, `cargo metadata`/`cargo update --precise` only, no build). From lock `7941da2f…a9f6` (243 packages), A adds **38 packages**,
  B adds **2 more** (rustls, rustls-webpki), and **no existing package version changes**. None is yanked. With the precise pins below, every added version is at least
  27 days old. The added packages: asn1-rs(+derive,+impl), bit-vec, data-encoding, der-parser, deranged, getrandom 0.2.17, lazy_static, minimal-lexical, nom 7, num-bigint,
  num-conv, num-integer, oid-registry, powerfmt, rcgen, ring, rusticata-macros, rustls-pki-types, synstructure, time(+core,+macros), untrusted, windows-sys 0.52 +
  8 windows-targets 0.52 crates, x509-parser, yasna, zeroize.
- **CI.** The Windows `verify` job took 585 s and 716 s on the last two green main runs (43768671593 and 43775693200), against `timeout-minutes: 12` (720 s). There is
  no cargo cache. Clippy takes 116 s and nextest 307 s. Main `6e9edd5` CI 43777637580 was still in progress when this brief was written (Ubuntu and macOS green).

## Decisions

1. **A0: one caller deadline over owned, admission-bounded read units.** Add `READ_BUDGET = 1500 ms`, `READ_PERMITS = 8`, `ReadError::Busy`, `ConsensusNode.id`
   (set in `start`), a separate `read_permits: Arc<Semaphore>` (proposal permits untouched) and `fresh_read(f) -> Result<(LeaderId, T), ReadError>`, with a pure
   `f: FnOnce(&StateStore) -> Result<T, StoreError> + Send + 'static` (no await, no Raft call). `linearizable_state` wraps it with an unchanged signature.
   - **Caller.** Take `deadline = now + READ_BUDGET` first. Then `try_acquire_owned`, else `Busy`. Under the `owners` mutex (check `closed`, else `Unavailable`; reap;
     spawn; no await), spawn the owned task into `owners.set` with the permit, as `propose` does. Then `timeout_at(deadline, rx)`, where elapsed or dropped is
     `Unavailable`, and drop `rx`, so a late result is never observable. That one deadline covers queue, quorum, apply and store-lock wait.
   - **Owned task (keeps the permit until its work actually ends).** (1) If the deadline has passed or `tx.is_closed()`, stop before submitting. (2) Await
     `get_read_linearizer(LinearizerOption::new(Some(ZERO), true).with_wait_timeout(remaining))` **to completion, with no timeout**, because it is the only signal
     that an enqueued request has ended. ForwardToLeader→`NotLeader`, QuorumNotEnough→`NoQuorum`, other/Fatal→`Unavailable`. (3) Re-check, then `try_await_ready(raft,
     Some(remaining))` (`Ok(Err)`/Fatal→`Unavailable`). (4) Re-check, then `spawn_blocking` a closure **owning the permit and the `SmAdapter` clone**: lock, check
     `applied >= read index` (else `Corrupt`), run `f`, and drop the guard, Arc and permit in that order (#147). Await it, then `tx.send(..).ok()`.
   - **Honest limits.** The deadline bounds **caller waiting only**, not queue residence or the blocking closure. OpenRaft 0.10.0 cannot withdraw an enqueued read: it
     lives until dequeue (not time-bounded here; it depends on RaftCore progress), then at most `remaining` as a `PendingRead`, or until leadership loss or stop. The engine timer starts at dequeue, so caller deadline `Unavailable` may precede engine `NoQuorum`; no shorter sub-budget or timeout relabelling is used. A
     store read lasts as long as the lock holder. **Bounded:** at most `READ_PERMITS` units per node exist anywhere (queue, pending set, apply wait, blocking pool),
     because a permit returns only after the engine reply or the closure finishes. Excess callers get `Busy` before submitting. Nothing detaches silently: an
     abort happens only after `closed`, and if RaftCore did not stop, shutdown reports `Orphaned`.
   - **No deadlock or shutdown overclaim.** All acquisitions (issuing, then proposal, then read) use `try_acquire`. No mutex is held across an await, and apply and
     reads share only the store mutex, on blocking threads. A wedged RaftCore yields `Busy`, not leaks. Shutdown keeps `closed`, `try_shutdown` (engine units resolve
     `Fatal`), the 5 s drain, `abort_all` and the weak check. A unit still in its closure gives `Err(Orphaned)` in time: responsiveness, not hard termination.
2. **Commit evidence: a reviewed adapter, with no claim of forgery-proofing.** `propose` delegates to a private `propose_response(cmd) -> Result<ClientWriteResponse<Arx>,
   ProposeError>`. The module `consensus/issue.rs` alone holds `fn committed(&self, resp: ClientWriteResponse<Arx>) -> Result<Commit, IssueError>`. It
   requires `resp.log_id.leader_id.node_id == self.id` (`WrongNode`) and `Outcome::Enrolled(grant)`, and records `(term, node, index)`. Each guard requires the
   fresh read's `committed_leader_id() == commit.leader` (`StaleIssuer`), so a re-election between commit and guard, even of this same node, refuses. The client
   retries the same request, gets a `replay:true` grant in the new term and is issued. **Honest limit:** `ClientWriteResponse` fields are public, and anyone can
   obtain an `EnrollmentGrant` from a local `EnrollmentState::apply`. The compiler enforces only that `IssueTicket` (private fields, constructed solely in `issue.rs`)
   cannot be made elsewhere, and `ClusterCa::sign_leaf` requires `&IssueTicket`. `issue.rs` itself is trusted, reviewed code. It obtains responses only from its own
   `propose_response` call, and no function accepts a caller-supplied grant or response for signing.
3. **CSR proof of possession (`cluster/pki/csr.rs`, `ProvenCsr::parse(&[u8])`).** (a) `1..=MAX_CSR` (4096 B), else `Oversized`. (b) x509-parser
   `X509CertificationRequest::from_der` with an **empty remainder**, else `Malformed`. (c) Exact algorithm pairs **before verifying**: an Ed25519 SPKI (OID 1.3.101.112,
   no parameters) with signature algorithm Ed25519 (no parameters), or an id-ecPublicKey SPKI with namedCurve prime256v1 and signature algorithm ecdsa-with-SHA256 (no
   parameters). Anything else (RSA, P-384, SHA-1, mismatched pairs) is `UnsupportedKey`. (d) rcgen `CertificateSigningRequestParams::from_der` gives proof of
   possession. Map `InvalidCertificationRequestSignature` to `BadSignature`, `UnsupportedSignatureAlgorithm` to `UnsupportedKey`, and every other error, including
   `UnsupportedExtension` and bad DN strings, to `Malformed`. (e) Defense in depth: require `public_key.subject_public_key_info() == subject_pki.raw` byte for byte,
   else `UnsupportedKey`. (f) Keep **only** `public_key` and `digest = KeyDigest::of_spki(subject_pki.raw)`, and drop `.params` at once. Never call the CSR's
   `signed_by`. Subject, SANs, KU, EKU, BasicConstraints and attributes are discarded. `CsrError` Display text is static. Ingress (the future transport slice) and the
   signer use this one function, so the digest proposed with Redeem equals the one the signer re-derives. This is the SPKI-source correction to the settled handoff,
   and for every accepted CSR the value is identical.
4. **Ephemeral CA (`cluster/pki/ca.rs`).** `ClusterCa { issuer: rcgen::Issuer<'static, KeyPair>, cert_der, not_after_s }` has no Debug, Clone, serialize, load or
   save. **`ClusterCa::ephemeral(cluster, now_ms)` is `#[cfg(test)]`**, so production builds contain no CA constructor and no real private key can enter. It uses a P-256
   key, `IsCa::Ca(Constrained(0))`, KU keyCertSign+cRLSign, CN `arxcovo test cluster CA` (no owner data) and test validity `ca_nb = floor_s(now) - 120 s ..= ca_nb
   + 31 d`. It contains every decision 5 leaf whose signer time is in `[ca_now - 60 s, ca_now + 1 d - 60 s]` (all engine tests; `CODE_TTL_MS` is 15 min). The
   `#[cfg(test)] ephemeral_with_validity` gives the near-expiry control. This is test arithmetic, **not production CA policy.** Only the leader (a voter) signs.
   Key bytes never reach entries, forwards, state, snapshots, logs, evidence or MCP.
5. **Leaf parameters, built only by the signer.** `IsCa::NoCa` (explicit CA:false is not required), KU digitalSignature, EKU exactly {serverAuth, clientAuth}, an empty
   subject, and exactly one SAN URI `arxcovo://cluster/<32 lowercase hex>/node/<decimal id>` from the ticket. The serial is 16 `getrandom` bytes with the top bit
   cleared and bit 6 set. Validity: `not_before = floor_s(signer_now) - 60 s` (2×`SKEW_MS`), `not_after = not_before + 30 days`. If `not_after` exceeds the CA's, the
   result is `CaExpiring`, with nothing signed. The DER must be ≤ 4096 B. `pki::node_uri`/`parse_node_uri` are strict: lowercase hex only, no leading zeros,
   id ≥ 1, no trailing bytes. **Stale-acceptance bound.** A returned leaf was signed at true time `T_s` (signer reading `s`), and guard 2's later ReadIndex still saw
   the member, so any Remove commits after `T_s`. A verifier accepts while its clock reads `<= not_after <= s + 30 d - 60 s`. **Under the unenforced assumption** that
   each signer and verifier clock remains within `SKEW_MS` (30 s) of true time throughout the interval (the existing contract), an isolated stale verifier stops
   by `T_s + 30 d`: signer error contributes at most30 s, verifier error at most30 s, offset by the60 s backdate (flooring only shortens it). Conservatively:
   **≤ 30 days + 2×SKEW_MS after signing, and so after removal.** Pairwise clock agreement alone proves no real-time bound; a clock outside the assumption accepts without bound. This is a test-slice ceiling, **not** production revocation freshness;
   renewal and rotation are not built, an expired member fails closed, and production lifetime is decided before production wiring.
6. **Issue path (`ConsensusNode::enroll(&self, ca, clock: &dyn SignerClock, input: RedeemInput, csr: &[u8]) -> Result<IssuedLeaf, IssueError>`).** The whole call
   runs under `ISSUE_BUDGET = 5 s`. A `try_acquire` on `MAX_ISSUING = 4` permits returns `Busy` when none is free. Steps:
   1-3. Parse the CSR (decision 3) **before** proposing. Propose `Command::redeem(.., &proof, csr.digest(), clock.now_ms())` (proposal data, never a guard time),
      drop the proof, then call `committed`.
   4. Guard 1: `fresh_read`, whose returned leader must equal `commit.leader` and whose `f` runs `check_grant(&grant, &code)` (a present record consumed by exactly
      (request, key, node)) and `authorize_peer(grant)`, plus `grant.key() == csr.digest()`. **Only after it returns** (queue, quorum, apply and store waits done),
      sample `pre_now` and run `check_issuable(pre_now)`. Then (5) sign with that `pre_now`, with no await in between.
   6. Guard 2: the same `fresh_read` with a new ReadIndex. Only then sample `post_now`, run `check_issuable(post_now)` and require `post_now >= pre_now`
      (`ClockRollback`). No clock value sampled before a wait is used after it. (7) Only then return `IssuedLeaf { der, node }`. A failed guard drops the DER.
   `IssueError` = `Busy | NotLeader(Option<u64>) | Unavailable | BadCsr(CsrError) | Rejected(Reject) | WrongNode | StaleIssuer | Revoked | Signer(IssueRefusal) |
   ClockRollback | CaExpiring | Sign`, and `ReadError::Busy` becomes `Busy` while `ReadError::NoQuorum` maps to `Unavailable`. Display text is static. `joiner_reply()` maps `NotLeader`, `Unavailable`, `Busy` and
   `StaleIssuer` to `RetryLater` in the existing `JoinerReply`. Nothing is cached between guards, and no endpoint or owner approval exists. A follower gets
   `NotLeader` with nothing appended. An `ISSUE_BUDGET` timeout leaves the outcome unknown, so the caller retries the same idempotent request. Abandoned read units
   stay owned and permit-bounded.
7. **Staleness stays as ADR 0005 states.** `authorize_peer` is local and its staleness is unbounded. It is insufficient for control or execution traffic. B uses it
   only for Raft peer TLS, so peers never need a quorum to form one. Fresh quorum checks exist only inside issuance.

## PR A0 files and tests

`consensus.rs` (`fresh_read`, `id`, `read_permits`, `Busy`), `consensus/adapter.rs` (main PR150's `#[cfg(test)]` `APPLY_PAUSE`, keyed by the `SmAdapter` Arc pointer,
inside the blocking worker **before** acquiring the store mutex) and `three_node.rs`. No new dependencies. Actual OpenRaft, `mem_net`, `timeout(45 s)`. Every fixture using the shared pause slot (R1, R6 and any future issuance guard test) holds the test-only async APPLY_PAUSE_OWNER mutex for its complete hook lifetime. The **store gate** is a
`std::thread` holding the leader's `SmAdapter` mutex until an mpsc release (never a Tokio worker). R1 and R3 hang past the old 4 s test timeout before the fix:
- **R1 apply wait.** Pause the leader's apply, then commit a write on the followers. The leader's `linearizable_state()` gives `Unavailable` in ≤ 2.5 s. Release,
  and the write and a read succeed and `converge` holds. **R2:** an isolated leader gives `NotLeader`/`NoQuorum`, or `Unavailable` only at or after READ_BUDGET. Both caller return and all read permits returning must occur within2.5s measured from the original call start.
- **R3 store wait.** With the gate held, `Unavailable` in ≤ 2.5 s, while a concurrent `interval(10 ms)` counts ≥ 50 ticks. Still gated, `available_permits() ==
  READ_PERMITS - 1` because the closure owns its unit. Release: the permit returns in 1 s, the late result is never delivered, and the next read is current.
- **R4 admission and cancel.** Gated: start `READ_PERMITS` reads and abort one caller early. The next call is `Busy` in ≤ 50 ms before any spawn, and the 64 proposal
  permits are unchanged. No aborted caller frees its permit early. Release, and all units return in 2 s with `owners.set` reaped.
- **R5 shutdown.** (a) Units in flight on an isolated leader: `shutdown()` is `Ok(())` in ≤ 5.5 s. (b) Gated with a unit in its closure: `Err(Orphaned)` in ≤ 5.5 s,
  not a hang. After release, the store Arcs drop in 2 s. All existing tests pass unchanged except T2's isolated-read pattern also admits deadline `Unavailable`; its non-success requirement and every other assertion stay unchanged.

## PR A files and tests (Sol high; Cargo adds only the pins below)

- **Cargo.toml (arxd):** `rcgen = { version = "=0.14.10", default-features = false, features = ["crypto", "ring", "x509-parser"] }`, `x509-parser = { version = "=0.18.1",
  default-features = false }` and `time = { version = "=0.3.55", default-features = false }`. The latter two are already in the lock through rcgen.
- **Cargo.lock:** run `cargo update -p rustls-pki-types --precise 1.15.1`. Allowance: exactly the 38 listed additions, with zero changed or removed entries. The
  evidence diffs the old and new lock name/version lists.
- **New files:** `cluster/pki.rs` (`#![allow(dead_code)]`, mods, `MAX_CSR`, URI helpers), `pki/csr.rs`, `pki/ca.rs`, `pki/tests.rs`, `consensus/issue.rs`,
  `consensus/issue_tests.rs`, `pki/fixtures/{p384_sha256,rsa2048_sha256}.csr.der`, and `docs/evidence/m3-certificate-issuance.md`.
- **Changed files:** `enrollment.rs` (`check_grant`, test); `consensus.rs` (`pub(super) mod issue;` and private `propose_response`, with `propose` delegating unchanged).
  `enroll`, test-only `ISSUE_PAUSE`, private `committed` and `IssueTicket` live in `issue.rs` as an `impl ConsensusNode`. Ticket type is visible only within
  `cluster` for `pki/ca.rs` to name it; fields and constructor remain private to `issue.rs`, with read-only accessors as needed. Parent code cannot call `committed`.
  `issue_tests.rs` is a child of `issue`. `three_node.rs`: expose only `APPLY_PAUSE_OWNER`, `code`, `mint`, `founders` as `pub(super)`; Cluster/D1 helpers already are.
  Add an empty-by-default extra-marker list scanned by `finish` as raw bytes/hex alongside all existing markers and its positive control. Existing helper behavior,
  diagnostics, deadlines and Params are unchanged. Add `tools/ci/test_windows_cluster_quarantine.py` to the allowlist only for the E3 no-bypass assertions below.
  Fixture CSRs carry only public data: generate once with openssl, discard keys, record command and sha256. No private key is committed.

**Pure tests (`pki/tests.rs`, `enrollment/tests.rs`)**, each of which fails without its check:
- P-256 and Ed25519 CSRs give `digest == of_spki(keypair.subject_public_key_info())`.
- A flipped signature byte and a flipped SPKI byte each give `BadSignature`.
- The P-384/SHA-256 fixture gives `UnsupportedKey` at (c). Mutant: with (c) removed, (e) still rejects it, and with both removed the test fails. RSA, P-384/SHA-384
  and ecdsa-with-SHA384 on P-256 each give `UnsupportedKey`.
- Size and shape: empty input, and 4097 B before parsing, give `Oversized`. Every truncated prefix gives `Malformed` with no panic, as do one trailing byte and a
  custom-OID extension.
- **Untrusted CSR fields are discarded.** A CSR asking for CA:unconstrained, keyCertSign, EKU any/codeSigning, a DNS SAN, a foreign-cluster URI and a CN is accepted.
  The issued leaf has none of these: no CA, KU digitalSignature only, EKU exactly {serverAuth, clientAuth}, one SAN for the ticket's node, an empty subject.
- **Leaf.** It verifies under the CA SPKI and fails under a second CA. Its SPKI raw equals the CSR's. Validity is exact to the second. Two issues give distinct
  positive 16-byte serials. The DER is ≤ 4096 B.
- **CA arithmetic (C1).** Containment needs `validity >= 30 d + 60 s + (floor_s(signer) - floor_s(ca_now))`. `ephemeral` (31 d) issues at whole-second `ca_now` and
  `ca_now + 1 d - 60 s` (both fail under a 1-day CA). Near-expiry control: `30 d + 59 s` gives `CaExpiring` (sign counter 0), and `30 d + 60 s` issues.
- URI parse rejects uppercase, leading zeros, 0, trailing `/`, overlong input and other schemes.
- `check_grant` matrix: pruned record, different request/key/node, removed, wrong cluster, and current.
- Error Display/Debug output contains no input marker.

**Engine tests (`issue_tests.rs`, actual OpenRaft, 3 nodes, each ≤ 45 s, total added wall time ≤ 25 s):**
- **I1 happy path and retry.** Mint, then enroll on the leader, issues node 4, and after `converge`, `local_authorize(4, digest)` passes on all three. An identical
  retry gives the same node and SPKI with a new serial.
- **I2 follower.** It returns `NotLeader(Some(l))` and its log has no new entry. The leader then succeeds.
- **I3 race.** Two concurrent enrolls with one code, different keys and requests give exactly one leaf. The other returns `Rejected(Consumed)`, and exactly one
  member is added.
- **I4 leader change.** Pause AfterCommit, isolate the leader, wait for a new leader and resume. The old leader returns `Unavailable`/`StaleIssuer` within its
  budget, with no leaf. Retrying the same request and CSR on the new leader issues the same node ID (`replay`). Heal and converge.
- **I5 Remove between guards.** Pause AfterSign, `write(Remove(4))`, resume. The result is `Revoked` with no leaf, and a later retry gives `Rejected(Revoked)`.
- **I6 signer time.** `FakeClock` produces `GrantExpired` before signing (a test-only sign counter stays 0). Advancing past expiry during AfterSign gives guard 2
  `GrantExpired`. Rolling back gives `ClockRollback`. A future-dated committed command makes `applied_ms` large and gives `SignerClockBehind`. **No cached time:**
  advance the clock past expiry while guard 1's `fresh_read` waits on `APPLY_PAUSE`, and the result is `GrantExpired` with the sign counter at 0.
- **I7 adapter units.** A crafted response from another node gives `WrongNode`, a non-Enrolled response gives `Rejected`, a term or node mismatch gives
  `StaleIssuer`, and a ticket key ≠ CSR digest is refused.
- **I8 Remove after issue.** Issue node 4, then mark follower F isolated (it stays running). Commit `Remove(4)`. The leader and the other follower refuse
  `local_authorize`, but F **still accepts**, which asserts the documented unbounded staleness. Heal F, and it refuses. The leaf stays chain-valid throughout, so
  the certificate is never the authority.
- **I9 privacy.** No store file contains CA key, code secret or proof markers. A positive control is retained.
- The existing T1-T4, U6 and crash probes pass unmodified. Store-file advisory 1 (Weak expiry ≠ file closed) stays **open**. A and B add no production restart
  caller, and it must be fixed before the first one.

## PR B files and tests (follow-up; specified now, built after A merges)

- **Cargo:** `rustls = { version = "=0.23.46", default-features = false, features = ["ring", "std"] }` and `cargo update -p rustls-webpki --precise 0.103.16`.
  Allowance: exactly +2 (rustls, rustls-webpki).
- **`pki/verify.rs`:** `PeerVerifier` holds `RootCertStore` with only the cluster CA (no system roots), the cluster ID, a ring provider and a `registry:
  Arc<dyn Fn(ClusterId, NodeId, KeyDigest) -> Result<(), PeerRefusal> + Send + Sync>` that reads local applied state. It implements `ClientCertVerifier` (the inner
  `WebPkiClientVerifier`, mandatory) and `ServerCertVerifier` (`verify_server_cert_signed_by_trust_anchor`, ignoring the server name). It verifies TLS 1.3
  signatures only. Intermediates must be empty. After the chain check, `peer_identity(der)` requires ≤ 4096 B, an empty remainder, exactly one SAN extension with
  exactly one URI, strict `parse_node_uri`, the same cluster, and `KeyDigest::of_spki(spki.raw)`. Then `registry`. Errors are static rustls errors. Configs use
  `builder_with_provider(..).with_protocol_versions(&[&TLS13])`.
- **Sender check:** `check_sender(authenticated: NodeId, claimed: u64)`. The `mem_net` `Factory` gains an optional authenticated identity from the handshake, which
  replaces `source` in the sender comparison.
- **Tests (`pki/tls_tests.rs`):** in-memory `ClientConnection`/`ServerConnection` byte pumping, at most 32 rounds, with no sockets or listeners.
  - Mutual handshake passes on TLS 1.3 using a leaf from A's actual 3-node `enroll`.
  - Refused: a foreign CA with an identical DN, SAN and key; an unknown self-signed leaf; the wrong SPKI (a real-CA leaf for node 4 over another key gives
    `KeyMismatch`); no SAN, two URIs, DNS only, a foreign cluster, a malformed URI; an intermediate present; an oversized certificate; no client certificate;
    expired and not-yet-valid (the verifier `now` argument); a removed node after local apply.
  - A stale isolated verifier still accepts, as documented.
  - A sender mismatch is refused and the counter increments, while a matching sender elects normally.

## CI, routing and limits

A and B each record the measured Windows `verify` time and the clippy and nextest step times. If a hosted Windows job exceeds 660 s or times out, the coordinator
lands a separate `ci-speed` PR first, naming a measured component and reduction. The old Playwright 199 s caching example is stale after C1. **No timeout-minutes
increase, no ci-time-budget or agent-review bypass**, and no test is skipped. Sol builds A0, A and B. Opus reviews each one against this brief. Every merge needs all
five gates plus a green CI run on the exact current main. Deferred, explicitly: transport ingress (`no_client_auth` enrollment config), sealed CA transfer to new
voters, voter membership changes, crash and power-loss probes, renewal, and real-host three-process acceptance. All of these are **unverified on real hardware**.

## Issuance readiness corrections (E1–E6, 2026-11-16)

This section supersedes conflicting historical handoff wording above. Source work requires PR149 merged and exact-main CI green, plus the Sol contract check.
E1 module placement and narrow ticket visibility are reflected in the file list. The compiler protects ticket construction, not authenticity of a public write response.

E2: Append to the ISSUE_PAUSE text: "The test-only `ISSUE_PAUSE` is keyed by the node's store pointer (`Arc::as_ptr(&sm.0) as usize`, as `APPLY_PAUSE` is), not by node ID. It releases through a `tokio::sync::oneshot` and never blocks a runtime worker. It compiles out of non-test builds. `APPLY_PAUSE_OWNER` becomes `pub(super)`, and every test that arms `APPLY_PAUSE` or `ISSUE_PAUSE` holds it for the hook's whole lifetime. This is a test-correctness lock for shared static slots, not a scheduling change. Unit test: a pause armed for one node instance does not fire for a second instance with the same ID."

E3: Add to engine tests: "I1–I9 are ordinary required tests on all three OS. No `ignore`, platform `cfg`, filter, test group, retry or timeout change is added, and none of them is covered by the existing quarantine of 8. Their module path stays outside `^cluster::consensus::three_node::`, so the sampler's exact-8 inventory is unchanged. The quarantine guard gains one assertion: `issue_tests.rs` and `pki/tests.rs` contain no `ignore` or platform-dependent `cfg`/`cfg_attr` (including windows/unix shorthand, target_os and target_family). A mutation that inserts an `ignore` must fail the guard. A red follows the resumption brief's pre-registered red handling: no rerun, no quarantine, no deadline or permit change. A green gives no Windows stability credit."

E4: Append to decision 6: "Order: absolute `deadline = now + ISSUE_BUDGET`, then `try_acquire` on the issue permit, then parse. The permit is held by the caller's future, and `enroll` spawns no task. Only its proposal unit and read units outlive a cancellation, under their existing owners and permits. `ShutdownReport` and its cap of 72 are unchanged. An elapsed budget gives `Unavailable`. `ProposeError::Busy` gives `Busy`, `NotLeader(l)` gives `NotLeader(l)`, and `Stopped`/`Unknown` give `Unavailable`. CSR parsing and signing run synchronously inline: at most 4096 B, one verify, one P-256 sign and one `getrandom` call. The budget is responsiveness at await points, not preemption. I4 also asserts that the issue permits are all free as soon as the old leader returns, and that its read units return within 2 s."

E5: Append to decision 6: "`RedeemInput { cluster, request, code: CodeId, proof: RedeemProof }` derives no `Clone` and is consumed by value. The proof is dropped right after `Command::redeem`, before the first await. It is never captured by guard closures, pause hooks, errors or `IssuedLeaf`. Dropping is not erasure: this slice adds no zeroize. `trait SignerClock: Send + Sync`. A compile-time test asserts that the `enroll` future is `Send`. `check_grant` returns `Result<(), Reject>` using existing variants only (`UnknownCode`, `Consumed`, `KeyConflict`, `WrongCluster`). `authorize_peer`'s `NotMember`/`KeyMismatch` gives `Revoked`, and its `WrongCluster` gives `Rejected(WrongCluster)`."

E6: In I6, arm APPLY_PAUSE on the leader's store and write W. Release AfterCommit only after entered fires (W is committed). Advance the clock only when
read_permits.available_permits() == READ_PERMITS - 1; release the pause before guard 1's deadline. No cached pre-wait clock reading can satisfy the test.
Baseline for PR A is the post-PR149 exact-main Windows job. Prior reference: a18e95c CI43836099670, 433 s, nextest239 s, install6 s after C1. Old585/716 s and
Playwright199 s figures are historical. Added compile/test cost remains unmeasured; the660 s rule remains, with no new optimization/pin/version authorized.

## E7: new I4 fixture lifecycle correction

Coordinator Sol accepts Opus8de6d99 (retained0f6d0ff): the local I4 red is supported by cross-node term history as cleanup selecting the intentionally isolated old leader before it observes the new term. E3 still stops on the first red. After other-provider diagnosis and root contract check, one changed version may correct this new test's protocol ordering: record old term before isolation; after heal, wait at most5s for that node's metrics current_term to exceed it, with an explicit failure label, then unchanged converge. This is the same cleanup ordering used by the D1 S3 control. No Cluster helper, existing test, timeout constant, retry loop, quarantine or production behavior changes.

Also require an actual committed same-request replay response on the new leader (replay=true, same node and CSR key digest), followed by issuance retry and comparison of leaf SPKI to the original CSR. Keep oldissuer refusal and permit return, newleader success and all-node convergence assertions. The recorded first I4 red is the before-version evidence; do not rerun that head. This correction targets a new fixture, not the three failed shared-suite fixes, and is not a quarantined-test recovery claim. Any further local engine red stops again for disposition; no automatic additional tweak. Final I7 and the final-source PKI changes remain unverified until the new version's single verification round. No owner approval is needed for this ordinary reversible new-test correction under the authorized development scope.
