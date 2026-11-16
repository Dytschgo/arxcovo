# Founder bootstrap F1 — local implementation evidence

Prepared base `ff4eece89ca0c76870cda6d971acec7e6cebf8ad`; source exact main `c6121523417089bc0afaf307470183dd4a7f47ba`; accepted contract `455d8ea63463f426d4d829c73965dac2738948d1`. Final execution-order amendment permits isolated local work only. Provider Sol, high effort. No implementation correction was needed (0/2 consumed). No natural red occurred.

Implements single-voter founding from a proven CSR, private sealed subjects, persisted first-free=2 admission before initialize, owned bootstrap snapshots, fresh authorization guards and an absolute release deadline. Existing issue module is an identical byte prefix of the amended module (Git LF bytes); consensus.rs, adapter.rs, start/fresh_read/shutdown and existing test helpers are unchanged. No production CA constructor, custody or transport. The validity helper retains the fallible Sign path before the bool containment check.

## Commands and actual results

All Cargo commands used `CARGO_TARGET_DIR=<workspace>/target-founder-local` and `CARGO_PROFILE_DEV_DEBUG=0`; heavy commands were serialized with no shared target. Logs are local temporary artifacts; hashes below bind exact output.

| Command | Result |
| --- | --- |
| `cargo test -p arxd --locked --lib --no-run` | PASS, cold compile 60.00 s; no compiler corrections |
| `cargo nextest run -p arxd --locked -E 'test(/^cluster::/)' --no-fail-fast` | PASS: 155/155, 468 skipped; test wall 9.889 s, command 23.026 s |
| `cargo test -p arxd --locked --lib cluster:: -- --test-threads=4` | PASS: 155, 13 existing ignored, 417 filtered; test wall 19.75 s |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS, 33.55 s; no lint corrections |
| `python -I -B -m unittest discover -s tools/ci -p test_windows_cluster_quarantine.py` | PASS 3 tests, including all founder skip/binding/seal mutants |
| `python -I -B -m unittest discover -s tools/ci -p test_windows_diag_sample.py` | PASS 58 tests |
| `node --test scripts/ci-workflow.test.mjs` | PASS 4 tests |
| `cargo fmt --all --check` | PASS, after exact mutation restoration |

The natural cluster nextest and in-process runs include all F1–F10 cases, pure PKI and enrollment tests, existing issuance I1–I9, active R1–R5 and crash probes. Existing eight Windows quarantines were not forced or repeated; process-child ignores also remain unchanged. No full native, hosted or Windows `verify` run was authorized or performed. The 660 s hosted rule is unchanged, but this local evidence cannot establish its hosted headroom. No comparable baseline was run to claim an overhead reduction.

Founder test durations reported by nextest:

- `PASS [   0.206s] ( 10/155) arxd cluster::consensus::issue::found::found_tests::f3b_persisted_allocation`
- `PASS [   0.298s] ( 11/155) arxd cluster::consensus::issue::found::found_tests::f10_drop_bounded`
- `PASS [   0.311s] ( 12/155) arxd cluster::consensus::issue::found::found_tests::f1_happy_registry`
- `PASS [   0.303s] ( 13/155) arxd cluster::consensus::issue::found::found_tests::f6_adapters`
- `PASS [   0.334s] ( 14/155) arxd cluster::consensus::issue::found::found_tests::f8_store_privacy`
- `PASS [   0.391s] ( 15/155) arxd cluster::consensus::issue::found::found_tests::f4_registry_restart`
- `PASS [   0.386s] ( 16/155) arxd cluster::consensus::issue::found::found_tests::f5_joiner_two`
- `PASS [   0.428s] ( 17/155) arxd cluster::consensus::issue::found::found_tests::f3_preinitialize_refusals`
- `PASS [   0.437s] ( 18/155) arxd cluster::consensus::issue::found::found_tests::f2_once_and_concurrent`
- `PASS [   0.453s] ( 19/155) arxd cluster::consensus::issue::found::found_tests::f7_clock_guard_order`
- `PASS [   0.023s] (124/155) arxd cluster::pki::tests::founder_pure_identity_and_validity`
- `PASS [   5.174s] (150/155) arxd cluster::consensus::issue::found::found_tests::f10_shutdown_owns_closure`
- `PASS [   5.208s] (151/155) arxd cluster::consensus::issue::found::found_tests::f10_outer_budget`
- `PASS [   5.514s] (153/155) arxd cluster::consensus::issue::found::found_tests::f9_post_deadline`

## Mutation evidence and failure history

