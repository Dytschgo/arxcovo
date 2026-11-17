# M3 next slice: anchored endpoint proof before redeem-secret release

Implemented locally; independent Sol high source review PASS f3cf3861. Hosted proof pending.
Original design: Sol 6.1 high, 2026-11-17. Required prepared base:
0375963b55277f27dbfcf9684c53b61d7294245f, containing actual main
59a665c9cda602d0b2939613656f2c98399f51f1 (PR173 atomic-anchor kernel).
Root subsequently confirmed main CI44114182696 attempt1 green:609s wall/Windows604s.
Root review clarifications R1/R2 are normative in ../reviews/m3-preproof-contract-root-review.md.
Independent Sol recheck9f3908d1 PASS; root authorizes local source under ADR0005.
M2 open; M3 unchecked; hardware **unverified on real hardware**.
Historical repair counters Sol3/3, Opus0/2 remain unchanged; no retry grant.

## Decision and observable result

Choose a pure bounded voter observation plus an internal, connection-owned TLS
proof-release gate, demonstrated with one actual founder and in-memory TLS.
Trigger: a joiner holding arxj2 reaches a CA-pinned TLS endpoint and obtains an
untrusted voter list. Result: only a completed TLS1.3 connection whose leaf key
is in the canonical list matching the code anchor can carry the redeem proof.
A CA-key-only endpoint completes chain validation but receives ZERO proof bytes.
This resolves feasibility of the pre-proof identity boundary, not production join.
No address hint, code expansion, discovery, listener, admission or receipt needed.

Source facts: EnrollmentState::voter_anchor already hashes uniform applied
membership plus registry bindings. membership.rs validates canonical membership;
learners/addresses are excluded from voter_anchor. pki/verify.rs PeerVerifier
requires a registry and expected target; AuthenticatedPeer exposes only node ID.
Neither is a bootstrap verifier. Existing client_config also requires a client
leaf, which a pre-redeem joiner does not have. Do not fabricate either credential.

## Allowlist and smallest build order

All paths below are relative to crates/arxd/src/cluster/ unless stated otherwise.
1. New enrollment/anchor.rs and its pure tests; enrollment.rs module/re-export,
   voter_anchor delegation only; enrollment/code.rs one crate-internal proof-byte
   accessor if required by step2. No command/apply/codec/schema changes.
2. New pki/preproof.rs and pki/preproof_tests.rs; pki.rs module bindings;
   pki/verify.rs share bounded identity/chain helpers at pki-only visibility.
   Existing peer verifier/config behavior and authorization must remain identical.
3. New consensus/anchored_join_tests.rs; consensus.rs cfg(test) binding only.
   tools/ci/test_windows_cluster_quarantine.py additive inventory guard only.
   No other production consensus, storage, issuance, founder or old test edits.
Each step belongs to this one security concern; no cleanup/dependency work.

## Pure data and APIs

BoundVoters has private storage, at most16 (NodeId,KeyDigest) pairs. Provide
observe(&EnrollmentState,&Membership), decode(&[u8]), encode(), anchor(cluster),
and contains(node,key). observe is a BORROWED, synchronous pure operation;
caller must supply state and membership from the SAME locked StateStore image.
Never zip independently sampled metrics/registry. Refuse absent membership at
caller; refuse joint (even identical configs), empty or any unbound voter.
No endpoint-local assumption that node1 is always the whole voter set.
Wire-shaped list is count:u8 followed by sorted(node:u64 BE,key_digest32)*.
Length must equal1+40*count, count1..16, unique increasing nonzero IDs, unique
key digests (registry invariant), no suffix/truncation. Check length/count BEFORE
allocation; maximum641 bytes (calculation). Learners and all addresses stay out.
Hash exactly current domain/cluster/count/pairs; delegate existing voter_anchor
through this implementation to avoid two definitions. Existing literal vector
and apply-time tests must remain unchanged and still pass.
No pending-code lookup, availability oracle or AnchorQuery service is added.
The future query must enforce pending/unexpired code and uniform refusal;
this pure observation must never be advertised as that authorization check.

## Dedicated pre-proof TLS boundary

