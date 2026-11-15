# A12 one-run source/disposition preparation

2026-11-15. OpenAI gpt-6.1-sol / high implementation sub-agent; no subagents.
**Inactive candidate only. No dispatch, variable, host or native permission exercised.**
Fresh independent Opus source/content review BEFORE publication remains required.

## Actual identities

- Merged baseline PR137: `fb23b58ed5a333044af746bdfe15afb7c0bec64f`, tree `1b861b4a175c00a63ca7e64ec35b017442d4e7a7`.
- Reviewed PR137 head: `b80f8881bb928aef034a1843b6ea7c78c5303089`, same full tree (locally recomputed).
- Supplied coordinator evidence: Opus PASS6897800328/status58968647772 and all five
  required gates green on PR137; these remote statuses were not queried here.
- Task parent: `ea4c25b3aa7dda50e8ce0e9577268fc856a00a7c`, tree `be23e020122f10b8e29b3ca9091f84b3409fc9c9`.
  Its sole baseline delta is the coordinator PROGRESS checkpoint, preserved unchanged.
- Source candidate commit: `0ff89c84cd1451b3f4d6328341e7c3198c50a884`, tree `ea44d7d01efefec8cf09cba517f371f408deffa7`.
- This docs commit is subsequent; its own hash cannot be embedded in its content.
  Candidate main SHA and final reviewed-head/tree are intentionally unset. The coordinator
  must record actual readbacks after review/checks/merge; no future SHA prediction.

Read AGENTS/KICKOFF/PROGRESS, consumed ADR0019 D7a-k, brief0020 A12, ADR0007
cwd/identity and ADR0020 cleanup contracts, residue/delivery preparation records,
independent Opus source reviews and B1/B2 recheck, and final D7k NO-GO assessment.
Read actual A12 reader/native fixture, driver build/provision/ACK/monitor/cleanup,
Runner validators/run_core/unix workspace_cwd, cgroup kill/release/residue path.
Historical A7/A11 source remains FINAL NO-GO: no fixes, debug, binds, old host,
original policy/package/NNP probe or grant reset. This is the different A12 item.

## Strict eligibility predicate and modes

Against merged baseline `fb23b58ed5a333044af746bdfe15afb7c0bec64f`, compare FULL recursive trees, not just
listed diffs. No deletions, renames, type changes, mode changes, symlinks,
submodules or unlisted additions/modifications. Each changed/added allowed path
must be regular blob mode100644. Existing paths outside this exact list must
have identical blob, type and mode; every non-doc entry must equal the complete
candidate inventory below. Allowed paths (no wildcard):

- `crates/arxd/src/exec/disposable/a12.rs` (100644)
- `tools/unix-exec-validation/driver.py` (100644)
- `tools/unix-exec-validation/test_driver.py` (100644)
- `tools/unix-exec-validation/mutation_check.py` (100644)
- `tools/ci/a7-admission-readonly.py` (100644)
- `docs/adr/0019-unix-native-execution-bridge.md` (100644)
- `docs/evidence/linux-a12-one-run-preparation.md` (100644)
- `docs/reviews/m2-a12-one-run-source-opus-review.md` (100644)
- `docs/reviews/m2-a12-one-run-source-opus-recheck.md` (100644)
- `docs/PROGRESS.md` (100644)

PROGRESS is coordinator-only; this implementation does not edit it. The two exact
review-note paths are reserved for independent Opus review/recheck, not builder
claims. Optional new review paths may remain absent. No other docs exception.
Review corrections need a new exact inventory and fresh review before eligibility.

The strict implementation scope check found exactly five changed source paths,
plus the pre-existing parent PROGRESS delta, and then this ADR/evidence pair.
Driver delta is exactly TEST and its two fixed flags; Rust non-test delta exactly
its fixed flag. Wrapper delta exactly MODULE_SHA256/SIZE. All other runtime,
workflow, package, helper, Cargo/toolchain, rights, A7 code and policies equal baseline.
The final first eligible NORMAL main squash containing D7l must equal the independently
reviewed head's FULL tree and this source inventory. No main advance, substitution,
rebase with unrelated changes or inventory mismatch is accepted.

## Call graph, tests and unchanged oracles

Only fixed Rust DELIVERY_ENABLED=true, Python A12_DELIVERY_ENABLED=true,
A12_EXECUTION_ENABLED=true, TEST=exec::a12::native::disposable_linux_a12.
No source bug fix, framework, production or workflow change. Ordinary Linux
`a12_delivery_closed` is replaced by an injected reader candidate test; it never
calls prepared_delivery. Enabled reader invoked only by the explicitly selected
ignored native fixture. Gate wiring guard extracts actual native body, counts
reader/prepared callers and rejects direct-reader bypass and extra-caller mutants.
Ignored registration is checked. All strict reader/hash/schema/per-call tests remain.
Extracted actual pure gate passes with true and historical false constants and
kills inverted guards in both states, without compiling any native adapter.

Historical A7 mocks explicitly restore their original selection. Historical false
A12 flags are injected, with each singly false and both false poisoning uid/open/
stat/build/call/cgroup/file effects. Enabled CLI guard is exercised with valid fake
environment and effect entry points poisoned; it returns without host action.
All actual-effect test ports are mocked. Existing portable suite temp fixtures are
ordinary D: test directories, not host/proc/sys/account paths. Selected Rust tests
use pure validators, injected reads, source strings or owned mock workers; no actual
Runner, LinuxFs or native fixture. S2b selected test reads repository pins and
mutates bytes in memory. D7j wrapper uses FakeOS ports, never its native entry.

Unchanged native requirements: <=4096 strict root0600/nlink1 record, exact source/
workflow and artifact hash/size/inode bindings, fresh exclusive standard nologin
account, no-follow owned root/workspace, trusted true/sleep, per-call revalidation,
empty cwd under the absolute OwnerConfig.workspace. Real true completion and
sleep15 populated owned leaf proof BEFORE cancel; explicit join; real identity,
LinuxCgroupV2/stop/tree_cleanup_confirmed, cleanup_residue=false, empty runs/tasks
and held-leaf literal ENOENT. Fixture never deletes a leaf to manufacture proof.
Markers ARX_A12_NORMAL_OK, ARX_A12_CANCEL_OK, ARX_A12_COMPLETE_OK exactly once,
strict one-test summary, attributed ACK, successful systemd exit and independent
finally cleanup confirmed. Owned unit/cgroup/live-UID release precedes deletion;
partial/collision/uncertain capture quarantines rather than adopts or guesses.

Bounds unchanged: Runner15/proof5/call25/fixture90/preparation15/ACK5 seconds;
packaged startup30/monitor120/runtime140 seconds; output1024, memory64MiB/pids32,
DEBUG=0 A12 build and 128MiB harness cap. Full per-call hashing cost is unmeasured
on Linux. Synchronous NSS/filesystem stalls and scoped joins have no hard OS
interruption guarantee; late results refuse, workers remain owned. No raised caps.

## Proposed execution and predictions (not exercised)

D7l in ADR0019 is normative. First eligible main squash only AFTER fresh independent
Opus source/content + exact-head review, all five greens, coordinator merge and
actual main/source/workflow/full-tree equality. Independently confirm native,
D7h SUDO_PACKAGE_REVIEWED_SHA and D7j A7_ADMISSION_REVIEWED_SHA absent. Coordinator
sets UNIX_EXECUTION_REVIEWED_SHA ONCE to exact main readback, verifies equality;
other two stay absent. One unchanged unix-execution-experiment.yml dispatch, main,
no inputs, fresh ubuntu-24.04, attempt1, A12 only. Direct returned URL bound through
terminal; no early clearing, no ancillary-discovery redispatch. Finally clear native
pin and independently verify ALL THREE absent HTTP404. No actions performed here.

An accepted dispatch consumes the attempt including skip/failure/unknown; confirmed
rejection consumes too. No retry/new grant by this task. Exclude host plus every
D7a-k and older experiment environment. Any failure stops with closed sanitized
record and bounded source assessment; any security finding escalates. Mandatory
independent post-run assessment BEFORE credit. No old-A7 source/runtime permission,
A7/sudo/NNP/policy-view credit, real-Runner PA6 or M2 completion follows.

Predictions: true Exited/exit0; sleep Cancelled after real populated-leaf proof;
correct UID/GID/root attribution, confirmed tree cleanup, residue=false, empty runs,
held-leaf literal ENOENT, all three markers once, exact summary, ACK/systemd success,
final cleanup confirmed. These are source feasibility predictions, NOT acceptance.
Everything native remains **unverified on real hardware**. Current candidate needs
hosted Linux compilation checks even though PR137's closed reader compiled there.

## Checks actually run (Windows; no native calls)