Each command below ran once as an intentional mutant. All returned 101 with an actual `test result: FAILED` and no compile failure. These are expected failures, not natural reds or implementation fix attempts. The F8 probe injects a CA leak into the test directory as a scanner positive control. The F2/F4 probe bypasses both the persisted initialized precheck and NotAllowed refusal. Python finally restored all three touched files byte-for-byte; fmt then passed. No engine/test failure was rerun or fixed.

| Probe | Exact test filter | Seconds | Log SHA-256 |
| --- | --- | --- | --- |
| F1 fake registry digest | `f1_happy_registry` | 7.734 | `0546a0bb79fd837b72d5d3da03ebd9777ed3f60724db2f2e3e6b9600889b7aa8` |
| F2 repeat initialize accepted | `f2_once_and_concurrent` | 7.953 | `c523e9e60c6870454b3c2cb5c291f0d9a27916d8c254a5b7b636772a19adb73d` |
| F3 parse after initialization | `f3_preinitialize_refusals` | 7.688 | `9cf1e713036445967ff328220ec98ef7d63fcba32b4e17f2a85ee404ff38a11e` |
| F3b allocation removed | `f3b_persisted_allocation` | 7.703 | `5d7047c978c65988c2b1d8f293a1e0b45b01fef1069fb357142e12114d9a09b8` |
| F4 restart refusal bypassed | `f4_registry_restart` | 8.281 | `7eadf19912f15ed989a79e6351d548779218fdda802b052584a188067d12fe27` |
| F5 constant fingerprint | `f5_joiner_two` | 8.094 | `b56718c9f4269fa3090d677086807c112fcf4561947a7a354da6068c8952f86a` |
| F6 issuer node check removed | `f6_adapters` | 6.14 | `88eec1c6a5c1a95b45c2a0b9d5d66e2b3de50d15accbb36862e0ea7f0ea65995` |
| F6 NotAllowed wrong mapping | `f6_adapters` | 6.047 | `eb65251382f9bdf2787f7dd8200aea081bf0d46f35dc1e4b920d6f7013e0a60d` |
| F7 time sampled before guard | `f7_clock_guard_order` | 7.266 | `478648cf5194998233ad0c1235d037978296c0a4628a9b834dd62ab3ed773ac8` |
| F8 injected CA leak control | `f8_store_privacy` | 7.109 | `13578f690dd5ae62771c52691c3df5a7abd205cd14a0f922bce1f00ec03d07f6` |
| F9 elapsed check removed | `f9_post_deadline` | 12.657 | `9f9d2bb6700cd434721967268e1bf2b4a2c8ea184acc9307a026bbfa090ef1db` |
| F10 caller owns permit | `f10_drop_bounded` | 14.171 | `94ea9f21cab7337dd9a6e71536f257562537ef35d2dd126aad6dbc8df9e4fe58` |
| F10 async owner owns permit | `f10_shutdown_owns_closure` | 14.516 | `da28c7fa24db8c20d570ebe2470e5888e2c3a3a677f595b67dbd7693c8822e9c` |
| pure zero randomness accepted | `founder_pure_identity_and_validity` | 9.141 | `93e26de185f9b2f98e7b36e8573a657093b6e1e3a451716ddcb0a44c466cb838` |
| pure validity split boundary | `founder_pure_identity_and_validity` | 5.953 | `ab494cbaf7f1dea13ddf5955cbe4de10fef04340f88613f949f7015a87dc7005` |

Every mutant command was `cargo test -p arxd --locked --lib <filter> -- --nocapture`. Full command arrays, changed-source SHA-256 and local log paths are retained in the JSON listed below. First observed red: deliberate F1 fake-registry-digest probe, returned Revoked at issue unwrap. Natural first red: none. No shared Windows recovery attempt consumed.

## Scope and artifact pins

Pre-test diff inventory contained only the five modified allowlisted tracked source/guard files; the two new founder files were untracked at that point. Post-test scope assertion checks the full tracked delta against the nine-file allowlist, compares the full pre-existing issue module prefix, and checks the following exact-main blob pins. Mutants were confined to allowed founder, PKI and founder-test files and restored.

