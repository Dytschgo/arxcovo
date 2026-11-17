# Anchored pre-proof local source: stopped WIP

Sol 6.1 high implementation child, no children, 2026-11-17. Assigned isolated
worktree only. **Incomplete WIP; no publication or behavior acceptance.**
M2 open / M3 unchecked; hardware **unverified on real hardware**.
Historical Sol3/3 and Opus0/2 remain unchanged; no Windows repair credit.

## Pins and scope

Authorized unpublished base `be3598241007038764207d5a77a24e1ce0ab4b64`;
actual main `59a665c9cda602d0b2939613656f2c98399f51f1`.
Source WIP `9b6c7e0` (full SHA in local artifact manifest).
Read AGENTS, full KICKOFF, engineering skill/behavior safeguards/active profile,
full anchored brief, normative root R1/R2, independent Sol recheck and latest
ADR0005 amendment. Root-supplied main CI44114182696 attempt1 success is reused
only:609s wall/Windows604, all92 tracked cases per OS/Unix fixtures28.
Windows U6 Stop/Orphaned non-gating failure earns no cause/fix/re-enable credit.
No remote query was made.

Source changes are confined to the ten allowed paths: new anchor, preproof,
preproof tests and anchored founder test; enrollment module/delegation and proof
byte accessor; pki module/identity visibility; consensus test binding; additive
quarantine guard. Existing tests, peer verification bodies, Cargo/lock,
workflows, policies, timeouts, retries, permits and PROGRESS are unchanged.
No public endpoint/query, discovery, signing authority, enrollment schema or
production engine behavior was added. JoinCode::proof remains trusted internal
API; the wrapper only establishes its own connection-owned path.

## Mandatory stop

The first scope/UTF8 classifier asserted that every untracked path belonged to
the source allowlist. It included its generated `.task-evidence/inventory.json`
(currently untracked evidence) and failed before reaching UTF8 byte comparisons.
This is a classifier failure, not observed out-of-allowlist source mutation.
Per explicit natural static-suite stop rule, no correction or rerun occurred.
No pure Rust behavior test or mutant ran. No mutation kill or restore credit.
The inventory operation had already started before this failure; its final
state is recorded below. No new verification operation was started after stop.
`git diff --check` after stop was an additional read-only preservation check
(exit0); it is disclosed rather than counted as the prescribed verification.

## Actual commands and correction accounting

All Cargo compilation uses task-specific `target/anchored-preproof`, locked and
offline flags (fmt has no dependency resolution). Logs/caches are retained in
`.task-evidence` in this assigned worktree; no installs, network, admin, native,
VM, old Lab Manager, protected VM or cache deletion operations occurred.

- `cargo fmt --all` applied formatting; `cargo fmt --all -- --check` exit0 before
  first Clippy and again before inventory.
- `cargo clippy --locked --offline --workspace --all-targets -- -D warnings`
  first exit101: incorrect `super::found` import; unused production `encode`.
- Mechanical correction batch1: import `issue::found::founder_init`; annotate
  encode's intentional no-production-query-caller dead code outside tests.
  Same Clippy command then exit0,14.05s. One of two build correction batches
  used; no behavior-driven fix, counter reset or algorithm chase.
- `python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py`
  exit0,7 tests,0.301s. Includes additive required binding, omission, rename,
  ignore, Unix hiding and helper/start-count negative controls.
- `python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py`
  exit0,60 tests,0.264s. Old sampler unchanged.
- Inline Python scope/UTF8 classifier exit1, AssertionError on generated JSON;
  mandatory stop. Scope/UTF8 byte comparison acceptance **UNRUN**.

Preparatory read commands: PowerShell rejected Unix brace expansion in an rg
argument (parser error); corrected read used explicit filenames. A guessed
`.config/nextest.toml` path was absent; another rg glob was invalid on Windows.
These read-only command failures changed no source and ran no behavior checks.
They are separate from the single compile/import/lint correction batch.

