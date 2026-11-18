# M3 authenticated identity: bounded implementation

Sol6.1/high, 2026-11-18; no children. Source a75962718aa6290681a2392c1ec825307af10bf0;
assigned base57c8d379d8321b31732d67635a965dc9b329570b, main05b9c829 ancestor.
Four allowed source paths only. PROGRESS unchanged; no unmerged query dependency.

Private node/optional cluster+key retain the single peer_identity parse. Clone+Copy,
node and constructor APIs remain; cluster-visible accessor refuses None as Incomplete.
assume keeps node behavior with None. TLS validation/configs remain frozen.
Exactly three plain synchronous tests implement both-handles TLS identity/SPKI,
incomplete refusal, genuine second-cluster rcgen CA/leaf with production configs,
actual Found/Remove outcomes and binding controls, and assume denial. Two-member
fixtures avoid LastMember; booleans reach designated assertions after fixture drop.
No helper threads or universal panic-cleanup claim. Runtime remains unverified.
Dedicated three-name guard pins both parent bindings/path and hiding/omission/
duplicate/weak-inventory mutants. Old classifiers remain unchanged, including
TLS14/founder13/fixture28/sample8/atomic1/anchored-proof1. Preserve future query set.

## Actual validation

Owned exec721c93: fmt exit0; Python quarantine guard8 passed; sampler60 passed.
Exec572501: four-path scope, exact append, UTF8 without BOM/LF passed.
git diff --check passed. Static transcript below is transcribed, not raw output.
One locked/offline workspace all-target Clippy -D warnings: exit0,39.750s,
owned session25644 completion5b1f31. No mechanical batch/recheck used.
One cargo test -p arxd --locked --offline cluster::pki::tls_tests:: --
--test-threads=1: cargo exit101,40.703s; wrapper exit1, owned session79896,
completiond8b4ea. Compilation failed: rustc-LLVM ERROR: IO failure on output
stream: no space on device. No tests ran. C: free0 observed; STOP, no retry.
M1/M2 never applied or run: no kills or restoration claim, no postrestore test.
verify.rs SHA256 a65991d2ebe729fd9071ab66e36e760fc88d888f79039df1d0d48b33f4b1f5ba
remains the exact baseline. No semantic fix attempted; counters unchanged.

Private target/identity-validation/{cargo-home,build} only; registry copied read-only
from shared cache, no reparse points reported; resolved workspace containment checked.
Owned copy session22415 exit0 (c6dafb); all owned execs completed. No process-name
ownership guess. No cache deletion/network/install/engine/admin/VM/hosted action.
Disk-full also blocked evidence writing: an empty docs commit was corrected by
removing only two self-created runner scripts, preserving all cache/build/log data.

Local ignored logs in target/identity-validation/logs (not committed artifacts), SHA256:
clippy.json: 123658a7b314657a5ddad4d4396e847afea017f1d209e20ce5e95397c6434924
clippy.txt: ceed786d44c9e9ff28a69c85884e5cd404029f88ccc54ef1a39b397c6a7c4afa
focused.json: fce42973fe56702179866e16d7a01d8b61b77a096174ad806e9cb5f3944eef99
focused.txt: 0891c5ff3e7297276a67d9006923428ab87a8d474cc98427fcec802635486fcd
scope.txt: e5719f4c346720da0a99a1b1104bb0366bd64a5d872ef45c4ffe189064aafbb6
static-transcript.txt: 5616611a7e21810d11850053bbbe3e3a84fefdb46eb7cfec568d24c0719a643e

## Handoff / proposed PROGRESS (not applied)

Sol6.1/high source a7596271: fmt/static8/sampler60/scope and Clippy pass; sole TLS
invocation compile-blocked by disk full. Zero runtime/mutation credit. Independent
actual-diff Astra6.2 review required before publication. Root must resolve resources
and authorize further validation; normal hosted gates and fresh-main feature Win
<=660 remain unrun. Supplied mainCI44212212325 green608wall/605charged/Win604 and
PR176 state are context only, not independently fetched or reused as source proof.
No PR/push/watch/merge. Last merged PR unchanged; Sol3/3 historical Opus0/2 unchanged;
M2 open, M3 unchecked. Hardware **unverified on real hardware**. No admission,
transport, custody, freshness, owner-authority or production acceptance claim.

## Root guard correction after Astra review - 2026-11-18

