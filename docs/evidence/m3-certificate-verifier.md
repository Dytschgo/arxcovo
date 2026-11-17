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


## Coordinator private-fixture correction and final local checks

Sol diagnosed the first startup failure with a focused precondition test on the
same plain temporary-directory setup: the directory failed the existing security
privacy check before any node could start. That diagnostic failed at its explicit
privacy assertion (D:/Temp/arx-tls-private-precondition-red.log). Commit34902ca
uses the established private_temp helper in this new fixture and keeps an explicit
privacy precondition regression. Production privacy checks and shared cluster
helpers are unchanged. This is a new-fixture correction, not a historical Windows
cluster fix. The original failed engine run above remains recorded.

At34902ca, one changed-source cluster nextest run passed145/145 in10.315s.
Quarantine guard2, sampler58, Node29, format and diff checks passed. Logs are
D:/Temp/arx-tls-private-{cluster,guard,sampler,node}.log. Clippy then stopped on
self_named_constructors in the private certificate extraction helper. Commit
d1c207d renamed that helper to from_verified_certificates with no behavior change
or suppression. Final workspace all-target locked/offline Clippy passed in8.72s
(D:/Temp/arx-tls-final-clippy.log), and in-process cluster tests passed145/145,
13ignored, in9.29s (D:/Temp/arx-tls-final-inprocess.log).

These are coordinator runs, separate from the builder's evidence. Behavioral
mutants, full independent TLS/security source review and hosted timing are still
pending. No Windows stability, production transport, production CA or hardware
acceptance is claimed. M2 remains open and M3 unchecked.


## Opus findings and coordinator resumption correction

Opus reviewa647147 retainedb344df7 found B1: reusable rustls configs resumed
TLS1.3 without calling either certificate verifier. Local removal and expiry
could therefore be bypassed. Required gaps R1/R2 were independent server-side
chain/time coverage and exact native time-refusal assertions. None is dismissed
by the earlier green tests.

Sol4c16354 disables client resumption, replaces server session storage with
NoServerSessionStorage and sends zero TLS1.3 tickets. Reused configs must now
perform full handshakes with both registry checks; removal then gives NotMember.
The expiry regression first signs in near the leaf's end and advances only
90minutes, keeping session-ticket age short while crossing certificate expiry.
Direct client-certificate checks independently cover foreign same-DN CA,
self-signed certificate, NotValidYetContext, ExpiredContext, KeyMismatch,
NotMember and WrongCluster. Existing client-side direct/handshake time tests now
assert the exact native variants. The handshake pump was factored to expose
completed connections for full-handshake assertions; its32round cap is unchanged.

Before the production correction, the reused-config test failed with Resumed
rather than Full (D:/Temp/arx-tls-review-red.log). An initial expiry test advanced
30days and passed because its ticket had aged out; that inadequate setup was
replaced. The corrected near-expiry test failed on old production code because
the resumed connection succeeded (D:/Temp/arx-tls-expiry-red.log). After correction,
all7pureTLS tests passed (D:/Temp/arx-tls-review-fixed.log). Removing only the
server-side chain call compiled and failed the new direct-client test at its
foreign-CA refusal assertion (D:/Temp/arx-tls-server-chain-mutant.log); original
bytes were restored in a finally block and compared exactly.

Merge277c3e9 integrates reviewed store-close main e601caa. Source merged without
conflicts; the two doc conflicts retain B addenda and refresh PROGRESS. Combined
nextest passed152/152 in9.591s, in-process tests152/152 with13ignored in9.17s,
and workspace all-target Clippy passed7.14s, all locked/offline. Guard2, sampler58,
Node29, fmt and diff checks pass. Logs: D:/Temp/arx-tls-resumption-{cluster,
inprocess,clippy,node}.log. After those runs, the self-signed direct-client fixture
was strengthened to be in-date with the correct URI/EKU, and its expected native
refusal narrowed to UnknownIssuer or BadSignature. That final focused test passes
(D:/Temp/arx-tls-server-final.log), and final arxd all-target Clippy passes3.57s
(D:/Temp/arx-tls-server-final-clippy.log). No broad rerun is credited for this
last test-only refinement; production and engine source remain identical.