Before the first behavior checks, pure quota/gate controls and founder cleanup
ownership were completed. Clippy covers their final source. New tests cover
actual TLS plaintext phases, synthetic CA attacker lists/keys and a mismatched
CertificateVerify resolver; none has runtime pass credit. Quota tests include
pre-admission, aggregate/overflow, caller-buffer drains and partial-write fatal
controls; none has runtime pass credit. Independent review must assess coverage
and implementation fully; static construction is not proof of those claims.

## Remaining work and proposed PROGRESS (not applied)

Root disposition is required before any classifier correction, natural pure
round or designated M1/M2/M3 execution. Exact M1/M2/M3 patches, runtime logs and
restore hashes do not exist because no mutant was authorized after stop.
All natural anchor/preproof TLS cases, existing pure pki cases/TLS10 and exact
literal_anchor_and_proof_binding remain **UNRUN**. No local founder/consensus
engine case ran. New anchored_preproof_single_voter first runtime evidence must
come from ordinary hosted all-OS CI. Existing fixture28, sample8, founder13,
TLS14 and atomic issuance1 runtime/listing acceptance is not inferred from
source or guard passes.

Independent full source and exact-head Sol review, normal five gates, hosted
all-OS proof, feature Windows<=660s and fresh exact main remain mandatory.
No push, PR, comment, status, watch, link, merge or protection change occurred.

Proposed PROGRESS: Sol6.1 high anchored-preproof source9b6c7e0 WIP; fmt/Clippy
pass after one mechanical batch; guard7/sampler60 pass; scope classifier stopped
on generated JSON before UTF8 checks. Natural Rust cases/mutants unrun; root
disposition, independent source/exact-head review and hosted gates pending.
M2 open/M3 unchecked; hardware unverified on real hardware; counters unchanged.

## Retained artifacts and final ownership

Already-started `cargo nextest list --workspace --all-targets --locked --offline
--message-format json` finished exit0. Its inventory.json is101589 bytes and
inventory-build.log6375 bytes. No test executed. No inventory classification
was attempted after the mandatory stop, so protected count/ignore pinning is
still unverified for this head. The pending listing's shell and child processes
finished; all three owned Cargo sessions returned completion.

Source full SHA: `9b6c7e060e54d9ec8b44fc12e0715ab92412a3bb`.
Source patch SHA256: `d7bc1e6cb7d3a20e003132809cc2d273c89a7b93d341bada6a693739d1ada002`.
Local artifact manifest `.task-evidence/artifact-hashes.json` SHA256:
`11bfeae6bf5407097eb3505cfa62ba64b9f6055b4d8e9de755a27929b1d477d2`. It pins both Clippy logs, both fmt logs, guard,
sampler, listing/build output, recorded scope failure and full source patch.
No mutation or restoration hashes are claimed. Logs/caches remain in the
assigned worktree; generated evidence files are retained locally, uncommitted.
The source stop was preserved without a classifier fix or natural-suite replay.

## Root scope-classifier disposition (2026-11-17)

Root Sol 6.1 medium inspected the retained scope-failure.log: the checker mixed
generated .task-evidence/inventory.json with four new source files. Root limited
the source comparison to the committed source diff and inspected evidence
separately. All ten source paths exactly match the allowlist; strict UTF-8/LF
passes for all ten. Existing TLS/pki/enrollment tests, issuance, adapter, Cargo
lock, CI workflow and sampler match the prepared base byte-for-byte. verify.rs
changes only pki-parent visibility of Identity/fields/peer_identity; bodies match.
No repository source or classifier code was changed to close this artifact error.

Root classified the existing successful nextest listing without rerunning Cargo:
fixture28 ignored, founder13/TLS14/atomic issuance1 required, and the new
anchored_preproof_single_voter required. Sample selection remains attributed to
guard7/sampler60; this full listing is not an engine or sample execution.
Artifact D:/Temp/arx-preproof-root-disposition/scope-inventory.json SHA256:
9e15b5249775f42b0f08a93613037eca40d3b490df6608f86ccff2ea761fa8f3.