| Frozen file | Git blob (same as source main) |
| --- | --- |
| `crates/arxd/src/cluster/consensus.rs` | `5b83422bf8d0cc56b7c76ad15609543c40c14e00` |
| `crates/arxd/src/cluster/consensus/adapter.rs` | `181e9dce6765b1db9339c254ec2dcead9b88a0b9` |
| `crates/arxd/src/cluster/consensus/three_node.rs` | `f0e2c96c130a61ab66f5299e296fd49654794289` |
| `crates/arxd/src/cluster/consensus/mem_net.rs` | `e799e2510ab12a23f5fbfa7a64bf1c2c8c108712` |
| `crates/arxd/src/cluster/pki.rs` | `5fd6a00d44a8bd69ef591417baa0ec1e7de60af6` |
| `Cargo.lock` | `5e0b3191cf2141f430743ed31b7f4582f202cfbb` |
| `Cargo.toml` | `343f7301c8744ba3ee1b8ebfb5422c70a04479f9` |
| `crates/arxd/Cargo.toml` | `a0f0adc2436b284693ce48d4d56a800cd6382347` |
| `.github/workflows/ci.yml` | `0c7af38b33f6c127029d8312a0c9ecd92f6361da` |
| `tools/ci/windows_diag_sample.py` | `6ba6f257152964cbde7722a3f6a633cdbb3fa056` |

Feature blob pins (before evidence-only commit):

- `crates/arxd/src/cluster/consensus/found.rs`: `b4de3dac384a3f1d7c004d81f4472a5bb4ded20c`
- `crates/arxd/src/cluster/consensus/found_tests.rs`: `4b46f4fec4cd6f40b87a6b0eb90e3d1af528d8b4`
- `crates/arxd/src/cluster/consensus/issue.rs`: `101f0175f318902d118d08566ab3623fb0912b83`
- `crates/arxd/src/cluster/enrollment/registry.rs`: `ccf1535e79709c929ffd14c5bfc532253dbb2736`
- `crates/arxd/src/cluster/pki/ca.rs`: `288be77634295529df6e0fb8e85fff46b9e4cb12`
- `crates/arxd/src/cluster/pki/tests.rs`: `e531935a995344e08172760e92cd5e8fec42922e`
- `tools/ci/test_windows_cluster_quarantine.py`: `d1ba3446b157f7b6c6dadc4f326be3fc6ca525b3`

Local output hashes:

- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-compile.log`: SHA-256 `ce342db51e367cfaca7721323a3911fa6bbd656e539ee4d45039b441859b41f6`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-nextest.log`: SHA-256 `a311696824e2aad958a40b9a87f9e8b8a26d3172ee41c3a1b11bb4d19c3fb58a`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-inprocess.log`: SHA-256 `0ba97ee83d7329e27d81692ba67c96e587e685d5b9b5a32f4f1e5cd1ba9011ea`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-clippy.log`: SHA-256 `674bf2d6f5d24625b3542226bc28d3faba1d4bac40f8e9b0bbe695ca4dca6a0e`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-guard.log`: SHA-256 `6c13f3a1c1ae62287170d456acb3139e23e6025f6abbb2b00ce1ab42557f8425`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-sampler.log`: SHA-256 `a37f63196910c91b7ed983d0df14d6e6d85d7ad9a2e349bc427f64929534a0af`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-workflow.log`: SHA-256 `3dfdf84d21315ae77b2d45a3c2c6aed73bbb8af6ddbc8eb37265782d304632dd`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-fmt.log`: SHA-256 `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-mutants.json`: SHA-256 `4c85865913c794e589a0e82116853a72ebd33282725c8ad02411e71cde8fe587`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-mutants.py`: SHA-256 `f5816e08bdadc74a5d2b3ae83e4b7be077d2350ce7f2cc7cd2e34f35852077a8`
- `C:\Users\DYLANF~1\AppData\Local\Temp\arxcovo-founder-scope.json`: SHA-256 `268da1e7b7d4678449fed34a13544d729bfcd193761e337430e0354e7703cf9f`

## Limits and handoff

Local Windows single-process temp-directory evidence only. Restart proves Raft and registry persistence while test keys stay in memory; no key recovery or custody claim. Store advisory 1 remains open. Full M3 and hardware paths remain **unverified on real hardware**; M2 open, M3 unchecked. PR159 held; PR162 observations were not integrated. Main c612152 remains red per coordinator-supplied evidence, not a new remote query. Sol Windows shared fixes3/3 exhausted, Opus recovery0/2 unused. No children, native/admin/VM/old-Lab-Manager operations, installation, cache deletion, push, PR, status publication, merge, watch, dispatch or rerun occurred. Root independent review then Opus review and fresh green exact main are required before publication.

Source commit: `ec3e5d2228fe1d06564d3e7e4bfad40b5b1f53aa` (`feat(cluster): bootstrap a single founder from a proven CSR`).
All verification above exercised these exact feature blobs; later mutation probes
restored the original bytes. `git diff --check` passed. The isolated build directory
was moved, without deletion, to ignored `<workspace>/target/founder-local` after
all Cargo commands exited. Final process inspection found no cargo, rustc,
cargo-nextest or arxd process; all owned test gate threads ended with their test
processes. No cache deletion occurred. The local target is retained for review.
