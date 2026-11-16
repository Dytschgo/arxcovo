# M3 PR B certificate verifier evidence

2026-11-16; builder GPT-6.1 Sol, high effort. Prepared base `71607f5`, source
commit `bacb3e403c0da64393c7b5f5edda9361e7bf1799`. This is **stopped work,
not merge-ready**. The first unmutated engine fixture failure triggered the
user's explicit no-fix/no-rerun rule. No source change followed that failure.
Root owns PROGRESS and briefs; neither was edited.

## Contract and implementation

Read AGENTS.md, KICKOFF, and the issuance first-slice brief including readiness
E1–E7, final V1–V9 and root Sol API/trust clarifications. The supplied readiness
references (readiness4c9cb65, Solcontractcheck51706df, merged issuance6231da9,
exact-main CI43854991316) are coordinator evidence, not locally rechecked CI.
The supplied Windows baseline is verify595s / clippy127s / nextest338s /
install10s and exact-eight sample29.735s. None is a PR B measurement.

`pki/verify.rs` adds a cluster-root-only trust store, mandatory mutual TLS1.3,
explicit ring provider and per-config TimeProvider. It rejects intermediates
and >4096-byte leaves before parsing, uses native webpki chain/time checks,
then requires a fully consumed X509 DER certificate with one SAN extension,
one GeneralName URI, strict cluster/node parsing and the raw SPKI digest.
Registry errors use PKI-owned unit VerifyRefusal, including Unavailable;
enrollment PeerRefusal is unchanged. TLS1.3 handshake signatures use rustls's
ring algorithm set on both sides; TLS1.2 signature entrypoints refuse.
PeerVerifier Debug is fixed text and custom errors contain no inputs.

Client verification requires an expected node, independently of the supplied
ServerName. Configs disable SNI and connection tests use `arxcovo.invalid`.
`AuthenticatedPeer` construction is private and exposed adapters require a
completed TLS1.3 ClientConnection or ServerConnection before parsing the
verified peer certificate. These are trusted reviewed adapters for these
configs, not evidence unforgeable against arbitrary insecure crate configs.
The test-only assume constructor confers no founder certificate credit.

`ConsensusNode::peer_registry` captures only sm_weak, upgrades on each call,
drops its Arc before return and refuses dead/poisoned stores. Authorization
uses local applied state: staleness is unbounded, with no quorum or freshness
claim. The mutex wait is unbounded. Production callers must decide their
threading. All test handshake pumps run on std threads or spawn_blocking,
use at most32 byte-pump rounds and open no sockets/listeners.

MemRouter's default-empty optional authenticated map is keyed by source.
No Factory field, shared Cluster helper, Params, deadline, three_node.rs or
consensus/tests.rs change was made. The new sender fixture installs real
handshake node4 as leader-source identity while source==claimed, observes an
increased mismatch counter and another leader, waits for the old leader's
new term before convergence (E7), then installs an explicit assume(L) control.
It checks and resets its intentional mismatch count before the unchanged
finish helper's zero-mismatch assertion.

Pure tests cover happy mutual handshake/extraction, incomplete extraction,
target mismatch, foreign same-DN CA with same leaf key, wrong SPKI, malformed
DER/truncation/trailing data, SAN shapes/foreign cluster/malformed URI,
intermediates/oversize precedence, absent client certificate, exact validity
bounds and outside bounds through direct client verification and handshakes,
and both-side TLS1.3 valid/tampered signature and TLS1.2 refusal entrypoints.
Engine tests use actual enroll leaves for nodes4/5, target binding, removal
following local apply, stale isolated acceptance, sender binding and the
300ms store gate with >=20 heartbeat ticks. Joiner PKCS8 is test-local; only
public leaf DER and CSR enter enrollment/verifier APIs. Existing extra-marker
scanner checks raw/hex joiner key, CA key, code secret, proof and owner-transfer
markers without printing them; new fixtures supply positive-control markers.

The shutdown/poison fixture did not reach its assertions. Coverage remains
incomplete: no behavioral mutants ran; SAN duplicate-extension sensitivity,
negative assertions across both verifier directions, precise native time error
variants and independent handshake-alert sensitivity need review. Its current
self-signed control asserts refusal, without proving which chain/time check
caused refusal. Do not infer complete brief acceptance from the green tests.

## Dependency identities and primary API verification

Cargo.toml adds exactly rustls `=0.23.46`, default-features=false,
features=[ring,std]. Lock precise webpki0.103.16; existing pki-types1.15.1.
Python tomllib compared every package (name,version,source,checksum) with base
HEAD lock: exactly two additions, zero changed or removed identities. Existing
arxd gains the rustls edge; no unrelated upgrades. Initial resolution selected
webpki0.103.17, then `cargo update -p rustls-webpki --precise 0.103.16` set the
required final identity before any check/build.

Both additions have source `registry+https://github.com/rust-lang/crates.io-index`:

| Name | Version | Full checksum |
| --- | --- | --- |
| rustls | 0.23.46 | 48e13bd8c0e9365c43cfa5c9e8f9ad49d3c8444926c9aac819e0e4dc503c8fdf |
| rustls-webpki | 0.103.16 | 4443b1b6d39d7c663139942cc4558262562d2c84123bb6282ef177009ca8b281 |
| rustls-pki-types (unchanged) | 1.15.1 | 2f4925028c7eb5d1fcdaf196971378ed9d2c1c4efc7dc5d011256f76c99c0a96 |