Opus recheck of the correction remains required. Main e601caa CI43862021319 is
in progress at this checkpoint; no TLS publication/merge credit is granted.
PR158 merged only after all5gates and fresh exact-main6231da9 green. Its Windows
PR verify was543s; the new main and PR B timing are not yet known. M2 open, M3
unchecked, eight Windows quarantines unchanged, hardware unverified.

Final changed source pins (supersede earlier pins only for these paths):
- `crates/arxd/src/cluster/pki/verify.rs`: `c8c198f4ee784c093c339930f83c91214b617f22`.
- `crates/arxd/src/cluster/pki/tls_tests.rs`: `dd5ef2b3ade262ddc8fa35eca660c73122b35d49`.


## R3 independent per-side resumption regression

Opus recheck0dea866 retained56e6f07 closes B1/R1/R2 and approves production
4c16354. R3 requests independent tests: either safe side masked a regression
on the other when both production configs were paired. Sol coordinator adds
three test-only cases; no production code changes.

The protected production client is paired with a server that issues tickets and
stores sessions; the protected production server faces a client with resumption
enabled. Only the protected side's registry observes removal, while its peer
retains the valid binding. Each test completes two full handshakes, then expects
NotMember after removal. A both-resuming control produces Full then Resumed,
proving the test peer can exercise the bypass. These are ordinary required tests
on all platforms, with no ignore, scheduling, helper or timeout change.

An initial compile found ServerSessionMemoryCache::new already returns Arc;
removing the redundant wrapper fixed the test-only construction before runtime.
No runtime test ran on that compile failure. All10pureTLS tests then passed.
Each isolated per-side production revert compiles and fails only the matching
focused test with Resumed versus Full (D:/Temp/arx-tls-r3-client-mutant.log and
D:/Temp/arx-tls-r3-server-mutant.log). After each run, original verify.rs bytes
were restored in finally and compared exactly. Restored pureTLS suite passes
10/10 in0.02s (D:/Temp/arx-tls-r3-restored.log); arxd all-target locked/offline
Clippy passes6.65s (D:/Temp/arx-tls-r3-clippy.log), fmt/diff pass. The prior152
nextest/in-process and auxiliary runs are reused; no new full-suite pass is
claimed for three added pure tests. Test blob now supersedes prior pin:
`tls_tests.rs`: `1626658eb4277f7c7f1ad4c930bdb08af533c01f`.

Main e601caa CI43862021319 completed green: Ubuntu221s, Windows477s,
macOS269s active job duration. This confirms main after PR158; it grants no
TLS PR B hosted timing or Windows stability credit. Opus R3 recheck remains
pending. M2 open, M3 unchecked, eight quarantines unchanged.


## Publication checkpoint

Opus R3 recheck6930dc6 retained4852d17 closes the last blocker at c2eceeb.
It independently ran15TLS/engine controls, isolated per-side reverts (9pass/1fail
each), both reverts and removal-only probes; restored10pure tests, Clippy,
format/diff and guard2 pass. Those are Opus runs, separate from root's focused
single-test mutant logs. No source changed after c2eceeb. Earlier full review and
correction recheck remain retained, including their initial blockers and limits.

Source review is now PASS on Standards, Spec and Security. Hosted verification,
exact-head publication review and Windows<=660s still pending. Current main
e601caa CI43862021319 is green (Windows477s); re-read before merge. The separate
log-close regression e6e7a88 is not included in this TLS PR. No transport,
production CA, quarantine recovery, M2 or full M3 acceptance credit is given.


## Founder-main integration - 2026-11-17 (Sol high)