All TEMP/TMP under D:/Code/arxcovo-a12-one-run/tmp. Cargo outputs reuse
D:/Code/arxcovo-a12-delivery-source/target; extracted Rust artifacts/scripts under
D:/Code/arxcovo-a12-one-run. Rust1.95.0, Cargo locked/offline, no installs/cache deletion.

- cargo test --locked --offline -p arxd --lib exec::a12::: 7 passed,409 filtered,0.04s.
- Exact exec::residue_tests::cleanup_source_pins_release_sites_and_legacy_errno_order:
  1 passed,415 filtered. exec::stage_b_tests::w8_e7:2 passed,414 filtered.
- python -I -B -m unittest discover -s tools/unix-exec-validation -p 'test_*.py':
  196 passed,1.116s on final source after docstring relocation.
- mutation_check.a12_assertion_mutants():9/9 killed entirely in memory, including
  AND-to-OR closed-flag bypass, disabled candidate defaults, DEBUG0 removal,
  account bounds, pending capture, live UID, build clock and duplicate artifact.
- Extracted actual closed_read Rust (pure only): true/false positive cases pass;
  inverted-guard mutants killed in each state (2/2). Actual Rust wiring test also
  rejects direct-reader bypass and extra-reader-caller mutations.
- python -I -B tools/ci/test-a7-admission-readonly.py:13 passed; exact S2b
  test_pinned_manifest_applicability_in_memory_only:1 passed,0.010s final.
- cargo fmt --all -- --check:passed. cargo clippy --workspace --all-targets
  --locked --offline -- -D warnings:passed,3.24s final. git diff --check:passed.

Initial adaptation check failed on historical AdmissionPathRefusal using the new
selection and guard returning None rather than integer0; corrected only test setup/
expectation. Initial fmt needed closure expansion. No runtime/source fixes. No
unresolved repeated three-fix failure. No full Rust suite, actual Linux compile,
Linux harness size/performance or native acceptance claim. No actual Runner/LinuxFs,
native ignored fixture, host/proc/sys/root/sudo/account/cgroup/helper/network action,
dispatch, variable, push, PR, merge or watch. Fresh Opus review remains coordinator
work BEFORE publication; this sub-agent creates no delegates or review claims.

Final static scope/pin audit passed: exact driver three-literal delta, wrapper
hash/size-only delta, Rust native/shared code equality after removing the fixed
flag and ordinary injected-test replacement; 223 inventory rows and digest,
regular100644 modes, no unlisted paths and no PROGRESS edit from task parent.
This audit used Git/repository bytes only. The evidence/ADR link targets exist;
review-note paths above are deliberately pending, not claimed review artifacts.

## Complete non-doc inventory and canonical pins

Baseline entries: 223. Candidate entries: 223. This is the historical
222-entry inventory plus the subsequently merged A12 source file, with no omitted
non-doc paths. Canonical digest input: UTF-8 rows sorted by exact path, each
`mode\tgit_blob\tpath\n` (LF including final newline); excludes only docs/.
Baseline SHA256: `84f8446e059f5e64a1aa7957763a7bb171c43de967410dcd23811ddaba2b9d05`.
Candidate SHA256: `dbc11a5ad3a0e37d6bf659b1e1bac65c2758565d8a11cf5c500bb415e375c710`.
Per-file SHA256/size below hash canonical committed blob bytes (LF for text), not
platform checkout newline conversion. Binary blobs are hashed unchanged.