Root authorizes resuming the previously UNRUN pure anchor/preproof/TLS tests,
existing pure pki cases and literal anchor regression, then the three designated
once-only mutants if natural checks pass. No passing guard/sampler/Clippy/fmt
rerun is needed absent a source edit or concrete concern. Any natural behavior
failure, mutant survival/compiler/unrelated failure or scope need stops again
without correction/retry. Production source remains9b6c7e06. No local Windows
engine execution, publication or acceptance; separate source review follows.
The initial compile correction batch remains1; this is a static artifact-check
disposition, not a behavior fix or counter reset.

## Bounded pure continuation: first semantic stop (2026-11-17)

Sol 6.1 high test child, no children. Prepared local head
`7bc58ddd99230c5215480231c84c5218119baddd`; source remains
`9b6c7e060e54d9ec8b44fc12e0715ab92412a3bb`. Read AGENTS/KICKOFF, full
anchored brief, normative root R1/R2, independent contract recheck and this
record including root scope disposition. Only the assigned isolated worktree
was used as cwd. No source correction was authorized or attempted.

Reused, not rerun: root scope/UTF8/frozen-byte disposition and inventory counts;
prior fmt/Clippy, guard7 and sampler60. Saved inventory SHA256
`c6a6ceb4a377a4fd9e1340eb71203e5b8156197761ff61c6c4c4273d2e33b3c4`
was read to enumerate exact names before selection. Inspected anchor/preproof
fixtures and existing pure PKI/TLS source. No new nextest listing was needed.

### Actual natural round and mandatory stop

One command, locked/offline, serial, no retries, fail-fast:
`cargo nextest run --locked --offline -p arxd --lib --test-threads 1 --retries 0 --fail-fast -E <exact-name union>`.
The literal argv and all 26 selected names are retained in
`D:/Temp/arx-preproof-continuation-98c62e96/natural-command.json`.
Selection was exactly enrollment::anchor2, pki::preproof quota2,
pki::preproof_tests5, pki::tests6, pki::tls_tests10 and existing enrollment
literal_anchor_and_proof_binding1. No consensus/founder-engine/anchored_join
case was selected. Existing founder_pure_identity_and_validity is a pure case
in pki::tests, but remained unrun after fail-fast.