Prepared base `8676a6f7607c1f9ac5f4ee87b1798f8ba9e34b7c`, source main
`c40c547721641974bcb407ce343c795e6c6609ec`; merge reviewed PR159 head
`3d702d17a6175c054c73292aca0385bd8740f2a2`. Root supplied the prerequisite:
exact-main CI44058803351 attempt1 SUCCESS, 632s wall / Windows628s, founder13
per OS. This sub-agent did not query hosted state. The recorded root contract
applies the owner's whole-fixture decision to the three TLS Cluster cases.

The merge retains main's entire sealed LeafSubject/founder binding block then
PR159's entire peer_registry/TLS binding tail, byte for byte. PR159 verifier,
pure TLS tests, mem_net and pki.rs are unchanged. The engine-test delta against
PR159 is exactly +12 attribute lines, no body changes. Client/server resumption
disabling, both per-side adversarial tests and the both-resuming control remain
byte-identical. No production, timing, retry, scheduling, transport or custody
redesign was made. The guard's new keyword required=() keeps existing callers'
behavior and validates all 27 fixture gates, the TLS 3/1/1 classification,
constructors, module bindings and founder seal/bindings. Every specified mutant
fails, including a Cluster constructor inserted into the required live test.

### Actual local checks (single final round)

- Compile-first gate: `cargo test -p arxd --lib --no-run --locked --offline`
  passed (87s fresh build). Real nextest listing subsequently compiled the final
  gated source (10.23s), before runtime. No Cargo API change was necessary.
- Identity script passed: issue.rs equals main plus exactly the PR159 tail;
  four reviewed TLS blobs preserved; engine source differs only by three gates;
  Cargo files equal PR159; main fixture sources, sampler/filter and sampler tests
  unchanged; EXPECTED/ISSUANCE/REASON declarations unchanged.
- Cargo name/version/source/checksum inventory: exactly +2, zero removed or
  changed packages. Existing arxd package adds only its rustls dependency edge.
- Quarantine unittest suite: 3/3 pass including all old and new mutants. Unchanged
  sampler suite: 60/60 pass. `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` pass
  (Clippy 38.91s). `git diff --check` passed.
- Real Windows nextest JSON inventory: all 27 fixture cases ignored; sample FILTER
  matches exactly the original eight (checked with the real sampler inventory
  validator). All 10 pure TLS plus required live verifier and directory precondition
  =12 unignored; all13 founder and pure read-attribution unignored. Three existing
  D1 controls and two crash-child harnesses retain their prior ignores.
- One ordinary `cargo nextest run -p arxd --lib --locked --offline
  -E 'test(/^cluster::/)'`: **151 passed**, 449 skipped, 5.617s runtime.
  Log confirms all10 pure TLS, both required engine-file tests and all13 founder
  passed. No natural runtime red or engine correction occurred.
- Exact-name OR selection of the three quarantined TLS cases: **0 run**, 600
  skipped, actual exit4 (expected). No ignored test was executed, including the
  diagnostic sample. No Windows fixture execution or recovery credit.

One routine pre-verification correction: Python's default Windows text encoding
could not write a Unicode historical progress line; restored the original bytes
and used explicit UTF-8. No runtime test preceded that correction. No second
routine correction, engine fix, rerun, install, cache deletion or hosted action.

Fresh build target/temp and logs retained at `D:/Temp/arxcovo-tls-integration-24e5c5ed079e4c12943de1079a646549` (target/, tmp/, logs/).
All own commands exited; caches retained. Log SHA-256s:

