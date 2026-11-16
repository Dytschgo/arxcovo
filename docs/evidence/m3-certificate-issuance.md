# M3 certificate issuance PR A — stopped implementation

Builder: GPT-6.1 Sol, high, 2026-11-16. Prepared base `8961772`; coordinator supplied exact-main CI `43841341548` green on merged `f1c399d` (Windows 517 s, clippy 110 s, nextest 279 s, install 9 s, sampler all eight 28.766 s). Those baseline figures were supplied, not independently rerun here.

## Disposition

**Incomplete / blocked for coordinator disposition.** The first execution of the new issuance engine tests was red. No rerun or cluster fix was attempted after observing it. No deadline, Params, helper assertion, diagnostic, quarantine, filter, retry or workflow was changed. This is not a fourth Sol cluster fix. No push, PR, hosted dispatch, native M2, tooling installation, VM, admin step or PROGRESS edit occurred.

The failed test was `cluster::consensus::issue::issue_tests::i4_leadership_loss`. After the isolated issuer returned without a leaf, the new leader issued the retry, routing was healed and the unchanged `Cluster::converge` called `attributed_read(l).await.unwrap()`. That returned `ReadError::NotLeader`, panicking at `three_node.rs:222:52` in the executed source. This is a fresh-read helper refusal, not a timeout or Orphaned claim. The E3 red/no-rerun instruction is the stop boundary.

Bounded failure attribution (integer/enum data only):

```text
[DIAG-3n] attr helper=Converge fail=NotLeader node=1 t=3539 polls=0 retries=0
[DIAG-3n] s3 before=3539,1,L,1,4,4,4,3238 after=3539,1,L,1,4,4,4,3238 class=Churn
[DIAG-3n] at n1=3539,1,L,1,4,4,4,3239 n2=3539,2,F,3,6,6,6,- n3=3539,2,L,3,6,6,6,0
```

No cause or safe fixture correction is established. Coordinator disposition is required; do not infer permission to retry or weaken the fixture from this evidence.

## Actual commands and results

All Cargo compilation used a new task-owned target directory `D:/arxcovo-m3-a-20261116-d46c67c8/target` and temp directory under the same root. No cache cleanup or installs.

- `cargo update -p rustls-pki-types --precise 1.15.1` initially reported package not found because the baseline had no rcgen dependencies. `cargo metadata --format-version 1 --no-deps`, then `cargo update --workspace`, then the precise update and `cargo fetch --locked` succeeded. No existing version was upgraded.
- `cargo check --locked -p arxd --tests`: initial compile rejected my use of a struct literal for non-exhaustive `CertificateParams`. Corrected to default plus field assignments, preserving every specified field. This was an implementation syntax error, not a contract substitution.
- `cargo test --locked -p arxd cluster::pki --lib`: **4 passed**, 0 failed/ignored, 0.01 s test execution; cold build 64 s. This predates the subsequently added P-256/SHA384 OID mismatch check.
- First two attempts to compile the engine tests failed on missing/wrong `WatchReceiver` imports. The existing three-node source supplied the correct trait import. No test bodies executed in those attempts.
- First engine execution, `cargo test --locked -p arxd cluster::consensus::issue --lib`: **9 passed, 1 failed, 0 ignored**, 4.47 s execution, 21.19 s compile. I1/I2/I3/I5/I6/I8/I9 and hook identity/admission passed. The initial I7 was an insufficient adapter test; while that command was running, it was replaced with actual response and fresh-guard checks. The replacement I7 has **not been compiled or run**. I4 failed as above. No engine rerun.
- `cargo fmt --all`: completed successfully after those final test additions.
- `python -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`: **2 passed**, 0.020 s. The added test runs the production guard on both new source files and proves injected ignore/windows/unix/target_os/target_family mutations are rejected.

Not run: final-source compile, locked workspace clippy, final-source pure/enrollment tests, full nextest, existing cluster/crash probes and controls, sampler, sampler unit suite, workflow tests, full fmt check, final diff check, crypto/field-discard/guard/privacy behavioral mutants. No pass is claimed for them. The added engine wall-time result is below 25 s on that single execution but is red and gives no stability or acceptance credit. Hosted Windows added cost remains unmeasured; the 660 s rule is unchanged.

## Dependency and source pins

Pinned `rcgen =0.14.10` with only crypto/ring/x509-parser, `x509-parser =0.18.1` without defaults, `time =0.3.55` without defaults, and transitive rustls-pki-types 1.15.1. Cargo.lock contains exactly +38 packages and zero removed packages. All old name/version/source/checksum identities remain identical. Cargo necessarily qualifies old windows-sys and synstructure dependency strings when duplicate versions appear; after normalizing only those qualifications all old third-party dependency records are identical. arxd adds the three requested direct edges. No rustls package is added.