Astra b989fb90 reviewed092dcf9c: production/three Rust blobs match contract;
B1 raw matching accepted block-commented tests/bindings and B2 accepted async.
Root changes only the new identity guard: reject block-comment delimiters in
the three guarded files (currently line comments only), anchor test declarations,
and explicitly reject async signatures. Negative controls cover commented source,
commented test attributes, each commented parent binding and each async test.
This conservative block-comment fence is intentional; no Rust lexer is claimed.
The first editing script accidentally changed the older fixture matcher too,
causing six Python unpack errors. Root restored the entire pre-identity prefix
from HEAD and compared every existing main function/class AST unchanged.
Corrected CI-equivalent quarantine suite:9/9 passed in0.124s; diffcheck clean.
No Rust or engine execution in this correction. Separate D-volume validation
remains pending; its immutable Rust baseline is unchanged by this guard edit.
Independent Astra delta recheck and hosted gates still required.

## Integrated D-drive validation continuation - 2026-11-18

Sol6.1/high, validation only, no children. Tool-prepared isolated worktree
t3-f4e7e857-fd1f7b08703d99bc, branch t3/identity-validation-evidence, exact base
092dcf9c729e3e34111fa33dd8bc753b3b98c944. Integrated source be3f9281
on supplied green main e3e79089; no hosted state was queried in this task.
The three identity blobs equal original a75962718aa6290681a2392c1ec825307af10bf0:
verify.rs 7f60471a863eb8423316e919c83bc254903c12f9;
tls_tests.rs 8bed50125797725ed4bafc40b3836334ae0353dc;
authenticated_identity_tests.rs 8bfa9a416bb757dce4e43d68b6f60532ede563d1.
Current merged query and founder diagnostics/guard were left unchanged.
This is root-authorized compilation-resource recovery, not a failed-test rerun.

Owned artifact root: D:/Temp/arx-identity-d-validation-f4e7e857.
CARGO_HOME=cargo-home, CARGO_TARGET_DIR=target, TMP=TEMP=tmp, logs=logs under
that root for every Cargo invocation; CARGO_NET_OFFLINE=true,
RUSTUP_TOOLCHAIN=1.95.0, CARGO_INCREMENTAL=0. Existing toolchain used.
Resolved absolute paths and existing ancestors were checked for reparse points;
source registry cache/index/src trees were checked recursively before copying.
D free at preparation: 2792416669696 bytes, above required 50 GiB; each Cargo
invocation also checked the 50 GiB floor. Only registry/cache,index,src from the
authorized old task-owned cargo-home were copied, not credentials/root config.
Copy exit0, 228.171s; owned exec session67299 completed exit0. Old C build/cache/logs
preserved. No cache deletion/move, install, network, engine, VM or admin action.

### Commands and actual outcomes

All commands below ran exactly once, locked/offline. No mechanical correction
batch or recheck was needed. Times are Cargo process wall times, including build.

1. cargo clippy --workspace --all-targets --locked --offline -- -D warnings
   Exit0, 39.227s; owned cargo PID48720, exec62462 completed exit0.
2. cargo test -p arxd --lib --locked --offline cluster::pki::tls_tests:: -- --test-threads=1
   Exit0, 53.616s; owned cargo PID38928, exec73359 completed exit0.
   Actual result: 13 passed (old10+new3), 0 failed/ignored, 635 filtered,
   test time0.11s. In-memory TLS and pure registry only, no engines.
3. M1: cargo test -p arxd --lib --locked --offline cluster::pki::tls_tests::authenticated_identity_tests::peer_identity_retains_verified_tuple -- --exact --test-threads=1
   Cargo exit101, 26.463s; owned cargo PID53504. Mutation wrapper exec15081 exit0.
   Replaced binding Some((identity.cluster,identity.key)) with
   Some(([2;16],identity.key)) after the unchanged real peer_identity parse.
   Compilation succeeded; exactly 1 test ran, 0 passed/1 failed, 647 filtered.
   Designated assertion at authenticated_identity_tests.rs:116 failed:
   "actual first-cluster identity retained on both handles".
   This is the intended semantic kill after fixtures drop, not a setup failure.
4. M2: cargo test -p arxd --lib --locked --offline cluster::pki::tls_tests::authenticated_identity_tests::peer_identity_assumed_founder_denied -- --exact --test-threads=1
   Cargo exit101, 27.249s; owned cargo PID49064. Mutation wrapper exec1746 exit0.
   In assume only, replaced None with
   Some(([1;16],KeyDigest::of_spki(b"designated synthetic mutant").unwrap())).
   Compilation succeeded; exactly 1 test ran, 0 passed/1 failed, 647 filtered.
   Designated assertion at authenticated_identity_tests.rs:217 failed:
   "synthetic founder has no authenticated identity".
   This is the intended semantic kill, not a setup failure.