| Path | Mode | Git blob | SHA256 | Bytes |
| --- | --- | --- | --- | ---: |
| .gitattributes | 100644 | d0d6371de329ffb232ba2fb9b79732165a702be7 | 23302dd90b8bb74b44d365fafef8df20f809b61cb347023951ecf60a88a29651 | 32 |
| .github/agent-authors.json | 100644 | de7a8c79940b870926acec71eeeb7707e2f09886 | 407f082c15a5366dc7c307a64f59edc05f38696488318b0af46bcc19941a4887 | 17 |
| .github/main-protection.json | 100644 | 575540057aee74d979b86ff6c086bf8ae465dd09 | 33e0f700d5e0114fc6c6624a101bec8e2f2a9c1e48eb9b9f9747c90b38f39f3e | 766 |
| .github/workflows/a7-admission-readonly.yml | 100644 | 5568feb19d9391ace8c6434909d4f19e9a4e36e8 | 7878e894b9487183dd8d9cdc4788b0ee73ad100b75b5410617b22ab23b5bbc33 | 2342 |
| .github/workflows/ci.yml | 100644 | 8fe814e48a7cc5f88561c64da8eb9c0b19877d33 | 07b8c8c350cb28e7f60ed293e4565d1f250d0f2c2bd2604bec35e6dda6d6df72 | 3701 |
| .github/workflows/e3-native-evidence.yml | 100644 | 30167418c0993d83d8d447f5f21ab3f57dd94835 | 7c7850d57ad6db22a8fd48f74396a4790b4eaa5d09cc853e19f025a725aa2e66 | 1252 |
| .github/workflows/e4-native-evidence.yml | 100644 | 2be8e3226b0b07ce9791bbf2f63bc51863c52d3a | 48122e681f227be7e7b2b6acc091c10782dd8685e9d71c81ffd4dcbb377834f8 | 1289 |
| .github/workflows/policy-signal.yml | 100644 | 1898f78e1f4f529ab575af6d5a1db3961631b6ea | e315fb9bd00e65a9e7942785cb147f33248990e6ee74b43672f60e5120fa21fd | 222 |
| .github/workflows/policy.yml | 100644 | 4925b7f12bdebcd624024e64af5ce9e000743b67 | efc81774b68f059cc0bbb281e6719997b61a7a38b94064dd07098b0fd1fa56b5 | 806 |
| .github/workflows/s2b-native-evidence.yml | 100644 | 8b01486932f9eb7346a21f3bbf1c5aa2844c4daf | 3097257cbc463e1b85a4845fc529f3df8af5c0db768db00d1b3a259aa81ad418 | 1564 |
| .github/workflows/sudo-package-readonly.yml | 100644 | fb4f0a5e28304e8dfc61cd3770c2ddff4f462ea3 | fe7e423573e43299c794f58c3dff0a9b9cbc69512654a1c973a32c90e84f94a1 | 1929 |
| .github/workflows/unix-execution-experiment.yml | 100644 | dbcca2276307357301719ed68a09631b8ab7a10a | 7b5370f46179ed8709d8ce7946968c735194ade55b3eb893e3c43c33c6a3efd3 | 2232 |
| .github/workflows/windows-identity-probe.yml | 100644 | 30027d353dc7e7dd99038b7dc88ed3786b2d2aa2 | 71bfab0f03f316fc33fe0487dd639ff44347207fba1eb9142ac933ef4ff2075b | 2062 |
| .gitignore | 100644 | 3c554ae03483f7a5d0e772215db0c29dc38d671e | 9b51697f998f26ee13481d524f025b6ca95991848cafd969fa4a513b4ffa60ae | 75 |
| .node-version | 100644 | ca5c350055ccb6dacb495a786f60c8232abed8f2 | 55075b5ec4e8b31936cbbc282b8829116d1fd48f2f2f1856dee592a6650700ce | 8 |
| .npmrc | 100644 | 145d3fa25b8c8cdd495ab40849109408792fee0d | cde76ca04e048c3eba070a2c0dfcf5b723cf6014eab177157096b924f24ec4ca | 35 |
| AGENTS.md | 100644 | c2ae25947916672b13180778265354907b75e295 | b8c1f8c0c126d8b9ce69fd7182109a68d907cdafd865921d1f40de8d8742cf66 | 1974 |
| CONTRIBUTING.md | 100644 | 483614be7811d25ae6cac786711a3f40f669f605 | 8e6ad855a9e8e36663dd61a966d4efa352cbc066c05a2ba9b7c6eb766efb7fdc | 480 |
| Cargo.lock | 100644 | 1c5c0fcf2b577f89ed998826ddf6608338b4b523 | 1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc | 51052 |
| Cargo.toml | 100644 | 343f7301c8744ba3ee1b8ebfb5422c70a04479f9 | a71354fd4e463f4ef6c3f02ca3a76ce03902c01f4d497f9863fa55b12f0cec2b | 177 |
| LICENSE | 100644 | a028880c724b3ad2c8ec7a867dc9ce3bb2539c2b | 0531b39424321b061df4627ba1528e69674cfd2df381fc54e9afa63d3bdbec98 | 34503 |
| README.md | 100644 | c338651dd7ba58c4344677f866fbe22039306286 | 1cfa336c11887f9fcaa6d17616174fdd661732f3654b457afed8703a33e91f89 | 1106 |
| SECURITY.md | 100644 | c100d23cd621428fa205584da64974c642c281d7 | 60d5449ebf0ad5ac21fb15987347926f97f1e5d7e3d966334d3266b29c606e3b | 619 |
| crates/arx-guest/Cargo.toml | 100644 | 3ad7c587de0344adc7b401f2f1d36cca022d9309 | 1eeab677ba6823c151f5fffc8f139e4eb673abb87e43544b714c5f6b523fdec9 | 134 |
| crates/arx-guest/src/main.rs | 100644 | fc1bbf91cb5052277a0f3752db4b89b328008e00 | 8fd6ae0205a48940668e0ec8f5b15b63df489e67351615b1f0bdb4b28a7c173b | 86 |
| crates/arx-guest/tests/cli.rs | 100644 | 7d5212c545400f926a8fd07d49786deaaf063f04 | 51386e0cc868fe5630ae7c05cb6afe8731f40ba592470e152707ad388d3f2470 | 949 |
| crates/arxd/Cargo.toml | 100644 | 055372c98d6e3f42a87a30725145cded1d7a3531 | 2d58fa3aa535df5dad88dd313e1f2915345f2ba285983ecff6e9104e0ae9d25b | 1551 |
| crates/arxd/README.md | 100644 | 0d3859e6a5e9011217819c6a8efd7d00a512c940 | d9ca313496f0de2fb7044e68ddeb814a6afb04db2a30e1089a643efa41a60f0f | 18866 |
| crates/arxd/src/client_config.rs | 100644 | 87e3d6a50fc68cbecc6428cfc075bb4f708f0842 | e0acffe09e606dc05a1a06e1231d08af52d8210cb06930a898f75a1985ebcc8c | 6212 |
| crates/arxd/src/client_config/tests.rs | 100644 | dc83fd3fde74fe355d0709e44d4dd901914bdd52 | 7eaaebf0da7108eb4876e1353ec5fb964331c0d1536e8b7c6e68ed09519ad140 | 35477 |
| crates/arxd/src/client_config/unix.rs | 100644 | fdc351273e641cf416591895bd50e3155581ddb7 | 1809fa97b5f0d0c83c5fb001819ab1e2bf56b9a3b955ee9ee69eaf61841cfc8d | 6211 |
| crates/arxd/src/client_config/windows.rs | 100644 | 4e53527cf95e20c4602ae88c5dc66fd38c30abd2 | 85aac020e5e5343c5b269fa47a6488a4b3d2f88bcb4c5d19a0427876f6f582d5 | 16592 |
| crates/arxd/src/config.rs | 100644 | 3ed5899d35cd6030e80a0be9274197834a0dee5c | 91363632998f5b3efb2fa13416e9639558a82ca2437495481acb437a19df919c | 6705 |
| crates/arxd/src/development/mod.rs | 100644 | 03be8eea75463f52e4cd4f3855921a69ed7a68c6 | f35f9304e6d7a756b854f9007b3fea27c61c5394495dc7811a2ca416aa1b719d | 18732 |
| crates/arxd/src/development/tests.rs | 100644 | f6962c26355fb817dc24bfd9e684f3b030f1ce92 | e84998ec751fc5ce8860fa9b7ce002e066e1a9fd3fb09515ac0b2b8f79b47753 | 14631 |
| crates/arxd/src/development/types.rs | 100644 | 754070e807a69f7e0f20de0a8813f2aa55e469e6 | 95b40fc8ea036e235360124c5f3bacf03a8c9ead925ef6f9c48ef3500ba8c111 | 940 |
| crates/arxd/src/development/windows.rs | 100644 | 491b1cfe9b2b23a6d1b814b51e8a6853f011b7f1 | cf3bc19a033327d8b4093f3823dccc576b8775d04e2cfbd7b6187e08e7e1cb89 | 114694 |
| crates/arxd/src/exec/cgroup.rs | 100644 | b01bd021d3bb1529cd50a93049cb4e9d17e73453 | 64d4cb8c48e516d5fdcd92f85fe2c9c9abb028ee55251929e95824c16211d121 | 10259 |
| crates/arxd/src/exec/disposable.rs | 100644 | 252a60449e1217a055129c4aed9d1ed7050912b5 | 2f836d67208c8945c29744f4d8d6ca64f4fb8156267d2ad2cc7f47b83c914708 | 19788 |
| crates/arxd/src/exec/disposable/a12.rs | 100644 | 50f8b15e00fddf4950cc8aaec7e88dbb2ea183a7 | d013932cc927104272072f4266a92924199e084fa5204f15c844828a35f35566 | 42028 |
| crates/arxd/src/exec/disposable/a4.rs | 100644 | b17a961996d0e8184da0262ea21e20d5773d484d | 9ea4d4b3203627a19e254318710bb7e2d27a35ea0133f9e8605392977aad2292 | 16055 |
| crates/arxd/src/exec/disposable/sudo_nnp.rs | 100644 | 6a3031a579d9fb1d1f499a3b4639c216e496d14f | 46b0de072f4e7fb8feb47e50ef670ac3d7678a4266baa9b991f4ce73693f31d8 | 64906 |
| crates/arxd/src/exec/hierarchy.rs | 100644 | e47ced34b2f516a59bfc662949040d5d65e6d2a6 | 895625acb659bb4a55e6a94c462aa46e2580fda038adbd33f4b8bf51675c2af7 | 321 |
| crates/arxd/src/exec/hierarchy/a4.rs | 100644 | 1c7192626056ea4aac5f63f6fea3c71ee9d408a3 | aebca7c098384f6ffac71662089a1bbc6fca07fca9b054e4c386da497fc8d81f | 24212 |
| crates/arxd/src/exec/hierarchy/a5.rs | 100644 | 71f064ccfb78ff06c75a180a61974d57552ea102 | 7335cbb3489172da4333f1fb50a6fb71d8cbf5319cf300bf813d65fb58f4b4b4 | 12299 |
| crates/arxd/src/exec/hierarchy/fake.rs | 100644 | 86f4e509814cae28618c84dfc65ec8e3a97b3b6a | da0e86bdf9b4c15b67edeac0470a0941d1daf762406efe1203dc4c4637e2086b | 37845 |
| crates/arxd/src/exec/hierarchy/linux.rs | 100644 | 7c4196333f4938499188c7f99e5476c955b8e9d0 | 59acc5ff560910f66564655008d6447cff3646864f6fd43338a507f972de2f1e | 29474 |
| crates/arxd/src/exec/hierarchy/parse.rs | 100644 | 145a44d4912fc28c39b0832d7decb331b64b465e | eb3153108c42625a282f5e746965a840d2b03ac745cc4688c3f2b5ca97838058 | 23362 |
| crates/arxd/src/exec/hierarchy/setup.rs | 100644 | b33bed7cd5d7022f30c039e761c544567304d28c | ab04270eb077ef8fe5c4ae6accc7d053b926a0465aa75fa31ed626f47347c179 | 10040 |
| crates/arxd/src/exec/mod.rs | 100644 | b150f543a894d57b25c56bab19c95b4f9bd2beeb | 1f23fe1a7d03c2a8a455426cafcfbd289ab6bf334bff290620dbd024e44896ad | 11693 |
| crates/arxd/src/exec/output.rs | 100644 | 1c383aafb66da08693850fe07878737eadea3bf0 | 0853bff866308a5fc20c4b517be0e158c5e12c85a5a063fc428211ab6d24202f | 1083 |
| crates/arxd/src/exec/residue_tests.rs | 100644 | 91c52516fba2b4b1740309c7b986759e6aa60fc4 | 8f3af42826c53896aa43282df75aecba6c835fb404bed35c90896495fecb8af8 | 4048 |
| crates/arxd/src/exec/stage_b_tests.rs | 100644 | ff20039925c40835a02a05147e5e10656bcbe0a1 | 756354c67e9264d998087f99f710700b36ec7b71cd22b1d74c8bba58b35cf5d6 | 33215 |
| crates/arxd/src/exec/tests.rs | 100644 | c2b413036247da906d61d5691249a2a2173a2647 | 88225cab3317cd8e99c83e9f9fe4c14c8473761839fb8ed5a58c0aab692aa185 | 5234 |
| crates/arxd/src/exec/unix.rs | 100644 | 0d087e37f31f3e17db652a87f2a0364eb398479e | 500a914a1f6c6b16de516a9c9df332a47ab89f17769b6a48cd5f06dcb70af0b7 | 21768 |
| crates/arxd/src/http.rs | 100644 | 09ffc537c198eaa47063eaa41b1d8d1d0b039be1 | 6362041d22cdaf008661fd988ec77b91aefebda1f9b1f6491c2cb7f0c5b764fe | 3346 |
| crates/arxd/src/lib.rs | 100644 | d7e1917e8f331ae3d2277b10d617d492e72fbeac | d1c15d300a6f5d34ffab4af2904d1959e501cb4bcfa908dfc8ded6ec26408773 | 277 |
| crates/arxd/src/local.rs | 100644 | 72feb0b22335c674848c6fc9742ebf776fe4140b | 403c29c34922178c341df8bef26f59670ab2287bcf24a54d9ff5a1e595cccf11 | 20866 |
| crates/arxd/src/local_execution_tests.rs | 100644 | 0d23f272d20f516a5269e5b08b09e4f0bb229f12 | b99db94a55eeaec06bdeb016e9da5feb10d8b5769fc04f2a588c01ae69cc2540 | 27786 |
| crates/arxd/src/local_unix.rs | 100644 | 44bdb0be221e5e3220837e65a5e0fc89df529b7b | 4b4d56206702ffd362de47851bcae2ea3adcaf7e8d216d24658b1b9e0546e4fa | 5481 |
| crates/arxd/src/local_windows.rs | 100644 | b685e90a502896f87121d741622ab563a3014ef1 | 67ab74b626b43eebbcfb4c3260d90084323f882cba7a31cc45f2ee00d8eabf18 | 9662 |
| crates/arxd/src/main.rs | 100644 | cae709bba9d106f0494fec656b356edfe7602634 | e838f9bef12f8df2236a454b70eebd6970323b8a13394cf22a696ef059ee79ab | 7063 |
| crates/arxd/src/mcp.rs | 100644 | f83acbc8b5da33577296a797c2dbfcefd18f396f | ff25ffbd4a390ff560821c337c4f178c8b826a0f32ea8a9083b3f7595352f1cc | 14977 |
| crates/arxd/src/mcp/activity_tests.rs | 100644 | f92b7a2f37c59627faee2704a99004cc5fcc1e48 | a4ed7cb9a72908473c3589b3f09be8250eba2d1b77dc54d1161ce93082b2b43a | 15097 |
| crates/arxd/src/mcp/discovery_tests.rs | 100644 | ccf3e75733aba9ab07e25ac8032f152693be8f21 | 36741a7e665acd773427fe6acd61b19de39822cd23107f370a4c70396d1c0b03 | 15848 |
| crates/arxd/src/security.rs | 100644 | 4c911a1ad59deff98f029dccd710deb31b0a6523 | 687b318dfecfe979b206ff65eca18b9508fd7d3070aca9d2e40c9710183d81d4 | 3494 |
| crates/arxd/src/service/application.rs | 100644 | 7976171bd2a67d194770e71f33b3f0a347e94a18 | 683345d29a3e48ea17552ff1800a4d0962eaa55281265604610d75a8abce0dd1 | 14333 |
| crates/arxd/src/service/application/tests.rs | 100644 | 541a8230b1b590a20bb338fa3ff37e8380ed0401 | 438e1f17366084a697bd1ca514dfccd8c2425a3f0a508fd732ef2a53bc56e332 | 32998 |
| crates/arxd/src/service/config.rs | 100644 | d396250f3cf78ffda14929e2a39102a769b504a0 | e5c0df0fa5bad1b85b81c8a360e7fad1166c71a1aa55f0c72f0a0350beab7f03 | 45236 |
| crates/arxd/src/service/executor.rs | 100644 | 648302034a22f5a49e76c9b198e30617f306f20b | 45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed | 32757 |
| crates/arxd/src/service/executor/tests.rs | 100644 | 168d86fe25ec0a729d0c40a8523331e7b69c782b | 7edc866b9846ffb63025f4d2196ed9847ee9c0dcde46f09831c746bca9771581 | 73576 |
| crates/arxd/src/service/layout.rs | 100644 | b636acea3df84e2c717ae7df3586486ffc1dfe91 | 53bc27813d697611d9b70566f1fe9642c40896feb606defb23a80c707ca96a5c | 21953 |
| crates/arxd/src/service/mod.rs | 100644 | c0488d356ca7b133d7c056c2273da2af2517a824 | 848ab03cd7a68e6946a21aa9750bc103111b50a9d8d3957d2fda68fda1c79530 | 8278 |
| crates/arxd/src/service/native.rs | 100644 | 3f9264f670cbf9b53c1e7212ae3d181a4fca4708 | 7631b2f4b1faf221882c0857f60c90b0412758b3323569a6bd1d9de2dfdee4f8 | 24188 |
| crates/arxd/src/service/native_macos.rs | 100644 | acfd3c11e285440604bd6fdcbb3f8278b13dc7b0 | 59d03b851f8c00196ce8012a999147129961fd56b4129a07a327119753602864 | 13709 |
| crates/arxd/src/service/native_macos_policy.rs | 100644 | 372a62949fe618138b599152ce300939a0f6c346 | fcf6926d29d862025638d8c32be736b481329814cdd8807dc972541c4a5dc5c8 | 5596 |
| crates/arxd/src/service/native_unix.rs | 100644 | fb1bb275b4e9f28e2f49648a10a43725347a9a51 | c7734674d3e8605217b1596cc2096837c6a068fb4b08b8ec692ef056506a15fe | 13220 |
| crates/arxd/src/service/native_windows.rs | 100644 | 2f4b6fe21a8d0dc5676427b263c8614f31cbdb40 | b556ce8cc919944c8daf00c9875e94995549e46603e804ccdf414a4c8d829761 | 25085 |
| crates/arxd/src/service/owner_cli.rs | 100644 | 933330993d4176d123fd5d87d6219e24cb342a27 | 1723d3206d9005fb65a1ffc7f556ec37d0637e6402525de6e7c5639af20cb962 | 15239 |
| crates/arxd/src/service/owner_cli/tests.rs | 100644 | 6627ffdc89693ca467d6e2ec04dc4c432d701585 | 40422968bd205752de7171efb35653cc623810fd92ea69306977db9dfa422971 | 28544 |
| crates/arxd/src/service/owner_install_windows.rs | 100644 | d1956de63f80a9a2138128e2d9de19f6d4c33fe1 | 6b7711a1d835503d7517bb80abb60ab041ba9520dc17969912b3e7828ca93df5 | 9584 |
| crates/arxd/src/service/owner_install_windows_tests.rs | 100644 | 6334aac5b9d86be697ca32d2392b08820809ebb3 | 80e0af1bf73f24aae6aedc82638f53e40a5c8ce8e777edc6818470622c240e63 | 28645 |
| crates/arxd/src/service/owner_launchd_capture.rs | 100644 | af209f870bd9c63110a74ef8b677342aee3c5de9 | 16e70bff7e5580b1774354a57c21a13d1ec772fe3610cff36380ae649273a5ea | 13235 |
| crates/arxd/src/service/owner_plan.rs | 100644 | 32a36454578553d7f8256be98e837bdb3f8885a1 | 588e0b4a393105b53206b3cc19c84bce9095e75b8a3ebc673ac4b0c87a25f75c | 37314 |
| crates/arxd/src/service/owner_query.rs | 100644 | 20c51c17ba24e5d3b5cecfad2af5d2b662445888 | b9afb2b10f2c4737dac0908289597c31b63f7627a5a1c48511225740a3cdae5e | 18701 |
| crates/arxd/src/service/owner_scm.rs | 100644 | 6254f1dd7fb03682b904315a76c064624458067f | 277696c1aa15264654bc6f77ef7ac27222eceeec2f302b247b007345b3987ed1 | 14603 |
| crates/arxd/src/service/owner_scm_tests.rs | 100644 | f9a3b747bce4bf17505d2c34c61e9aea8dd4ae9c | 5cec6c74ae2c152e0cfca578004f5cf301fb5a201a056ee018a4f96dc5fe8603 | 26544 |
| crates/arxd/src/service/owner_scm_windows.rs | 100644 | f006648ade2270b95dd0e5d0983c8e446bc7b431 | f376532b068d9d6d53c696a27dd906a421bfca0ab18672ad2df13dd32a7d5f01 | 15561 |
| crates/arxd/src/service/owner_scm_windows_tests.rs | 100644 | cb7e5de582436d44e8015b2b3ea92732b1cd212f | 1447ec52e36644a2e79466cd2e31d93e591bf081695ea1d9631e123a4299f64e | 6293 |
| crates/arxd/src/service/owner_transactions.rs | 100644 | 433b70381753569661aadc9c06ae1c653415e9df | b997aee1cf480b92454c378f2e9e9eb83e71854132d08741a7a505e6e7149d93 | 11602 |
| crates/arxd/src/service/owner_transactions_tests.rs | 100644 | 0f3fd140bcdda0891f0ea2e331def17609cb81c7 | 73984143b77cc4c086b74c1a84e0a002e8d0a733602d2c9f9a00db34c74ad303 | 37979 |
| crates/arxd/src/service/owner_unix.rs | 100644 | bd991dd11c75f079bd3f689ec936baf8f3371084 | 17d2a2c892d4885c291f4ca8fa2377807ab28b42408e0f76cede715fa54ad8e7 | 82543 |
| crates/arxd/src/service/owner_windows.rs | 100644 | 4c89e9af853190f92c1f9a18bcbf80f8237fa25b | 399ac58e1952f20d95fea09f7174eb29f7757148f15f1b70dacb6bbfe3560bf5 | 30197 |
| crates/arxd/src/service/owner_windows_tests.rs | 100644 | 5fd38201a80a766c4779391f7aec70ce4bc190f7 | fd1abfd7ad07e57b4b52ccdae47799250e788a9625f58cef091a80647bffd1b7 | 40690 |
| crates/arxd/src/service/preflight.rs | 100644 | 534625eeba3569119f1c4e49fc1ef35f1ca3f072 | d1ccb342bcc2303a11665cddba1591a974507d1cb776a659896c0c26a20228ad | 2387 |
| crates/arxd/src/service/tests.rs | 100644 | 0b9022e8f809fa6b298564b6bd713b97394e556e | 3696655a97f3f5ecfc2e93fdf4b895e5fe47226d2c09b36fa82fdf9105423995 | 14876 |
| crates/arxd/src/service/unix.rs | 100644 | 89f2a922d2f5c5c630f07fb77c34bae062617578 | 8d549618fb77932d0f80b15a0d0170946a9852423335599f9bd6199004e9de33 | 5783 |
| crates/arxd/src/service/unix_bridge.rs | 100644 | 01c626dddbbb43ac43f4d332a79c15dcc1bc02d6 | 323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1 | 15508 |
| crates/arxd/src/service/unix_bridge/readiness.rs | 100644 | 1ff55b22f810e1907e21a2edc9c9df07be868f92 | b9c8e297873233f52d3655f03d8543c663a81ff2a19cffdbaa112233bd0935ad | 1976 |
| crates/arxd/src/service/unix_bridge/tests.rs | 100644 | f5136186085e03c4225249b569e99debdad6aaa9 | e500e65fcfd42f30d0492bced7ac6911de9e0dc6619e50e87e82cc94aced7eae | 35002 |
| crates/arxd/src/service/unix_bridge/tests/a6.rs | 100644 | 781b3b36577794c60a12175e2e843296d9e3ece8 | 3a33d044b5e522fba7de2e10b8ce60d95eba94e330765448c3cb443b898d14e7 | 11297 |
| crates/arxd/src/service/unix_bridge/tests/e6.rs | 100644 | 7514f353217fe015974228af9d92fa07cb249095 | 05e30be5a57921492bd6289f23cd3618fb56270d89a78947cab18779516c05bb | 27236 |
| crates/arxd/src/service/unix_bridge/tests/e7-no-cache-golden.txt | 100644 | 1263f1c1bd5752c6ba981d928e4bfd36eb14919c | 592d8346a7e7f814fefdfd90cdba049ac30271e36c25d8e2873078e3c41d88f2 | 1548 |
| crates/arxd/src/service/unix_bridge/tests/e7.rs | 100644 | 0f3df557475c2a496b47de3dffe8527303cc45bf | 059aec258c31715e86a6a9a810c17615bd7d26f2598f4dd3fe4e8c2ed0455c3c | 41583 |
| crates/arxd/src/service/unix_bridge/tests/s2b.rs | 100644 | 7e098e3dc8f9f8141c8e7f27b489eeb9731d2e8b | 6eb93098dfda74ae52e751a07f69fd9252417184d65d6064cb6a26270643e089 | 8965 |
| crates/arxd/src/service/windows.rs | 100644 | 8059646740cb4b3479c22420e750a5daaa98e355 | 000282b28dd4d5e01c08adf22e77e0d66c9d5e4a6367c21f546da16c194cd628 | 14412 |
| crates/arxd/src/store.rs | 100644 | f223e99c6bdb57569557e384bec828cf5c40a3ab | b843ae9b55b797c84d1e1439f410e7fb70b0bd94513b63aa31bc2585275b3946 | 38489 |
| crates/arxd/src/store/crash_tests.rs | 100644 | 84531963eeae9e8b31dc8f0c83220f852e640783 | 9e99e0d7a92d24b8f17ff6232aaaba029edd935829a3c1628522d4259c34ed34 | 7644 |
| crates/arxd/src/unix_shim.rs | 100644 | e70d0881964fdfc18f8a51b4b51875e569b374b1 | 5e7a01094733e882b1ac67376cba5e24cbb1ad1db771d06a944806b747361fd4 | 9045 |
| crates/arxd/src/unix_shim_rights.rs | 100644 | b269e00622d48a2bb7dfd8143e1cdec7f25b5177 | 17310f10a8035d2a732c7b2e87a3be4dae1d4caabdee8dade8c6cea8a7017928 | 2909 |
| crates/arxd/src/unix_shim_rights_linux.rs | 100644 | daa2144283fea0d2b4bd859c796fb86a827b631d | 3fc3e949dc0725d22d466c12f1ef3fa13d395fea586a84546c8cb4e4be8ceef4 | 14519 |
| crates/arxd/src/unix_shim_rights_linux_tests.rs | 100644 | d02fd13090d0e778100a6fd9a8a8da28b5b60e92 | 672bc7e57bd6251b5e4db4a51e78de5f930b962ab9b3034774b128b4cacb7048 | 14726 |
| crates/arxd/src/unix_shim_tests.rs | 100644 | d8d879e795965650ec7a38fee896d0b45c2b8dfb | e9d9b79d966096adfeec9a3ba54b8c0769f44b8b585a652ce9c7497dc4ee64b1 | 1516 |
| crates/arxd/tests/cli.rs | 100644 | f65a1b502295d7d97adb9d2714d7fc1f2ababf95 | 7dafe8c4860b3837887d287d5668c0ea457cb03fed4454da72fb3a75e8e73298 | 1905 |
| crates/arxd/tests/inventory.rs | 100644 | e92ef2669e6777cede83859d383190c59aef421d | 45a2ce588505d454623bbd3594aac860f1c3b21992cd34da48a005f7640d6ce6 | 53630 |
| crates/arxd/tests/inventory/activity_v2.rs | 100644 | 40a3c40fd8879a415b0aade734cb7f2ca8312bca | ebe21ceae753ab88194828f74c110f775ac41a70e777c28ee890117c8bbb65f2 | 26531 |
| crates/arxd/tests/service.rs | 100644 | 4471b3a63696fa16b0957b545bc7d2b5fcbf2063 | 91bf603533b4219c89e0b12216efc796399afb4eb5a82fb0c59b5871a00b776e | 1713 |
| crates/arxd/tests/unix_shim.rs | 100644 | 4108b5c47238e5ef9be9e6c23dad1351cda90c9f | 20d0f61736d7d57c973ad19a9b027fa7d38bd05e45857c3dcee199570d905b7a | 9666 |
| desktop/main.cjs | 100644 | d9c9c78153f07acdb18cdca3b74e62fa2f99c5c5 | 3b1e0aa557f7b4a799d6867b170dea0c753b05b59a1cf33650fa16609fe8b21a | 504 |
| desktop/package.json | 100644 | 2974885dd37ed0e2cfe328fac2e57c9a293dcf06 | d18a76acd45ce6cda2db704771af514a32643574d6580f6fde80d0bf5fa0124f | 195 |
| desktop/playwright.config.ts | 100644 | 5b1dff857e4931d1d1ffb0bea6ed5b2bbcaa97ab | 3f03662d3c56f91cbc63794e372be0cc5ee321a3038ce7f21dcdd6818edc9e5d | 132 |
| desktop/tests/sandbox.spec.ts | 100644 | c612e12d02eeb4ff8b1c9c9c7a7037080b2f72f2 | 5630c4522df055c248d1a0d9ea7cce2ef69a824316394c35eea3530f9531a598 | 1593 |
| eslint.config.mjs | 100644 | 5e17b60985b1efc8ad6598722e040b390d7b105c | e18b3bfbb6c142c9ba9bc4188acb0ea8e5f24c1d2e0a8f6b1926f2f6fe6dabd1 | 526 |
| package-lock.json | 100644 | 8e9cff2aa19a35fe08264ad6d0c0e48f32b88c99 | 4f9f0c1a6c3c33b63207f6593121dd1a0bb9db047ec693cae597ddee71890d96 | 93654 |
| package.json | 100644 | ed4f1214dcfced8d7814028350de959a7f4cdbe8 | ed9d07cd6658d0c59898c1189e4be772934337735c7551e28538a43def078d0f | 743 |
| packaging/README.md | 100644 | fc4812328f8d0e6ebc49d50162cfa873fc30e297 | 4211b0940c27c78b0bf118266e2d117e745377f9cf3598b2fc5cdb48b623ccd7 | 1215 |
| packaging/linux/50-arxcovo-standard-rights.conf | 100644 | 93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74 | 3afb57ea49b5004d9e83293ad424ca1542a4d732d8642ccddf26e2668f49d67a | 134 |
| packaging/linux/arxd.service | 100644 | 0b337773c60e49fe84de76db018d2235ae076254 | b49e229789c6bc46393bf51dfb64894ee7dfa5d234981a1531b42d9e84adb12d | 453 |
| packaging/macos/com.arxcovo.arxd.plist | 100644 | cbb41a5785ca2a0261539a2cf8d48584310af84f | 999e719a58b492ec4efeea83d3ab137fad6f866915e555429bd67d67e4c4af95 | 856 |
| playwright.config.ts | 100644 | 68eb6494a2142383009bcb8e994e95e09dcd424d | d973420d37d3fe71eb396f6807460e3c23372ee1a16e9faa5e23baf0c3103f70 | 279 |
| rust-toolchain.toml | 100644 | 3417ee590341edc47a046d94e77ee9bee2b33bf4 | 97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe | 86 |
| scripts/ci-budget.mjs | 100644 | 61ac6e9c78821011afc30ec119d046c7c5c10a4b | 6330d197149c524798eadb40940fe13dc5cd47e4e85b1362d5c421e8c15f6305 | 2439 |
| scripts/ci-budget.test.mjs | 100644 | 34e1dba90848c91d3318f738955b1681b1217f9a | 280aaa46c1024f03f7f8379ee550540b45da456e7b32ea27dedbd9f81606d411 | 2627 |
| scripts/ci-workflow.test.mjs | 100644 | 2b8154b5097c331b4df55c134560857c13d5fb00 | 364c35cbd1442f60f319437c0b07d9f0a38988af4af3762e7290c9ac69ede775 | 2367 |
| scripts/enable-auto-merge.mjs | 100644 | 7683fca9dfb722a6565253bceece8189f8db4159 | 3e1a1f458fa61fe54ed6e17851bbd72daebaa08cf317dc670ad723264480765f | 2330 |
| scripts/enable-auto-merge.test.mjs | 100644 | 2ddb2aa529dceb2d8d73a7d19a8546ca5b17983e | 41f0000ad87cfc166b5b18ce74977d85cf0248b15890f8f7ef70822221e01337 | 2836 |
| scripts/evaluate-budget.mjs | 100644 | 75c21e6d3342f73ed22c34b40a7262d7a10ae385 | 01d09ce75ce53906c52794a43c01b3d3cf4c79d042b0545f6827456ff22033eb | 3632 |
| scripts/evaluate-budget.test.mjs | 100644 | df92db35466aecf9938c4f75e34b605194aaf03a | e94b94fa52115f6275dc79e9e0f885cb3419042e8ea542524d13cb17c18eb5d0 | 5438 |
| scripts/fixtures/ci-budget-buffer.mjs | 100644 | 4fb2e5c7a8458a220238d8c5175d242a29c9c6e0 | e136f49793f58396441f666b76bdfb59b5836bad59c089616a8d32819c4d66e4 | 4443 |
| scripts/policy.mjs | 100644 | 3586a49bd349f882ca535967ea988a326636476f | 03ee2073a6efaac78bc2d25d0276ea25811adf07dea36b1cb922e9b8f87bb3ea | 986 |
| scripts/policy.test.mjs | 100644 | 5720ce0b745e8e811eede997ca9a0ddeb85cabed | f609bcb5e4415c2b7a961783e91285ca024275c3424615d5da0bd2f55769fc92 | 1345 |
| scripts/test-service-artifacts.py | 100644 | 54b0baf3bfc9a9262a1d9deb7a0b46a2bfcd5437 | df0a2aba6c5faa1186770e1c1b67914a0792c0b170b3851b61072d1405583dc4 | 2710 |
| scripts/test_discovery_drain_mutations.py | 100644 | c88069c63f879fa96fb1191aa52f54e7b450d503 | ae3f85aab982258a1693ab74e7d3365d630eecc31b71dcd21790bfb870021d51 | 2771 |
| scripts/test_discovery_e7_mutations.py | 100644 | 1ac80cdf093fda548fe641ba6933824fcfb0c233 | d66938a2cae43084d9d22e8204e6716f60d6a5a6564abfa6cc2b7663ed525a4a | 2513 |
| scripts/test_discovery_followup_mutations.py | 100644 | 962e9e4f0d06e84755b5a87ef3332c1e265c30a9 | 6c06cb3464f2451a4af07880ccc28ac829eb9ce6b70f2b1ad51be444f3ea7796 | 3620 |
| scripts/test_readiness_e6_mutations.py | 100644 | 4a6ecf102b68e22752b099cc4f8638e87cc1c21d | ffe62e7b4fa99eb90ed953a1a3510f8b88014eddd9978191661ec35a5c485ce2 | 3596 |
| tools/ci/.gitignore | 100644 | c18dd8d83ceed1806b50b0aaa46beb7e335fff13 | 32dae3052f331ee34d628ef535709b301259a45df7c7522c4d35dcf49873f00b | 13 |
| tools/ci/README.md | 100644 | 2acc3dcf6aa55653378efcbd410cc9a65a1c1be0 | e3564c7ebb2e27a7509ba2f8354970b0eed40f96d0863364401883ff2714dc53 | 4375 |
| tools/ci/a7-admission-readonly.py | 100644 | e0998e5ed901e70aca54343e8052fcdbd5c9607a | 0fbe35e7241ed05ba6d44213f73e6b715ffa6475e05204cb40ab31e783e7be73 | 9172 |
| tools/ci/e3-native/driver.py | 100644 | fbbb32d8c70a4b58ab705582885b28f210a04233 | 5a2b0d68640b1f745a8983482f7363f975e2bbaa26aef1e11333b066c7b7a627 | 11500 |
| tools/ci/e3-native/mutations.jsonl | 100644 | 6cff8e824c687d5e4bd3cb9e07a284f40b4369af | c824820588a406800124352931597e3a220a32b945333e85d3a186612bca3d93 | 2013 |
| tools/ci/e3-native/test_driver.py | 100644 | ead690c51d712d032c28ef5c0ca4af9467e2d482 | 2f3fba561afa0058abd55eaa7a799cc6a0b7acbd4b2c3a7c4b756331dc305471 | 14894 |
| tools/ci/e4-native/driver.py | 100644 | 1551ab3f6106883c3234198c766f5642fc59ce99 | 8151d6a5546142c42e2b16445ab0e7117dc7bc0707b09f70c68a8f2098e3da74 | 14113 |
| tools/ci/e4-native/mutations.jsonl | 100644 | 6891576ab16ee8561f984949cbd727e5083dd320 | 15980053ce88457578407686d309af95a6dd18cb175fd777d82ac0e70b850671 | 6239 |
| tools/ci/e4-native/test_driver.py | 100644 | 971a21d6477002e20330ab59a482be5ddc265ff3 | 38e14eb4fc92fff62a304d53473dffc80cbb7465a25b31b65084f613d3104eff | 19530 |
| tools/ci/observe_dns.py | 100644 | e0120128e2e6da7339e6f214597f9b1ad5e9a810 | 7adb75525656937b43173c98dd04f46522631b1103700b5975732f02da0b255e | 1758 |
| tools/ci/observe_launchd_absence.py | 100644 | 923b0374d28d3623c4cbb3a4d12c645721b8fe7d | f5748bcc0d370d63b80a1c364780ee17ee1d5399af0527d7b92303757ec1cc27 | 7457 |
| tools/ci/observe_manager.py | 100644 | c7104fe1c827f992a90fa3d40668ada3455a0efa | 7d19274b079b645fa70b72460c14f92a2b2a689e3b379af9f8b82e6acd567302 | 6066 |
| tools/ci/s2b-native/driver.py | 100644 | 379b56c342b92e94faaaf27282d319b66a94c386 | d8582b949e579f8ccd2405ca5edc4c2f07e0ff2a260f7cc791dc224ad7057883 | 11337 |
| tools/ci/s2b-native/mutations.jsonl | 100644 | 1c04b068c8a46e037580f2c25945f42d283c4696 | 28f05948ca4aace6944fbe8c77676e8f702575042cf77d9c9c176096ac5b0f94 | 588 |
| tools/ci/s2b-native/test_driver.py | 100644 | 93f3bfcad6b89bcc84b31d2b1abc5f94edf35f5a | 9eff03de41890654c0cdb6f4f1d80b0fb54b6f1cebe73e5bea507cbf56be4057 | 8543 |
| tools/ci/sudo-package-readonly.py | 100644 | 40f9ea1ffd60ec9c31cf8507ade3000a2dca0eb4 | a40f58ae124b1d4136197cada6bb0cec08278ae04668d0ebca633b14a66f3f0c | 5869 |
| tools/ci/test-a7-admission-readonly.py | 100644 | e70649566ce39d0840df85696c6479d3ff1586ff | d5ee530c1f537be1b907eb0dca4ad764ef8f13c7b40892f9328187f6698a770b | 17064 |
| tools/ci/test-sudo-package-readonly.py | 100644 | 61a204201ab7af88e621b543487049b3500478f9 | ac114477eea3bc2863376c6c93954f0a9394527ca99b16002798edf4c3330ceb | 13765 |
| tools/ci/test_observe_dns.py | 100644 | f73515ddcc550f2959ae1d8ee10abda3b916a3b4 | d8ce3a1fc0b8f5aa40035abf7756ffbabdc4055307cf207d9080e0df16a408e3 | 3688 |
| tools/ci/test_observe_launchd_absence.py | 100644 | d78684c1e6f4be95b9787fe12610cd2372e2c2c3 | e4ef8f4d9bd64b0e5ad73aaf202563bdcdce46f00ac50191c8acfc787efc3d05 | 10608 |
| tools/ci/test_observe_manager.py | 100644 | 2f76bab053e72481895ddcc55d9b9c9bf68939ff | f58504815797f5c20e0fe56adc64cdf0b0256f6d5ea6d3127b0eb634f543cf38 | 6575 |
| tools/ci/test_ubuntu_mirror.py | 100644 | 1521cb29637b5afb62b779af8bac725ad4b7b42b | 94003fd00ff872e2ac073022a36006534d81bb03a1f6796c0cb873459da8cf66 | 33107 |
| tools/ci/ubuntu-mirror.md | 100644 | e800270f2f0d714e884b9460ecc174ec25e6db46 | 19c4238708f61a9a0ef0b53da06a16fe726ffe4d3525292dfd997445fef775a7 | 8384 |
| tools/ci/ubuntu_mirror.py | 100644 | 79b583e57b313eb5c8f968b1f2d441cf548720dc | 2c861c8b7adf8c330831fff7dd0c9633e31a66723f6cbe5077284a30c801fe88 | 10919 |
| tools/unix-exec-validation/.gitignore | 100644 | c18dd8d83ceed1806b50b0aaa46beb7e335fff13 | 32dae3052f331ee34d628ef535709b301259a45df7c7522c4d35dcf49873f00b | 13 |
| tools/unix-exec-validation/README.md | 100644 | 6e4b36c937c16aca2681b1ab1bc00acd8b3cda71 | 2fe1f138e19a6fde6af77d3b7d4d1952caf6a65c6a8e74d65e54a6fff682938d | 13378 |
| tools/unix-exec-validation/a4-helper/.gitignore | 100644 | 71e92a1fd2ea41385c1b23960a377bd1f547977e | c6e4e440e24869a9e65fc569416d62de9667d434d63172039c8258f84ee9ecbe | 20 |
| tools/unix-exec-validation/a4-helper/README.md | 100644 | 10ab0391abde27e45558c2ceda61bf5e0ed830d0 | ee4aef2d86696c8b27362aff0c5ecad58720bda6d6120010439199a30d068e97 | 7506 |
| tools/unix-exec-validation/a4-helper/build.py | 100644 | 5412b0cf345d3bd5dd0106e972a6b10e12827b7b | 6db176037849bdce663c25b79dc34025fae4539580ef832ff9f6f9592d12b45e | 10727 |
| tools/unix-exec-validation/a4-helper/helper.rs | 100644 | 5a02cf43d8abaa29e221be4ac54221e2ae05354f | 770ab397264ab78deb5d54854bd00d4cfabb1b3660b0e975ee751472fbeada5a | 7468 |
| tools/unix-exec-validation/a4-helper/image-pin.json | 100644 | 0554592aff7cc97e869f7f8a4dbf25b254aac640 | 7ae3e8c501e53681e327d2b229ed45b5870ba0e9df7a3c304aad1025631df971 | 2348 |
| tools/unix-exec-validation/a4-helper/linux-image-pin.json | 100644 | 43e23c1b8597eead804e78cdf69c58139a788bc0 | 6427856805b5fa362de01cace73930c6f1f9fa78ee08f32a8da61a252b0a13d8 | 2350 |
| tools/unix-exec-validation/a4-helper/recipe.json | 100644 | 91ee0a9075a87447bf531d6089c08547e2767fac | 90a3d497971ce504470c6bbead42e4a1b37f77ec0cd56b11cbb6312be739b47d | 759 |
| tools/unix-exec-validation/a4-helper/test_build.py | 100644 | 1a9d6829e6bb2e40c379b8b3fc208b8440b4ea0c | cd192281a079c7258ebf47bdc68fb184ed1b5b1703e5337fe4ff3037b03d4c2e | 9901 |
| tools/unix-exec-validation/a4-helper/tests.rs | 100644 | 27f880a5fd4881a08ab777f0e28de66fbc5cfeee | 215434d57814a8b2055582ddd84929bfcefc66253428c8a0ff40ea903b5805b4 | 6076 |
| tools/unix-exec-validation/driver.py | 100644 | 48e4fe6e549d0b813e0c5198aeb3506366851e6c | 24dd0d6a2c8948f2092e54fff40bbb03c4364ab2a07e59c6e12d62517e5fe47f | 96767 |
| tools/unix-exec-validation/mutation_check.py | 100644 | 8e3074cebd3ac8fdb2effa28e334e2eeca99c7fe | 752a0570f53f4275370318561bc16790684e075f9bba4cffdaa2d8eda0cd5c89 | 58814 |
| tools/unix-exec-validation/sudo_host_qualification.py | 100644 | 41ec1325a77669d34c20f00c4b04d40c9d8086a4 | b6a1891c14acfacfe2bbd2df1aefa1e79736c6be1e0592f353dea5ea55c3c5c9 | 8528 |
| tools/unix-exec-validation/sudo_package_profile.py | 100644 | 520d341633c67dc52ec483e9b3fcc92131a3da1f | 12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75 | 11180 |
| tools/unix-exec-validation/test_driver.py | 100644 | d6d8afea18c19ba04a8825be81509091dc66949e | a5fae4a61066dc9758fcfd3d91d46455b33e81cc0ce34f035a9a62f93f04d585 | 166104 |
| tools/unix-exec-validation/test_f0_blockers.py | 100644 | ce2357d1e3c14013f13eac52d72c8e5983814b39 | aaf69eb25d4489cb11536a41430f264c4157382c52cdd60bce6edf2093906fef | 20106 |
| tools/unix-exec-validation/test_f0b_transport.py | 100644 | 294568ab0cadaec82a7497f9a29858d605ccff04 | 0746e09cd8558602e97abcea1c9419204023496ef940d4197146226c8af99dc9 | 35252 |
| tools/unix-exec-validation/test_sudo_host_qualification.py | 100644 | ee0967be3ef09a256fc4867f81bcab0b9af2c7a4 | 04ed579578b4b80ef4e95f2a3f1d4bd93cf088cc52bcc65301858de4833046f8 | 12116 |
| tools/unix-exec-validation/test_sudo_package_profile.py | 100644 | 00dbeb7b637df485492168e9199139f1dca85db1 | c62f4558fdc5d2a61222599c41073c7c5140f8917f64625a71d80209602ae629 | 14619 |
| tools/windows-identity-probe/.gitignore | 100644 | cd42ee34e873b4ddf17cdeeed2e6c0d4e094c62b | 6c5082c7e6de3e350189c67a1aacf757485ecc26e0a3cf96683528565f3e5f12 | 10 |
| tools/windows-identity-probe/CiValidation.psm1 | 100644 | c62acaeac5f111acaaa1c4de17745e9a214b242d | f7bd0e55c55f08c321114eb67da48040bf3707bac68e119e273b1ad0351cf6ab | 9159 |
| tools/windows-identity-probe/DEBUGGING.md | 100644 | 4014b7d74ab0f7345f0264397aeba17ac6d15241 | cd77c41afe4ae8ece54bf1715b60f6da14dfd324229b5245f9ae79132f91a621 | 8258 |
| tools/windows-identity-probe/IdentityProbe.csproj | 100644 | 87dbc89ac7d56e08fd998d65df7eca3056f9eac5 | 8e61e6cc978f874618a0eb93f7f20c34843ffa91b331766484454001e54b9029 | 351 |
| tools/windows-identity-probe/Invoke-DisposableCi.ps1 | 100644 | f1d923809bf9e6ab80ce6f34c6d2472f18385cb7 | 0465f383cf1efa2171ddd6786e0e349b00ab12a6e1d099cbc8eb3c015b77e05f | 11251 |
| tools/windows-identity-probe/Native.cs | 100644 | 612d007b4a704c19b0abe2c0e173e8a06f9e92ce | d90b7fee27bb1ce1d00e50948a2616ebf1cabbb7699283d04e7d584304b89986 | 20597 |
| tools/windows-identity-probe/ProbeBatchRight.cs | 100644 | 50cbdaef70ede61f22ec442fbb0c4ba8d64ee3fb | 8714a0c523fb9c23a8e5822944d36028b01721bca6aeaaef5dc18c5ac8a0469f | 3747 |
| tools/windows-identity-probe/Program.cs | 100644 | e75f03dae5bb5c8087c981da378ac9b165040393 | babb3c5940189869d57f0e888d9e074e8f92835c4b9c143ae90868997bd18280 | 14339 |
| tools/windows-identity-probe/Test-CiValidation.ps1 | 100644 | 797ef395ceca74ef56cff8e6b8b8aa7430f588b0 | fc6fd2bb6eac0c0b3e6390676b67659af00f794146576c3cd4232bb346f72c15 | 9733 |
| tools/windows-identity-probe/Test-GuardMutations.ps1 | 100644 | d4d3537d21e70141d78c7d587b7bb30d3deee60d | 02dd320056c812b05bc36f28afe77ef2058fdf90102989bd48d336fbdf0e19ea | 4361 |
| tools/windows-identity-probe/Test-SetupDiagnostics.ps1 | 100644 | df191a56506c00ba9183762efad38283b75c7b30 | 8e577d85ca5d8e05e3d2c8e324b54b47a2f2901409dd7e02e449f0041a3ddb0f | 8391 |
| tools/windows-native-replay/Admission.cs | 100644 | f26127281e627d3cca76312846a18243206cd063 | b6c1aa6b39d618a85e2183988cc95b44c965dfbb44670b2c2c4382d336754289 | 16394 |
| tools/windows-native-replay/Check-Source.py | 100644 | 6c1185276c27dc71453d40a1ea2bd10bf3f52170 | ffe03dc462928c0746ba4bdea0dfcfa4d0a6e196441e962390cff91977aeb0ff | 7911 |
| tools/windows-native-replay/Controller.cs | 100644 | e1c83ed2e3701add6a9feddea9d2e94b13cbfe9a | d1155dcce5464270a1f98fc08fcfe116365e355694220d9eeb9d835824f4fda5 | 61494 |
| tools/windows-native-replay/HeldFiles.cs | 100644 | 9526c04b368664ee23e6a6efbb27e554cf5b10b9 | 9470b8f11a37ad22ecc7eb28926de444d0c8d5cd6e25e64b9e0131459ada1643 | 14742 |
| tools/windows-native-replay/Invoke-BoundedReplay.ps1 | 100644 | a4c88570d6012ae356c9b4f1e71b2f0d0d76acba | 29dc8f9f6468d9c03d9defe65381f6f13064ee354aa7637309c0a50b98b6fb33 | 757 |
| tools/windows-native-replay/NativeJob.cs | 100644 | d9a0fd2e78586b16c3f625cc73fe0928f932cc80 | 7c41bca70616f2ed6a0aeed5d30ff9700d1e9a2586b0a3e8771bd1d5665efab7 | 13876 |
| tools/windows-native-replay/PINNING.md | 100644 | 2cd40a0e13bb680a9d59136dd1dff0c0942f4518 | be261f32e527efd333c66c994d5f621cccc44e2731500186edf9a76bd647b725 | 19108 |
| tools/windows-native-replay/Prepare-Provenance.py | 100644 | cbc1ddc275b5543741b7f517699d7e7a8a3120a9 | 1b0f44ef0eb368cc4d47dd343df813fa3b52ef3e0d62ff23fa9b8711596785ee | 2924 |
| tools/windows-native-replay/Test-PureController.ps1 | 100644 | 1a39056c9e5298c6d520794e843eb031d0350ced | f13089b413634559d69c948215cfb20e467c5534a89cced9a010aa13ee0037fe | 11554 |
| tools/windows-native-replay/child/Cargo.lock | 100644 | dbf05ddef24026e55c6e87b9a38a7c7899412b9d | f4e0e27cc75747f2ad09ac4bccda3e0d1cbf34df174e9584074e56957377cd22 | 171 |
| tools/windows-native-replay/child/Cargo.toml | 100644 | 6d36690a7b1d4406339de7b63d753de4db44a091 | e6d25788c0611c605743e738d3d185222bfacbd503bdbcc2f22a9db255eb49c8 | 227 |
| tools/windows-native-replay/child/src/main.rs | 100644 | cd5b7c59da062c25118665b2f164769265545e7c | f2cb904fca9f0e8bab8a6ca13b7b88dbea5b769b871c014621e51f01f7a5fb32 | 88 |
| tools/windows-native-replay/selectors.json | 100644 | 41a9499660e8b0b40b21bd021f56ba45d760474a | e9aa9e6f405c2142d392096926830f0ba5b59be1a8ef46409d75d1513eab9b23 | 5197 |
| ui/index.html | 100644 | 20ab11d2a18157132247baa727940661c2b592ca | f626902e95b7baf01f14d9a8cc452025bb65f217f4205d585f3644092af7ad3d | 250 |
| ui/package.json | 100644 | 1799eb63a4394d847182abeda9fab5875909d41b | f9ae658e3591c0c7eb055ebd3c922ddebdcd559cd23fcc5bc92c07f2d0afbdc4 | 422 |
| ui/src/App.test.tsx | 100644 | 78ddaccc4262c91ae9db53ab7b04d312875252c9 | 985f17eababf768df2d15e5e24b1e523cdb2ff45a1559688edc2ef13144a8e5b | 441 |
| ui/src/App.tsx | 100644 | f6dc928d2bb381dd2662dab5084236e0ef5d2bc3 | c5e39b710ebdfb4a8beeb7d9c23649a1e5f8a0c93ae619da56b558bd6bab3325 | 132 |
| ui/src/main.tsx | 100644 | 1c773831afd7ba399ce6d31fa71fdecedcc2ebfe | 21ccb927edd915b062545ef76e563a8627ce86c0c5693155bac7f46cf061614f | 139 |
| ui/tests/smoke.spec.ts | 100644 | 903b1d97e221e4f42900ac4e02b02851b114a652 | ca6c1e7f18e25cece364174e49a8355795a7b030f6ef4a20fb34d9580d497683 | 344 |
| ui/tsconfig.json | 100644 | b212ae2d17ec1ec67ad3fd41b3514573c27bcd7c | 292aa97e9dc22c56e46bd8cb04686c439f68dc5a994cec0f93eae64537e4314e | 281 |
| ui/vite.config.ts | 100644 | 041854f74501ecbb719e93f24f73e471596ab572 | 320350dced549cebe2ae248283028ee7ae0fdbb36576a694f70cfbd755f3a30d | 82 |
| ui/vitest.config.ts | 100644 | b916fd8d80a9ae1434207a621d8cfc15291c271a | 02eea6d1beb6b7aa3ffb68c36c47cc01abd404ae2c6770aa889282ccf9652011 | 122 |