Added name/version inventory:

```text
asn1-rs 0.7.2
asn1-rs-derive 0.6.0
asn1-rs-impl 0.2.0
bit-vec 0.9.1
data-encoding 2.11.1
der-parser 10.0.0
deranged 0.5.9
getrandom 0.2.17
lazy_static 1.5.1
minimal-lexical 0.2.1
nom 7.1.3
num-bigint 0.4.8
num-conv 0.2.2
num-integer 0.1.47
oid-registry 0.8.1
powerfmt 0.2.1
rcgen 0.14.10
ring 0.17.14
rusticata-macros 4.1.0
rustls-pki-types 1.15.1
synstructure 0.13.2
time 0.3.55
time-core 0.1.9
time-macros 0.2.32
untrusted 0.9.0
windows-sys 0.52.0
windows-targets 0.52.6
windows_aarch64_gnullvm 0.52.6
windows_aarch64_msvc 0.52.6
windows_i686_gnu 0.52.6
windows_i686_gnullvm 0.52.6
windows_i686_msvc 0.52.6
windows_x86_64_gnu 0.52.6
windows_x86_64_gnullvm 0.52.6
windows_x86_64_msvc 0.52.6
x509-parser 0.18.1
yasna 0.6.0
zeroize 1.9.1
```

Primary implementation sources inspected: locally fetched rcgen 0.14.10 `src/csr.rs`, `src/certificate.rs`, `src/lib.rs`; OpenRaft 0.10.0 `src/raft/message/client_write.rs`; existing project fresh-read ownership and adapter source. Lock checksums bind fetched source. No real API/lock design conflict was found. This is not source/security approval.

## Public fixture provenance

Used the already installed Git OpenSSL executable; no install. Commands (temporary keys removed immediately after generation, never read into evidence):

```text
openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:P-384 -sha256 -nodes -subj /CN=public-fixture -keyout <task-temp>/p384.key -outform DER -out crates/arxd/src/cluster/pki/fixtures/p384_sha256.csr.der
openssl req -new -newkey rsa:2048 -sha256 -nodes -subj /CN=public-fixture -keyout <task-temp>/rsa.key -outform DER -out crates/arxd/src/cluster/pki/fixtures/rsa2048_sha256.csr.der
```

SHA256:

- p384_sha256.csr.der: `46633e68786c8b01fd14f46b518242a548aaf0478814328cf26aec292b1f8e9f`
- rsa2048_sha256.csr.der: `0ad090e754249a5434781a574e8f952dca00cc0b847da6236936dcf3453b11aa`

## Review limits and unfinished acceptance

Source implements caller-owned max-four admission, 5 s absolute await budget, no enrollment spawn, private ticket fields/constructor in issue.rs, fresh pre/post read guards, clock sampling after waits, proof dropped before the first await (no erasure claim), test-only ephemeral CA constructors, exact CSR algorithm/SPKI checks and fresh leaf parameters. Shared hooks are store-pointer keyed and async for issuance. Existing 64 proposal/eight read ownership and shutdown cap remain unchanged. These statements describe code intent, not approval.

Before readiness, finish all missing verification and meaningful mutants, assess I1 SPKI/serial and I4 replay evidence explicitly, strengthen any remaining test assertions and complete independent fresh Opus source review/five gates. Final I7 is unverified. Error privacy coverage and complete serial/SPKI assertions need review. No test-slice work establishes transport or production CA safety.

M2 remains open (A7 NO-GO, consumed A12 no retry); M3 remains unchecked. Store-file advisory 1 stays open: Weak expiry does not prove file closure. No production restart caller was added. All hardware paths are **unverified on real hardware**.


## Prepared-head verification: compile stop (2026-11-16)

Verifier: GPT-6.1 Sol, high, acting only as the test sub-agent; no delegation. Source pin: `f547459811d6e313728b923e6628ddd6c2870314`. Read AGENTS.md, KICKOFF, the issuance brief including E1–E7, Opus disposition `0f6d0ff`, and prior evidence `8c3436e`. The E7 I4 test body has old-term capture before isolation, the explicit new-leader `propose_response(Command::redeem(...))` asserting replay/node/key before `enroll`, raw leaf-SPKI equality with the original CSR, and the bounded higher-term wait after heal before unchanged convergence. This is source inspection only; it has not executed successfully. No production/helper/quarantine changes were made.