Task-exclusive reused target:
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-04a56b7c-8088f9385f5b4a14/target/anchored-preproof`.
Cargo rebuilt this assigned worktree's arxd; compilation succeeded in48.88s.
Overall subprocess50.110s, exit100; nextest runtime0.117s, run ID
`7ac5f0bf-b1c9-47e0-a3e5-6214c5eff8e3`. Results:

- PASS canonical_bounds_and_order (0.028s).
- PASS observation_excludes_addresses_and_learners (0.024s).
- PASS literal_anchor_and_proof_binding (0.021s).
- FAIL partial_proof_write_is_fatal_and_cannot_drain (0.029s).
- Remaining22 selected tests UNRUN;607 other tests skipped.

The first failure is the existing new fixture setup assertion at
`crates/arxd/src/cluster/pki/preproof_tests.rs:286`, in prepared():
`assert_eq!(plaintext(&mut s).len(), 32)` observed left0/right32 after query
pumping. It occurs before the partial-proof-write test reaches its intended
buffer-limit/write-failure assertion. This is a natural semantic failure,
not a compile/import failure or designated mutant kill. No cause, fix or
partial-write behavior acceptance is claimed. No fix, rerun or subsequent
verification suite followed. Root disposition is required.

The runner saved its Cargo exit/duration before a console-export error:
printing Unicode nextest separators to cp1252 raised UnicodeEncodeError.
This affected only display after test completion. The retained UTF-8 log was
then read with ASCII escaping; no behavior command was repeated. Future runner
summaries must retain ASCII escaping. Its shell exited1; actual Cargo exit100
is independently retained in natural-result.json and the completed log.

### Evidence, source preservation and remaining work

All continuation artifacts are retained outside the repository at
`D:/Temp/arx-preproof-continuation-98c62e96/`:

- natural.log SHA256 `48d0aa6a4eddaee46f6411bd8eb9d01d505dec94d49fdaecd47ec1d154714ff3`.
- natural-command.json SHA256 `d0792361fc73d827aaafa05c8a776233f0438d4ceb8f409a3b9747b1554986c0`.
- natural-result.json SHA256 `a3ac57a8d2fcd357b46e6916393ea04a48ae447b06e6abf044f6a0e8eccd8c1c`.
- manifest.json SHA256 `3405260e771d0c11f7cfa0d24b99bdf5f61ab7a7fa26ec7316e9ccf351f6628a`.

Read-only byte preservation compared all ten source paths to git objects at
source9b6c7e06: all exact equality PASS. Manifest records each source SHA256
and runner hashes. This is preservation evidence, not a repeat of the natural
suite or root's classifier acceptance. Git tree was clean after the failure
before this evidence-only edit.

M1/M2/M3 all UNRUN: natural-pass precondition failed. An external mutants.py
was prepared while compilation ran, but never executed. No mutant patch was
applied, no source backup/restore operation was necessary, and there are no
mutant patch/restore/kill hashes or credits. No natural replay after stop.
The sole owned runner and Cargo/nextest process tree completed; no background
process was started. Logs/caches remain intact.

PROGRESS is root-owned and unchanged. Proposed entry: Sol6.1 high bounded
verification source9b6c7e06; natural pure round stopped at first semantic
failure in prepared() query plaintext length0 vs32;3 passed/1 failed/22 unrun;
mutants unrun;source bytes unchanged;root disposition pending. Mechanical
correction batch remains1, historical Sol3/3 and Opus0/2 unchanged.

No local Windows engine/native/VM/hardware test, network/install/admin,
old Lab Manager/protected VM access, push/PR/status/comment/watch/link/merge or
protection action occurred. Hosted-first anchored_preproof_single_voter,
independent source/exact-head review, five normal gates, feature Windows<=660s
and fresh exact-main CI remain required. Main59a665c9 CI44114182696 green609s,
Windows604s is historical root evidence only. M2 open / M3 unchecked;
hardware **unverified on real hardware**.

## Root TLS-driver disposition and targeted correction (2026-11-17)

Root Sol 6.1 medium applied dytschgo-debug. The saved natural red is the failing
loop; no unchanged rerun was performed under the standing stop rule. The exact
case fails at prepared query plaintext0 versus32 before proof-write checks.
Static diagnosis ranked: premature one-direction quiescence; dropped partial
drain suffix; query accepted without queueing. Cached rustls0.23.46 vecbuf
write_to consumes only the successful vectored-write count. The bounded writer
may emit a first chunk while later chunks remain, which the driver must drain.

The driver returned when server output was zero, ignoring client progress. Root
changed ONLY that test helper to require both observed directional writes zero.
Its32-round bound, byte counters, fixed buffers and all assertions remain.
Source correction f0c13d23d975d2d2d6e539d3752f210a7f7d8422; production unchanged.
No temporary logging/instrumentation or additional retry was introduced. The
existing failing setup assertion remains the regression at the actual TLS seam.

One corrected exact-case run passed: test0.027s; build/run46.390s; Cargo exit0,
632 other cases skipped. This supports the premature-driver-return explanation;
no specific runtime TLS chunk size or product defect is claimed. The partial
proof-write/fatal/no-drain assertions were reached and passed unchanged.
Retained plan and target command/results/log: D:/Temp/arx-preproof-driver-disposition.
Targeted log SHA256 a55511ec4b7afc9261ff791ffa209afcda122097a77708eb5773568b5fa2c701.

Root authorizes ONLY the22 previously unrun pure cases next, followed by the
three designated once-only mutants if natural checks pass. Reuse unchanged
anchor3 and corrected target1 without natural replay. Final fmt/Clippy are
justified by the helper edit; no broad engine or repeated passing suite. Any
new natural failure/mutant survival/compiler/unrelated failure stops again.
This is one separate local test-driver correction after the behavior stop,
not a hidden mechanical batch or exhausted Windows engine repair attempt.
No production source or old verification semantics changed. Independent full
source review and hosted engine/gates remain pending; M2open/M3unchecked,
Sol3/3 historicalOpus0/2 unchanged; hardware unverified on real hardware.

## Corrected-driver bounded continuation completed (2026-11-17)

Sol 6.1 high test child, no children; assigned isolated worktree only, prepared
base `18556a70e4c7b6bea2fbc65a60ba58c620c57fb9`. Read AGENTS/KICKOFF, full
preproof brief, normative root R1/R2, independent contract recheck and full
prior evidence including root TLS-driver disposition. Source is original
`9b6c7e060e54d9ec8b44fc12e0715ab92412a3bb` plus ONLY test-helper correction
`f0c13d23d975d2d2d6e539d3752f210a7f7d8422`. Production unchanged.
No source correction was made in this continuation.

### Actual natural evidence and reuse

Ran ONLY the22 previously unrun exact names/filter from
`D:/Temp/arx-preproof-driver-disposition/remaining22.json`:
`cargo nextest run --locked --offline -p arxd --lib --test-threads 1 --retries 0 --fail-fast -E <remaining22 exact-name union>`.
Literal argv/names are retained in natural-command.json below. PASS22/22,
611 skipped, exit0; compile46.36s, nextest0.629s, subprocess48.000s; run ID
`2dcd8f4c-a0f5-4dbf-a6e2-a040f956f98f`.

Actual R1 whole-session admission/fatal/drain PASS0.027s; genuine release and
terminal gate PASS0.025s; gate errors/wrong identity PASS0.034s. Intact wrong
CertificateVerify PASS0.025s includes the assertion that actual possession
verification was reached once. Its synthetic CA/anchored leaf/list are valid;
the resolver actually signs using the other private key. No callback-only or
packet-marker substitution. The exchange consumes and checks query32 first,
then reads proof32 separately after release; successful exchange requires the
server plaintext proof read to have length32. Adversary zero-proof assertions
therefore distinguish query from delivered proof.

Reused without replay: original anchor3 passes from saved red `ad331016`
(3pass/1fail/22unrun), root's corrected exact partial-proof-write/fatal/no-drain
PASS0.027s, root scope/UTF8/frozen-byte disposition, saved inventories,
guard7/sampler60. Thus26 selected natural cases have pass evidence across these
rounds, not a fresh26-case run. Original red remains preserved; root did not
rerun unchanged. The test-driver correction is separate from the earlier one
mechanical build-correction batch and grants no Windows repair credit. Cached
rustls partial vectored writes explain possible remaining client chunks while
server idle; no runtime chunk size or product defect is claimed.

### Once-only counterfactuals and exact restores

Each used `cargo nextest run --locked --offline -p arxd --lib --test-threads 1 --retries 0 --fail-fast -E test(=cluster::pki::preproof_tests::<case>)`.
No engine case selected. Each compiled, ran exactly its designated1 case
(632 skipped), exited100 at preproof_tests.rs:228, and failed ONLY the exact
`server received proof bytes` assertion: left32/right0. M2's dead-code warning
and M3's unused-import warning were mutation consequences, not compile failures.
No fix/retry, surviving mutant or unrelated failure occurred.

| Mutant / exact case suffix | Build / case / subprocess seconds | Patch |
| --- | --- | --- |
| M1 adversary_anchor_digest_zero_proof | 14.18 / 0.030 / 15.266 | Omit only anchor-digest comparison; attacker list contains attacker's endpoint key. |
| M2 adversary_endpoint_key_zero_proof | 14.67 / 0.028 / 15.657 | Omit only voter-key membership; retain cluster check and genuine list/digest. |
| M3 adversary_certificate_verify_zero_proof | 14.76 / 0.028 / 15.766 | Replace only new bootstrap TLS1.3 signature delegate with assertion; retain signature-call counter and existing peer verifier. |

M3's delivered proof with the wrong real CV signer supplies the counterfactual
to the intact possession refusal. Each mutant reached successful exchange and
actual server proof receipt before its zero-proof assertion failed. Kill credit
is limited to these three controls.

Baseline preproof.rs SHA256 (also exact restore hash after EACH mutant):
`cda6d9b06610794f15bf377b22dd726eec7b530acea55fff99f61ba8b944e67e`.
Exact patch files, literal commands, changed-source hashes, log hashes, durations
and per-mutant restores are retained in mutant-results.json and M1/M2/M3.patch.

- M1 mutated SHA256 `dcf17dcc0f95a3d94ce6b46b533eb31a6da73f768ece9df83b08fd18a860ae89`; patch SHA256 `00175374fef81b688d2ff5f7c5a55ea4fdb1e3725cf224b4a5eb5e333fc99d3e`; exact restore PASS.

- M2 mutated SHA256 `b4eaa15b30ca493e4405edf36f39fd5237fd4c818f3a1ec038a8a5233736ace3`; patch SHA256 `bdfe64bb6a77d446636b9e0a442feba187ae3e2bedae382549e236a72b185a56`; exact restore PASS.

- M3 mutated SHA256 `8575558749dad29a59ca2ea4bbb8fa95b898cacde62699d64ea422af55943d0c`; patch SHA256 `c1b6eec2d4f01b6c4af1076b6fd83dee2533bfc0e6dc013b84061193081c6b2e`; exact restore PASS.

All ten allowed source paths matched the prepared head before execution and
after final restore/checks; baseline-hashes.json records their SHA256 values.
No natural case was replayed after restores. Final helper-edit checks:
`cargo fmt --all -- --check` exit0,0.937s;
`cargo clippy --locked --offline --workspace --all-targets -- -D warnings`
exit0,11.813s. No broad passing suite replay.

### Retained artifacts, ownership and readiness

All new artifacts: `D:/Temp/arx-preproof-remaining-cdb73694/`.
Natural log SHA256 `0a1ee92f3c9064a3e7d81b60a282e77564f1a7b05b6bb141644484cf1c3ca5f1`.
Manifest SHA256 `b7e8fd126f6639851cda4d2abfc129e9e0ea8e84d9e3f9eeb3d21046d28cde6b`; pins commands/results, all logs, baseline hashes,
mutant runner, baseline backup and exact patches.

Sequential exclusive reused CARGO_TARGET_DIR:
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-04a56b7c-8088f9385f5b4a14/target/anchored-preproof`.
The prior prepared mutant script was inspected, then copied/adapted externally
with an exact assigned-cwd assertion and this round's successful natural-result
precondition; no root-checkout import/execution. All Python text reads/writes
explicit UTF-8, console summaries ASCII. All owned Python/Cargo/nextest processes
completed. Logs/caches retained; no deletion/install/network/admin/native/VM,
old Lab Manager/protected VM access, push/PR/status/comment/watch/link/merge or
protection change. Only this evidence file is committed; PROGRESS unchanged.

Bounded pure continuation is ready for coordinator integration and independent
full source review. Independent exact-head review, normal five gates, hosted
all-OS proof including first execution of anchored_preproof_single_voter,
feature Windows<=660s and fresh exact-main green remain required. NO local
Windows engine test ran. Fixture28/sample8/founder13/TLS14/atomic issuance1/
newproof1 inventories remain unchanged; no hosted runtime acceptance inferred.
M2 open / M3 unchecked; historical Sol3/3 and Opus0/2 unchanged;
hardware **unverified on real hardware**.
