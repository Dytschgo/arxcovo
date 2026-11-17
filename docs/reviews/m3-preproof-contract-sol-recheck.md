# Anchored pre-proof contract: independent Sol recheck

Sol 6.1 high, 2026-11-17; independent contract review, no children.
Prepared head: 089e3e6dccde47cd73b7c5533434f1474880d994.
Actual main base: 59a665c9cda602d0b2939613656f2c98399f51f1.
Reviewed AGENTS, KICKOFF, ADR0005 including adopted D1/D2/D3 and atomic
amendment, brief e859ff16/retained79f5fa01 as present at prepared head,
and normative root review R1/R2. Applied dytschgo-review skill.
Local tree was clean. Base-to-head changes are documentation only.

## Standards

Blocking: none. The allowlist supports this bounded concern without changing
Cargo, workflows, production consensus/storage/issuance or existing tests.
No implementation smell finding is possible from a design-only patch.
No new owner approval step or product decision is needed.

Advisory S1: keep new adversary fixtures self-contained in the allowed files.
Existing pki::leaf and test_ticket helpers are unsuitable for these controls.
Identity/helper visibility may widen to the pki ancestor only; do not widen
CA signing authority. New inventory checks and their negative controls fit
the allowed CI guard file; old sampler/filter/count assertions stay intact.

## Spec

Blocking: none. BoundVoters can borrow EnrollmentState and canonical
Membership under one StateStore guard: state_store.rs:366/369 expose both.
enrollment.rs:319 already hashes domain, cluster, count and sorted voter/key
pairs. Delegation can retain the literal vector and apply behavior unchanged.
The list maximum is 1 + 16 * (8 + 32) = 641 bytes. Duplicate key rejection
agrees with the registry invariant. Missing/joint/unbound state refuses;
addresses and learners never enter the anchor. arxj2 stays fixed at270.

The single-founder proof is statically feasible using founder_init, found,
propose and enroll, retaining the founder CSR key locally for the test server.
The existing issuance test at issue_tests.rs:750 demonstrates those real APIs;
it is reference only, not a helper to invoke or edit. Observe on joined blocking
work while holding one actual store guard, then release that Arc before TLS.
The new all-OS anchored_preproof_single_voter needs its separate inventory,
module/ignore/rename/omission guards and single-start helper inspection.
Fixture28/sample8/founder13/TLS14/atomic-issuance1 remain unchanged.

Advisory P1: a successful proof release consumes authorization, while bounded
ciphertext draining must remain possible. A private Released state can retain
the connection but reject further query/anchor/proof operations. Fatal state
must reject all later operations, including draining queued proof. No writer,
raw connection or arbitrary config need escape. Keep only code metadata before
release; passing JoinCode by borrow avoids retaining its secret between calls.

The cooperative40s budget includes5s cleanup. Join blocking work, drop TLS and
router handles, require shutdown Ok and weak store/log expiry before PASS.
Unresolved non-abortable startup must report cleanup unknown and fail honestly.
No universal teardown promise follows from bounded pump rounds or byte counts.

## Security and cached API feasibility

Blocking: none with normative R1/R2. Inspected cached rustls0.23.46 and
rcgen0.14.10 source only; no compilation or execution credit.

R1 is enforceable. conn.rs:784/812 accept dyn Read/Write; vecbuf.rs write_to
consumes exactly the successful write_vectored return count. A slice-backed
reader and caller-buffer writer can preserve partial reads/writes without an
internal growing Vec or borrowing rustls buffers across calls. Check the entire
offered input length with checked_add against incoming128KiB BEFORE read_tls
or packet processing. Track consumed bytes consistently, return partial counts
or loop over the bounded remainder, and never silently discard a suffix.
For output, a private writer can limit copying to caller capacity and checked
remaining quota, recording only actual emitted bytes. Normal buffer exhaustion
is a short write; quota/overflow is terminal. IoState::tls_bytes_to_write also
permits queued-output preflight after processing. Counters cover handshake,
query, list and proof without resets. Tests must hit exact/over quota, repeated
small-feed aggregate, arithmetic overflow and post-failure zero-proof behavior
on the wrapper itself, independently of driver caps.

Advisory C1: set_buffer_limit(Some(4096)) is an application-write bound
(conn.rs:499), not a handshake quota. R1 counters remain essential. Do not use
an adapter that reports bytes written without copying them. Reject oversized
root/leaf/list before copying; keep root/leaf4096, list641, fixed query32 and
proof32. Partial proof writes cannot be retried as a fresh release; mark fatal
and prohibit subsequent drain on any write failure.

R2 is feasible without forged tickets or production CA access. rcgen provides
KeyPair::generate_for, CertificateParams::self_signed/signed_by and Issuer::new
for a synthetic CA signing both genuine and attacker leaves with canonical URI.
rustls crypto/signer.rs exposes CertifiedKey::new, SigningKey::choose_scheme
and Signer::sign. A test-only resolver via with_cert_resolver can pair the valid
anchored leaf with another real same-algorithm private signing key; unlike
from_der/with_single_cert, this construction does not demand keys_match.
server/tls13.rs:827 signs CertificateVerify through that key. client/tls13.rs:1180
checks the valid chain, then invokes the configured TLS1.3 signature callback
before Finished. Changing ONLY the new callback to assertion can therefore
complete this handshake and reach actual proof release. This is static
feasibility, not an executed mutant kill.

Advisory C2: retain evidence that the intact wrong-signer case reached possession
checking with valid chain/list/key binding. M3 must then queue and deliver proof,
breaking the same server-plaintext zero-proof assertion. For M1 use the attacker's
actual key in its list; for M2 use the genuine matching digest/list. Drain and
count server plaintext by protocol phase, distinguishing query32 from proof32;
callback failure, marker searches and unrelated refusal earn no kill credit.

The private config can use TLS1.3-only ring, with_no_client_auth, disabled SNI,
resumption and early data, injected TimeProvider, and the existing bounded
chain/URI/SPKI semantics. Pin root DER fingerprint before installing trust.
Check completed full handshake and THIS connection's peer leaf before release;
chain callback success alone is insufficient. Existing peer authorizations stay
unchanged. No AuthenticatedPeer or always-Ok Registry belongs in bootstrap.

## Evidence limits and disposition

Reused root evidence only: main CI44114182696 attempt1 green,609s wall,
Windows604s, all92 tracked cases each OS,28 Unix fixtures. Recorded non-gating
Windows U6 Stop/Orphaned sample with no orphans remains quarantined evidence;
no cause, repair or stability credit. No network, Rust/Cargo/build/test, engine,
mutant, install, native/VM/admin or protected-system operation was performed.

Ready for root source authorization; this review grants none. Separate Sol
source/exact-head review, five normal gates, hosted feature Windows<=660s and
fresh green exact main remain required. No local Windows engine authorization.
Sol3/3 and historical Opus0/2 counters remain unchanged; no retry grant.
M2 open / M3 unchecked; hardware **unverified on real hardware**.
Public listener/query ownership, discovery, receipt/key custody, owner/control,
admission/controller and clock policy remain separate blocked slices.

Findings: Standards0 blocking/1 advisory; Spec0 blocking/1 advisory;
Security0 blocking/2 advisory. No concrete contract blocker remains.