Internal PreproofClient owns its ClientConnection privately; no arbitrary config,
raw connection constructor/getter, writer, Deref or mutable connection escape.
Constructor takes JoinCode, bounded root DER (<=4096) and existing TimeProvider;
check SHA256(root DER)==code.ca_fingerprint BEFORE root installation. Keep only
cluster/code_id/anchor/fingerprint metadata, not the code secret, between calls.
Build own TLS1.3-only, no-client-auth config using pinned rustls/ring provider.
Verify chain/time, one bounded leaf/no intermediates, canonical cluster/node URI
and SPKI digest as current verifier does; delegate real TLS1.3 signature checking.
Reject TLS1.2; disable SNI, resumption and early data. This initial chain check
permits ONLY pre-proof exchange; it confers no registry/owner/member authority.
Do not return AuthenticatedPeer or accept an always-Ok Registry to bypass it.

Expose only TLS ciphertext feed/drain/process operations plus gate methods.
Before proof, application write is fixed query cluster16+code_id16 only, once;
no generic application writer. accept_anchor(list_bytes) validates BoundVoters,
code digest, completed full TLS1.3 handshake and THIS connection's leaf
(cluster,node,key) membership. Use peer_certificates only after handshake;
verify_server_cert success alone does not prove CertificateVerify possession.
Keep list/session binding private. Invalid/incomplete/mismatched input fails
closed; fatal TLS/gate errors make session unusable. Never reconnect inside gate.
release_proof(&JoinCode) rechecks code metadata and completion, queues exactly
32 proof bytes through this connection's writer ONCE, then consumes proof authorization; Released may only drain bounded ciphertext.
Proof bytes are this internal probe's payload, not an enrollment wire protocol.
No CSR/redeem dispatch is exposed; the trusted test bridge below calls enroll.
Fatal state rejects all later calls, including drain; partial proof-write failure is fatal.
Gate returns static errors without code/list/cert contents. Do not log proof,
secret, code text, private key, list or receipt; no Debug containing those values.
Existing JoinCode::proof remains trusted internal API, not a crate-wide capability
fence. This wrapper establishes its own reviewed call path, not sandbox isolation.

## Capacity, deadlines and ownership

No new async read API, task, semaphore or production blocking work. Pure observe
holds no Arc/mutex beyond its borrow and copies <=16 pairs; no scanning database.
No changes to8 read/4 issue/64 proposal units, READ_BUDGET1500ms, issue5s or
shutdown5s. Existing fresh_read cancellation ownership and weak peer_registry
remain frozen. In particular no receipt/list closure may capture StateStore Arc.
A future local query is a separate slice: both issue/read owned permits and store
Arc must live INSIDE its blocking closure through completion after caller loss;
closed Owners must refuse. This brief does not implement or prove that query.
TLS is driven in memory on a joined blocking thread, max32 handshake pump rounds,
fixed <=641-byte list input and32-byte proof output; set rustls output buffer4096.
The wrapper AND driver cap cumulative ciphertext at128KiB per direction over the WHOLE session,
with checked pre-admission counters, bounded drain buffers and terminal quota errors (root R1).
No unbounded Vec pump or IO wait, network sockets, queue or background retry.
Single-founder proof uses cooperative40s entry budget:35s work,5s cleanup reserve;
existing operation budgets unchanged. Drop connections/router handles and join
blocking work before checking shutdown Ok and weak store/log expiry. Startup
spawn_blocking is not abortable: unresolved startup fails with explicit cleanup
unknown; no universal teardown/time promise, reaper or detached cleanup fix.

## Meaningful verification and counterfactuals (future, not run here)

Pure: literal list/hash compatibility; every truncation, count0/17, oversized,
suffix, descending/duplicate/zero ID and duplicate key refuses. Joint/unbound
refuses; address-only and learner-only changes preserve list/hash, voter-key or
voter-set changes invalidate it. Decoder must not canonicalize attacker order.
TLS: real cryptographic handshakes, certificate/key parsing and received plaintext
counts, not callback-only tests or encrypted-packet marker searches. Good pinned
CA/list/leaf releases expected32 bytes; incomplete handshake emits zero proof.
Foreign root, wrong cluster/key/node, altered list, early/repeated release refuse.
Valid anchored leaf with a test resolver signing using another key must fail
CertificateVerify and release zero proof (no forged ticket or registry).
Required stolen-CA control: same test CA signs attacker's DIFFERENT key with the
real voter URI. Test-only adversary signs directly with rcgen and its CA key;
no forged IssueTicket, test_ticket, pki::leaf, seed registry or assume identity.
Attacker offers genuine anchored list: TLS chain succeeds, key gate refuses,
server plaintext lacks proof. With attacker's own list, digest gate refuses.