**STOP: final-source test compilation failed.** The single requested ordinary cluster nextest invocation reached Rust compilation and exited 101; no test body executed and there is no natural engine-red attribution. The previously uncompiled I7 calls `response.clone()` at `crates/arxd/src/cluster/consensus/issue_tests.rs:414` and `:420`. Both produce E0599: no method named `clone` for `ClientWriteResponse<consensus::Arx>`. No weaker adapter contract, source correction, rerun, or subsequent test command was attempted. This is the user's compile-error stop boundary, separate from E3's runtime-red rule and from deliberately expected mutant failures (none attempted).

Actual invocation (PowerShell environment scoped to that process):

```text
CARGO_TARGET_DIR=D:/arxcovo-a-verify-5aa012816b594d00b4707d580fb54fa5/target
TEMP=TMP=D:/arxcovo-a-verify-5aa012816b594d00b4707d580fb54fa5/tmp
CARGO_NET_OFFLINE=true
cargo nextest run --locked --offline -p arxd -E 'test(/^cluster::/)'
```

The target/temp/log directories were newly created under that unique D: root. No install, fetch, dependency resolution or cache cleanup occurred. The log covers 10:07:44–10:08:36 UTC (file timestamps, approximately 52 s; not test execution time). Output was redirected, then only its bounded compiler tail read. Full local log: `D:/arxcovo-a-verify-5aa012816b594d00b4707d580fb54fa5/logs/01-cluster-nextest.log`, SHA256 `649da943444de947204c2f45fa3940123ee3ad1caaa133c113f9ff9f6c70c884`. No raw private key was printed or captured by a test because no tests ran. This does not establish the requested behavioral secret-privacy coverage.

Read-only static pin validation completed against `8961772:Cargo.lock` using Python `tomllib`, `git show`, and SHA256, without Cargo re-resolution. Old count 243, final count 281: exactly +38, no removed or changed name/version/source/checksum identities. All old package metadata and dependency edges match after only the existing windows-sys/synstructure qualification exceptions; arxd adds exactly rcgen/time/x509-parser after the same normalization. Nine old package dependency records have textual differences including arxd. Final lock SHA256 `2fdb57ec3e214aea2e4716c1ff90ce5ba4f5ec0aa85ed1e41834e7a88a477c03`. Both public CSR hashes independently match the provenance entries above. No fixture bytes or keys were printed.

Static-validator bookkeeping: the first script incorrectly compared arxd's dependency strings before normalizing its existing windows-sys qualification and raised AssertionError. The corrected static validator applies the documented qualification exceptions to arxd as well as third-party records; it passed. This was a verifier-script assertion, not an engine/test failure or source change. Scripts and logs are retained under the task root: `verify-pins.py`, `logs/02-lock-fixtures.log` (initial assertion), `logs/02-lock-fixtures-final.log` (full inventory/edge/hash result, SHA256 `8ba9ddf2377710ad7ea79dbf479475a3f1099d249e562634377b174126fc0b97`). No behavior test was rerun. `git diff --quiet` returned 0 before this evidence update, confirming unchanged tracked source at the pin.

**Not run after the compile stop:** pure PKI including final exact-algorithm/OID check; enrollment matrix; I1–I9 and hook identity/admission; old crash and T/R tests; the single in-process `cargo test --locked --offline -p arxd --lib cluster::` interaction run; three opt-in D1 controls; quarantined eight sanity execution; fmt check; workspace all-target locked/offline Clippy; the two quarantine-guard tests and their injected platform/ignore cases; the 58 sampler unit tests; Node workflow tests. No focused engine rerun occurred. The planned first nextest command selected all ordinary cluster tests but compiled none to completion.

**Counterfactual gaps for Opus:** no staging copy or mutation execution occurred because compilation triggered STOP. Still required: focused exact-algorithm removal and original-SPKI defense (including the P-384/SHA256 case with the algorithm gate removed, then both defenses removed); CSR field-discard mutants; check_grant, fresh pre/post guards and clock sampling mutants; privacy scanner positive-control and persisted raw/hex marker counterfactuals. No expected mutant reds or surviving mutants are claimed. Existing static marker assertions do not substitute for these behavioral checks. Error privacy tests and fixture assertions remain unverified on this final source. Full-source independent Opus review remains next; this evidence is not a readiness approval.

Only this evidence file was edited. Bookkeeping repaired its known first-line byte 0x97 at offset 31 to UTF-8 em dash; the remaining historical bytes were preserved and the resulting document decodes strictly as UTF-8. No PROGRESS update (per this task's evidence-only constraint), network dispatch, native M2 run, VM/admin action, push, PR/status/merge/watch action, or TLS/production CA work. M2 remains open, M3 unchecked, and the eight Windows ignores unchanged. Hosted Windows stability is unverified; hardware paths remain **unverified on real hardware**.