Key pins are the inventory rows for .github/workflows/unix-execution-experiment.yml,
.github/workflows/ci.yml, tools/unix-exec-validation/{driver.py,test_driver.py,
mutation_check.py}, tools/ci/a7-admission-readonly.py, crates/arxd/src/exec/
{mod.rs,unix.rs,cgroup.rs,disposable/a12.rs,hierarchy/setup.rs,hierarchy/linux.rs},
packaging/linux/arxd.service and standard-rights drop-in, Cargo.toml/Cargo.lock and
rust-toolchain.toml. All workflow, production and target-oracle source rows are
unchanged except the five declared candidate source paths. Target identities are
fixed /usr/bin/true and /usr/bin/sleep with literal15; trusted runtime metadata is
required per call, not a fabricated host-file hash. D7j wrapper consumed MODULE
pin is driver SHA256 24dd0d6a2c8948f2092e54fff40bbb03c4364ab2a07e59c6e12d62517e5fe47f,
size96767. Its execution grant remains consumed.

## Opus S1 correction

The first historical driver mutant now anchors the exact A12 TEST assignment with
a leading newline and names the current candidate-selection test. The prior A7
anchor was stale and stopped the historical harness before its 40 cases. The
coordinator ran `mutation_check.main()` after correction: 91 audited tests passed
and all 40 historical mutants were assertion-killed. Driver bytes and wrapper
pins are unchanged. The inventory row and candidate digest above are refreshed;
fresh Opus recheck remains required. No native execution occurred.