One all-OS case anchored_preproof_single_voter: ONE ConsensusNode::start,
founder_init/found produce genuine founder leaf. On joined blocking work lock
actual store once, decode its membership and observe list; release store Arc
before TLS. Mint fresh code through existing trusted proposal API. In-memory
no-client-auth SERVER config is test-only, using genuine founder leaf/key.
Joiner has no leaf/registry. Pump real handshake; server sends bounded list;
feed received bytes to gate on that same client; assert no client certificate.
After successful proof receipt, test bridge compares bytes then calls actual
enroll with same code/request/real CSR; check genuine leaf/original commit.
This bridge is not public RPC/CSR transport or a joiner restart implementation.
Run attacker controls on pure TLS; no engine start per mutant. No Cluster helper.

Designated once-only mutants, restored exact bytes and hashes after each:
M1 omit anchor-digest check: attacker-list case must fail zero-proof assertion.
M2 omit endpoint-key membership: same-CA/wrong-key/genuine-list case must fail
zero-proof assertion. M3 replace TLS1.3 signature verification with assertion:
wrong CertificateVerify signer now completes; its zero-proof assertion must fail.
Compilation failures/unrelated failures/survival earn no kill credit.
Do not count refused attacker handshakes alone as evidence of proof nonrelease.

## Inventories, gates and frozen semantics

Preserve fixture28 Windows-ignored/Unix-required, sample8, founder13, TLS14 and
existing atomic issuance1 all-OS. Add separate ANCHORED_JOIN_REQUIRED inventory
with exactly anchored_preproof_single_voter, required all OS; additive pure tests
separately listed. Guard omission/rename/ignore/Unix-gating/module hiding, inspect
single-start body AND helpers, and ensure old sampler/filter/counts unchanged.
No local Windows engine execution authorized; pure TLS/mutants only locally;
new engine proof must run in ordinary hosted all-OS CI. Separate Sol exact-head
source review, normal five gates and hosted Windows<=660s remain mandatory.
Natural semantic/static-suite failure stops for root disposition; preserve all
existing exhausted counters. Root sets any mechanical-correction allowance.
Frozen: arxj2 length270/proof-v2, E-1 fence, original receipt position and fresh
signer time, effective retained55, limits/schemas, replay/revocation, old tests,
service/control/executor/UI, ingress, Cargo/lock, workflow/nextest/quarantine policy.

Resolved here: bounded list representation and same-session proof-release proof.
Still blocked: public query ownership/dispatch, discovery, daemon-sealed CA/joiner
key custody, durable receipt/bootstrap permissions, authenticated owner/control,
clock/skew/real expiry, admission/promotion controller and joint recovery.
Those technical prerequisites need separate briefs. No new owner-choice gap is
required for this slice; longer codes/new discovery or product choices stay out.
Proposed PROGRESS (not applied): Sol6.1 high design; anchored-list/pre-proof TLS
brief prepared, static local/cached API inspection only; root review/fresh exact
main green pending; no source/build/test/engine/network/hardware acceptance.

## Local source status (2026-11-17)

Source9b6c7e06 plus test-driver correctionf0c13d23 has passing evidence for all26
selected pure cases across recorded rounds; M1/M2/M3 compiled and failed the
actual received-proof32 versus0 assertion, with exact restoration. Final fmt
and Clippy passed. Independent Sol source review f3cf3861 PASS (retained320c6b48).
The original static and semantic stops remain in the chronological evidence.
Advisory: assertion/API panic can bypass explicit founder-test cleanup; no
cleanup-after-every-failure guarantee is claimed. See [local evidence](../evidence/m3-anchored-preproof.md).
Exact-head review and hosted all-OS proof remain required, with normal gates,
Windows<=660s and fresh exact main. No local engine or hardware credit.