SHA256 of all three local Cargo registry .crate archives matched those
checksums. Read primary unpacked rustls0.23.46 source: src/verify.rs
(ServerCertVerifier, ClientCertVerifier, DigitallySignedStruct Codec),
src/webpki/verify.rs (chain check with no DNS check, TLS1.3 signature),
src/webpki/server_verifier.rs, src/client/client_conn.rs and
src/server/server_conn.rs (builder_with_details), src/time_provider.rs,
src/lib.rs (ParsedCertificate and codec exports), and src/error.rs.
Read rcgen0.14.10 src/key_pair.rs for PublicKeyData/SigningKey traits.
These APIs compiled against the exact locked packages; no API contract
conflict was observed. No global provider install or implicit config builder
is used. No tooling installation occurred.

Primary source archives: [rustls0.23.46](https://static.crates.io/crates/rustls/rustls-0.23.46.crate),
[webpki0.103.16](https://static.crates.io/crates/rustls-webpki/rustls-webpki-0.103.16.crate).
These links identify the local checked archive contents; this session did not
perform a separate browser retrieval.

## Actual checks and stop evidence

Unique target `D:/arxcovo-b-94adb0b1/target`, engine-run TEMP/TMP
`D:/arxcovo-b-94adb0b1/tmp`; no cache cleanup. Local dev/test profile includes
debuginfo, unlike hosted CI's job-wide debug0. Timings are local, not hosted
Windows verify timing, and cannot establish compliance with660s.

- `cargo check -p arxd --locked --tests`: final compile passed3.47s. Earlier
  compile passes exposed missing rcgen trait imports and missing test timeout
  import/proof accessor; fixed before running engine fixtures.
- First `cargo test -p arxd --locked --lib cluster::pki::tls_tests`: build62s,
  tests0.01s;3passed/1failed. The foreign same-DN CA test incorrectly demanded
  UnknownIssuer alone. A native BadSignature is also a correct rejection for
  the same-DN anchor; changed only the assertion to accept those two native
  certificate errors. No trust behavior was weakened.
- Changed pure test source: same command,4passed,0failed, tests0.01s,
  build10.68s, overall11.174s. This preceded final formatting/source compile.
- `cargo nextest run -p arxd --locked -E 'test(/^cluster::/)'`: exit100,
  overall23.643s, build17.55s, execution4.645s.25/144selected ran:
  24passed,1failed;119not run due to default fail-fast.468outside selection
  skipped. Run ID b8e8508b-efae-4dd1-8683-63845a9f1bd8.
- Failure: `cluster::consensus::issue::tls_engine_tests::live_verifier_does_not_retain_store_and_poison_refuses`,
  0.075s. `tls_engine_tests.rs:294`, `.await.unwrap()` on
  ConsensusNode::start returned `Open(Io)`. This precedes CA/verifier creation,
  poison injection and shutdown, so **none of those assertions was exercised**.
  No fix or rerun, new quarantine or fourth shared cluster fix followed.
- Three other new engine tests passed during that same invocation:
  enrolled_target_binding_revocation_and_stale_acceptance0.945s;
  store_lock_wait_does_not_block_runtime0.944s;
  authenticated_sender_mismatch_and_assumed_founder_control2.423s.
  I1–I9 and the issuance hook/retained-record tests also passed in this run.
- Guard unittest:2passed0.024s, including new-file same bypass mutants and
  remove-binding mutants. Both exact new bindings checked exactly once.
- Sampler unittests:58passed0.067s. No actual diagnostic sample execution.
- `node --test scripts/*.test.mjs`:29passed,0failed,2.873s.
- `cargo fmt --all --check`: exit0. Guard/sampler/Node/fmt commands were
  initiated while nextest was in flight, before observing its engine failure;
  no further checks were started after observing the failure.

Logs retained locally in D:/arxcovo-b-94adb0b1 (not committed):

| Log | SHA256 |
| --- | --- |
| cluster-nextest.log | 74fe594c262c74f71d955fdacd8c5c6b0a3319261209f9be753050037074bd7c |
| pure.log | 120b24ec083fd02a84ed052e4c8a7b1bf7e7098a772231951999f8375a9c2fbb |
| pure2.log | c47e55161d93fad3aed36267db0bc919d83aa75eeb6b45d5b72799e174d03480 |
| check4.log | 118c018eb2630f8a4a157947723b24a901fe8a0cf02b769eed7f656f01ca6288 |

Read-only inspection after stop found this new single-node fixture uses plain
tempfile::tempdir, while the established Cluster fixture calls private_temp,
which sets a private Windows ACL (and Unix mode). This is a possible fixture
precondition difference, **not a verified root cause**. No diagnostic rerun
or corrective source edit was made. Coordinator disposition and an independent
provider diagnosis are needed before a changed fixture version is attempted.

## Unrun and constraints

After stop: no in-process `cargo test --locked -p arxd --lib cluster::`, no
workspace all-target locked Clippy, no final full workspace nextest, no
behavioral crypto/identity/registry/privacy mutants, no D1 epoch or induced
latency controls, no exact-eight sample execution, no hosted dispatch or
Windows PR B verify timing. D1 controls were neither changed nor credited.
Guard's embedded bypass mutants are the only mutant execution credited.

Fresh full-source Opus review, all five gates and Windows660s rule remain
required before any merge. Eight quarantine attributes and exact-eight
sampler inventory source remain unchanged; no platform cfg/ignore/filter/
retry/scheduling/deadline increase added to the new tests. The cluster-only
command above is a local validation selection, not a CI filter change.
No helper or production-store correction is authorized by this evidence.

An initial read-only `git status --short` was inadvertently run despite the
no-status instruction; it returned no entries. No further status command was
run. No subagents, push, PR creation, merge, watch, hosted dispatch, install,
VM/admin/native M2 or old Lab Manager action occurred. No secrets, keys or
proofs are committed or released in evidence. No transport or production CA
exists in this slice. M2 remains open; M3 unchecked; all hardware paths are
**unverified on real hardware**.