- `compile.log`: `93717f60f74974555abd1476b5115ed2f9421a4f71221eb649752db34233135b`.
- `identity.json`: `d55874293098b376e3ac564cc062251eaf573325a8e983a40286bf25c19b9fe3`.
- `guard.log`: `239ba389ac6161932a0c9428d0b07c37e2ca1da56cb377076c3e4cd2ee6a1931`.
- `sampler.log`: `c52280c9614bc998e6ad53bb19ad25ac2f00dfddae9b04d94448f9d0a58f0db1`.
- `fmt.log`: `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`.
- `clippy.log`: `add1ef0300aac69d0c402d768f07ee7822787155ef9af0922979b8940d865cee`.
- `list.json`: `c6ef09ed5402800905f409da0067ab535142d258aa5e984fcf0426d084a336eb`.
- `sample-list.json`: `1384bf26c161bc067952ec0969e7b41c380eec6de5e060410caf7f1a315f9f52`.
- `inventory.json`: `8ff9167a2a5e61070b1e021dcbde3dd0de231e557dd05e0e62366d7001a0c2da`.
- `cluster.log`: `59f7bc4eaec4af0a79d64cc9955f9e3398a58e98e1e7236154a81b44eb08d1f2`.
- `exact-three.log`: `20846ff8df6984ccc95da8c1052bd8d7814a79e0ffd5a23d19bfcf27bbaea25d`.

Final tested source blob pins (subsequent evidence commit changes docs only):

- `crates/arxd/src/cluster/pki/verify.rs`: `c8c198f4ee784c093c339930f83c91214b617f22`.
- `crates/arxd/src/cluster/pki/tls_tests.rs`: `1626658eb4277f7c7f1ad4c930bdb08af533c01f`.
- `crates/arxd/src/cluster/consensus/mem_net.rs`: `4bd971e67f63ed500d7753478731cb320a87f708`.
- `crates/arxd/src/cluster/pki.rs`: `9f8dfcf7b1171896f892c9e85b240964022653d6`.
- `crates/arxd/src/cluster/consensus/issue.rs`: `9e318aedd58d65ca93865ac206eed234ac55c24c`.
- `crates/arxd/src/cluster/consensus/tls_engine_tests.rs`: `674f6d53f6a567d631eee10f5dbcd8fa8bf526b9`.
- `tools/ci/test_windows_cluster_quarantine.py`: `927e47a4d0be42ba6a36e14ccc7fb3b136208045`.
- `Cargo.lock`: `6bb1f1c0840176b4411aa7b88a469c448f34d925`.
- `crates/arxd/Cargo.toml`: `f0a31a70c99a7dc45ddf762c0ef57647cf9fccb8`.

Dependency checksums:
- rustls0.23.46: `48e13bd8c0e9365c43cfa5c9e8f9ad49d3c8444926c9aac819e0e4dc503c8fdf`.
- rustls-webpki0.103.16: `4443b1b6d39d7c663139942cc4558262562d2c84123bb6282ef177009ca8b281`.

### Reused evidence and remaining gates

Cryptographic mutants are reused from the retained Opus R3 review
`docs/reviews/m3-certificate-verifier-opus-r3-recheck.md` (6930dc6, retained4852d17):
per-side reverts each produced9pass/1fail, removal-only probes also killed, restored
10pure tests passed. Earlier B1/R1/R2 review evidence is retained in the two prior
review files. Those mutants were not rerun: reviewed verifier/pure-test bytes and
resumption settings are identical and integration adds only fixture attributes.
The current real cluster run independently exercises all required pure TLS tests.

Original Ubuntu S8 CI43865758301 and Windows F1/F2 remain failures, with no shared
cause or stability claim. Windows failure evidence remains on
work/m3-pr159-windows-failure (`640a6dcc`..`d9adeb99`); not rewritten here. Root's
recorded decision releases only Windows merge eligibility after this 27-case
guard. Stability/re-enable remains held. Root must obtain independent Opus delta
review before push and exact-head publication review. All five normal gates and
hosted Windows <=660s remain required, without optimization or waiver. Ubuntu S8
gets one legitimate updated-source observation after publication; red means stop,
no rerun. Ubuntu/macOS must execute all27 and every TLS test.

No push, PR/status/comment/watch, remote merge, main/root branch mutation,
worktree creation, children, native VM/admin or old installation action. This is
a local integration merge commit only. M2 open, M3 unchecked; Sol3/3, Opus0/2
unchanged. TLS, Windows fixture behavior and transport are **unverified on real
hardware**. Hosted timing, independent delta review, non-Windows execution and
stability/re-enable are not locally verified or credited.