Both wrappers restored baseline verify.rs bytes in finally, then compared bytes
and SHA256 after EACH mutant. Baseline/restored-after-M1/restored-after-M2 SHA256:
a65991d2ebe729fd9071ab66e36e760fc88d888f79039df1d0d48b33f4b1f5ba.
M1 mutant SHA256:
ec64c8862b38ee114a51f430a6df5bda40ae1704b6d4871b0b91ad76c8476929.
M2 mutant SHA256:
173777e00813dbd6a51a449271f8982f8b51a4046e4af284244c13d6c5aa60ab.
Patches, stdout/stderr, timing/exit records and per-mutant restoration records
are retained under the owned logs root. No postrestore rerun was performed.
The retained target binary is therefore a mutant build, not restored-source proof.
All four owned Cargo PIDs reported HasExited=true after WaitForExit; all owned
exec sessions completed. No global process-name snapshot was used as ownership
evidence, and no unrelated processes were classified or killed.

Before this evidence append, every tracked file matched the initial SHA256
manifest; git diff was empty. Final checks passed: only this evidence file
changed; source identity, ASCII UTF8 without BOM/LF and git diff --check passed.
No additional static suite or tests were run; earlier static evidence remains
historical. No F6/F10/smoke/broad suite, push/PR/comment/hosted-status/watch/merge/CI action.

### Local artifact SHA256

Logs are local retained artifacts, not committed raw output. Paths below are
relative to the owned logs root; artifacts-sha256.txt contains the full inventory.

121f0d80470ab4360edbf890736ac57809b5c23bd4f496c103d09951f1ede907  baseline-sha256.txt
8c2937e552e6eb49efd36494306f0d1610aeb3854681e85e50dc424c6726a3f4  baseline.json
a23b8d2f8aa445f3793ae921a520ffcfd26362f3cf8f00fbe02c0d05654c8387  baseline.stderr.txt
21f94e0749642a1164d4e6b2593140dea976266122f37bdaad4d37a5ada9f401  baseline.stdout.txt
7afa939182431d307409e504db4e06c8a739ef2ce17adfd764b47941c3de2668  clippy.json
05521afa8c80f381d97be6dec0bc53b96d4d6c74968f794c6c92abacab313ad5  clippy.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy.stdout.txt
bd14e5e57a79785a5980454f064d9d213305de41b7df48dd25d2877451f710b3  m1-restoration.json
55cf98a5154b2f8e40437d29d9e666449bc35ae5d447a756502a0ef7bab73a5a  m1.json
430afefb210a17c3ffad89381021da92001936a5d0c55924d0196c33f8ffb396  m1.patch
c926458f5b3a4c8c14e8e426b4b32342795559e42c1dba48e2b909b1f3bff301  m1.stderr.txt
77b3cb08aa8217d1106536e23523e221238c3f562ad939beb44d2abe9589629b  m1.stdout.txt
84b2efc2e0c5af8f248623873d860650c3e72b2742f1641866053ed475c7c262  m2-restoration.json
a511d11fad073966b8413f2bbbbcee8a7b60365d087de17a01def9cab624ec34  m2.json
d80ad7ea8c62b38956e2a582394f655669317a2dc414d2ca01aba7f37b0fea00  m2.patch
decd5c8c98eb9a760161e09eac1d6cd7b89281ca64ce48959e8decdfb63c1b4c  m2.stderr.txt
1e1f4a7e87cf353d4bbe3a5f635fb6d8137e73c57ea6f921f99ec01df39a7175  m2.stdout.txt
a2912578e9bf2e204380fac1a1fdd6f1c53850d89102da94259d318e66e34559  setup.json

Runner SHA256:
5df2640b2b3fca1087780e8a50f8a5a71dba6fef6dff872e6eabbcec5565bf1b run.ps1
c5437af3c1632343c62cfefe69b8fce2312a61478d04fc26134bf0e97a40f6fb mutate.ps1

### Proposed PROGRESS follow-up (not applied)

Sol6.1/high integrated identity validation at092dcf9c: one D-only offline
all-target workspace Clippy passed; one focused TLS baseline13/13 passed;
M1/M2 each compiled and failed its designated assertion once. Exact verify.rs
bytes/hash restored after each, no postrestore rerun; all owned execs completed.
No source correction, engine run, hardware, hosted or publication credit.
Independent immutable-base Astra review remains a separate gate; all five hosted
gates, fresh green main and feature Windows<=660s remain pending. M2 open,
M3 unchecked, Sol3/3 and historical Opus0/2 unchanged; last merged PR unchanged.
Hardware paths **unverified on real hardware**. This proves retained TLS identity
and pure registry controls only, not admission, production transport, freshness,
custody, owner authority or milestone acceptance.

## Root evidence integration and parent-attribute correction

Root verified all18 declared D-validation artifact hashes, read baseline13/13
and both intended mutation failures, and integrated evidence-only6b572258.
No runtime rerun. Source restoration and Clippy remain attributed to Sol logs.
Astra a6e5970e closes B1/B2 but found B3: a line-commented first cfg(test)
attribute still matched either parent. Root now requires the complete binding
to start at line start and adds that exact negative control for both parents.
Rust files and all earlier main guard definitions remain unchanged.
