# A7/A11 fixed execution candidate preparation

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-15; no subagents.
Preparation only. Source commit `7385785c50ec28118db6d84138f7dc9e21242ec7`, tree `e7747d627369e1ad32fd5985c588e6c99a3ff80e`.
Parent e26f0ce. No PROGRESS change by this builder.

Baseline verified locally: `7d8ab1c160a4af488f4a85abb8dc08a45e4d6103`, tree `ea260f731c6d0149de68df7f0e2fb9428a9bfb7f`.
Reviewed integration `ebafeed326b92c1a52afd934118ce0b9197bb2a1` has exactly the same tree `ea260f731c6d0149de68df7f0e2fb9428a9bfb7f`.
PR127 all five green gates and Muse exact-head PASS comment6893948634 are supplied
coordinator evidence, not rerun here. Baseline hosted Linux compilation is inherited
only for unchanged bytes; this changed Linux controller needs fresh hosted compilation.

Read AGENTS/KICKOFF, ADR0019 D7h/ADR0020 D9b, delivery contract and amendment,
package provenance/conditional GO/comparator review, D7h FILE assessment, mapping
resolution, integration source/evidence/review and budget proposal/review.

## Concrete candidate

TEST selects only the combined fixed A7/A11 entry. standard_profile adds exactly
that literal; historical cases remain explicitly selected in mocks. Unknown TEST
and rerun attempts refuse. The qualification comparator CLI, legacy original-policy
False gate and original-policy writer refusal remain unchanged.

Driver admission checks Linux/x86_64/root identity, loads only the fixed 11180-byte
comparator source with SHA256 12a122db7d30be04dd7523db046d49f4083882c318b375ccf0986ec0eb338e75
through root-down nofollow opens, then invokes its existing _read_linux.
Checkout source trust comes from the fixed source hash and reviewed workflow/source
binding; checkout ancestry is not asserted root-owned. No sibling import search is
used under isolated Python. Installed reads retain the comparator's protected
ancestry, three exact payload/ELF/default pins, two util links, bounded read/reopen
and constant refusal semantics. Its qualified flag remains false.

The exact closed candidate_match report must equal the reviewed facts. It cannot
qualify the host by itself. Driver separately checks C2 absent or protected regular
single-link exact C1 bytes, protected /usr/local/bin ancestry, bounded root-owned
/bin link to exactly usr/bin or /usr/bin and actual /usr/bin directory identity.
Missing/unsafe ancestors and dangling/unknown links refuse. Existing cg_preflight
checks root-protected cgroup2/memory/pids, systemd>=254/kernel>=5.14 and hierarchy
before effects. Admission is re-run before staging, controlled provisioning and
handoff; there is no cached/adopted host result or reused D7h host.

Controller reader and every delivery recheck use the same fixed profile. Actual
protected installed sudo/plugin/util byte equality is reopened before Runner
construction and each call, with held descriptor metadata and named inode equality;
util links, C2 and alias are checked locally in the unit. Exact-byte equality binds
ELF/default interpretation to reviewed package source under D8/A1; no strings-only
inference, policy parser, sudo -V/list query or loader/whole-OS audit. The controller
keeps the existing four read-only mapping/mount/source-byte/entry/account checks.
UID/GID>=1000, nologin/sole group, artifact pins and workspace identities remain
per-call. Production setup runs exactly once before Runner, retaining the existing
identity/delegation/user.delegate=1 floor and hierarchy checks. Systemd/kernel floor
is observed by driver admission; this does not assert a new per-call systemctl
version query. D8/A1 trusts privileged concurrent writers/mounts and OS dependencies.

Budget, READY/ACK, both scoped workers, both oracles, cancellation, join-both,
4096/64MiB/swap0/pids32, 55/10 RunSpecs, 60/15 call envelopes, 90-case/10-join,
120-monitor/140-runtime/360-build/10min workflow and stop-gated owned cleanup
remain as merged. No production, package, helper, Cargo or workflow blob changed.
No current native authority, acceptance or whole-host qualification is claimed.

## Canonical profile meaning

Literal `51d4fb5f17f88e41d21472e04859c194e15cb7a4a1ec046c06a5b022a2a8f210`
is SHA256 of the following UTF-8 JSON, sorted keys, compact separators, no newline.
It identifies reviewed static package/default/policy facts; <case0> is a template
substituted only by the validated generated account name. It is never an image
attestation, observed-host digest or substitute for runtime checks/native oracles.

```json
{"architecture":"amd64","defaults":["/etc/sudo.conf","/etc/sudoers","/etc/nsswitch.conf","/usr/libexec/sudo/"],"payloads":[["/usr/bin/sudo",282032,"6ae6665cc51295ea8aaba94a3377dafa563f950aafc6296c0704fb33401a31ff"],["/usr/libexec/sudo/sudoers.so",588080,"151d681d51293a040dc479cb0175d1a6d461a40ff455661cf6ad485adf305d4c"],["/usr/libexec/sudo/libsudo_util.so.0.0.0",133944,"aae167b9da2436a31eba12d9fb20b36120fd204b2c720d62881d09e89eecd26d"]],"policy":{"nsswitch.conf":"passwd: files\ngroup: files\nsudoers: files\n","sudo.conf":"Plugin sudoers_policy /usr/libexec/sudo/sudoers.so\n","sudoers":"#includedir /etc/sudoers.d\n","sudoers.d/10-a7-case0":"<case0> ALL=(root) NOPASSWD: /usr/bin/true\n"},"revision":"1.9.15p5-3ubuntu5.24.04.4"}
```

## Verification actually performed

Call-graph audit preceded tests: selected Python tests replace filesystem, account,
process, systemd and comparator entry effects. Guard CLI/native suites were excluded.
New loader test replaces exec and the read entry; alias test installs fake ctypes
and fake descriptors. No CDLL/native readlinkat, installed adapter, LinuxFs, Runner,
ignored/native/helper/probe/account/sudo/root/proc/sys/policy execution occurred.
Extracted Rust contains package_file/runtime_link with fake file/metadata/libc ports,
real SHA256 from existing locked development rlibs, or existing pure/mock functions.
The fake libc layout is a test seam, not proof of Linux ABI compilation.

- Final audited combined suite: 136 tests PASS, including the six RuntimeAdmission
  tests. The earlier 135-test run preceded the attempt-only refusal test.
- mutation_check.main: 91 pure/mock baselines PASS, 40 assertion mutants killed.
  Default mutant now restores A4 against the focused combined-default assertion.
- execution_preparation_checks: actual extracted package_file/runtime_link PASS;
  nine assertion mutants killed (owner/nlink/mode/size/hash/named identity/link
  target/link owner/link nlink). Truncation, oversize, bad type, dangling target and
  full-buffer readlink outcomes refuse. No compiler failure receives mutant credit.
- execution_admission_mutants: six assertion mutants killed (package result,
  architecture, C2/alias, floor, profile mismatch, admission-before-effects).
- a7_integration_checks: five Python tests PASS, five driver and seven Rust
  scheduling/envelope/completion/join mutants killed. Failure/panic from either
  worker still cancels both and collects both joins.
- a7_case_mock_checks: actual case function PASS for both indices, failed calls,
  residue, order/spec/marker checks; five assertion mutants killed.
- controlled_policy_mutants: nine mocks PASS; six Python and five actual extracted
  Rust mutants killed, including extra/missing mapping and read-only/live bytes.
- sudo_nnp_delivery_checks: six mocks plus actual schema/layout PASS; all listed
  account/profile/artifact/source/unknown-field/copy-budget mutants killed.
- sudo_nnp_source_checks: nine mocks and actual scalar/record/result tests PASS;
  all associated C1/alias/C3-unopened/O_PATH/CLOEXEC/exec-EOF/nonzero, both-policy,
  identity/status/cleanup/record and collision mutations killed.
- cargo fmt --all -- --check and git diff --check PASS. Static exact-body comparison
  against baseline PASS for a7_case/a7_cases and driver monitor/result/mapping/recheck
  plus all four cleanup functions; budgets/oracles/cancel-join/cleanup bytes unchanged.

Development iterations corrected obsolete default assertions, missing Windows
mock attributes/selection, a redundant length mutant already covered by exact hash,
and mock reopen snapshots that initially masked independent owner predicates.
Mutant failure classification accepts Rust assertion panic exit101, never timeout
or compilation errors. No dependency/tool installation or network fallback.
All new build/temp files use D:/Code/arxcovo-d7i-preparation via TEMP/TMP and
CARGO_TARGET_DIR; existing read-only locked rlibs come from
D:/Code/Arxcovo/target/debug/deps via ARX_PURE_DEPS. No T3 cache/database cleanup.

Not run: full Cargo tests, Clippy, new Linux cfg compilation or any native path.
Unchanged builder evidence in linux-a7-a11-integration-source.md is reused only
where source bytes/call path remain unchanged; it does not compile this candidate.
Fresh Muse source/security review before publication, independent exact-head review,
all five green checks, coordinator merge/tree equality and D7i disposition remain.
A7/A11, M2, full F/G and real-Runner PA6 are **unverified on real hardware**.

No dispatch/variable/network/push/PR/merge/admin or owner action occurred. D7a-h
and their hosts stay consumed/excluded; stale A1/A2 remains closed. D7i is proposed,
not exercised. The same future fresh execution host must repeat the read leg.

## Exact changed source blobs

| Path | Git blob | Bytes | SHA256 |
| --- | --- | ---: | --- |
| `crates/arxd/src/exec/disposable/sudo_nnp.rs` | `7f35fecc5504563c867eb6b568567cecbde533e9` | 56413 | `0bcddcd6bce6713eb2a8280e701190e44c39c2ad937b23a725861fad99694c2e` |
| `tools/unix-exec-validation/driver.py` | `04acaf36ec1940f0d83acd824c57ae8b344d6011` | 80888 | `b3f807720d58947f23014da34f1c203d3c8633819a12695aa18fa908bd142f2d` |
| `tools/unix-exec-validation/test_driver.py` | `20831ca8138a09d0aadaf4d0fe5690d4fbd33094` | 113581 | `bf24ed1320489e6c13dd34163441ea530f6316f6fe73db396e6414d2d43f4f9f` |
| `tools/unix-exec-validation/mutation_check.py` | `c8632aa6f65525dbe8a485a0c5c076153a01eba5` | 48022 | `6d838247e9553e1d83cab8185d5bea6f091c8e7b122144b88f7ec95dbc724b73` |

Native workflow unchanged: blob `dbcca2276307357301719ed68a09631b8ab7a10a`.
Comparator unchanged: blob `520d341633c67dc52ec483e9b3fcc92131a3da1f`.
No affected fixed CI source hash pin was found by searches in scripts/tools/CI/Rust;
none was relaxed. Source-only compiler adapters now accept ARX_PURE_DEPS for
read-only development rlibs; generated executables remain under TEMP.

## Full source blob inventory

Complete recursive non-doc inventory at source commit `7385785c50ec28118db6d84138f7dc9e21242ec7`: every tracked
path outside docs/, with original mode/type/blob (not a selected subset of inputs).
Docs are excluded to avoid self-reference; their exact modes and delta paths are
bound separately by D7i and the reviewed-head/squash full-tree equality gate.
Coordinator must compare all these records, not just the four changed paths.

```text
100644 blob d0d6371de329ffb232ba2fb9b79732165a702be7	.gitattributes
100644 blob de7a8c79940b870926acec71eeeb7707e2f09886	.github/agent-authors.json
100644 blob 575540057aee74d979b86ff6c086bf8ae465dd09	.github/main-protection.json
100644 blob 8fe814e48a7cc5f88561c64da8eb9c0b19877d33	.github/workflows/ci.yml
100644 blob 30167418c0993d83d8d447f5f21ab3f57dd94835	.github/workflows/e3-native-evidence.yml
100644 blob 2be8e3226b0b07ce9791bbf2f63bc51863c52d3a	.github/workflows/e4-native-evidence.yml
100644 blob 1898f78e1f4f529ab575af6d5a1db3961631b6ea	.github/workflows/policy-signal.yml
100644 blob 4925b7f12bdebcd624024e64af5ce9e000743b67	.github/workflows/policy.yml
100644 blob 8b01486932f9eb7346a21f3bbf1c5aa2844c4daf	.github/workflows/s2b-native-evidence.yml
100644 blob fb4f0a5e28304e8dfc61cd3770c2ddff4f462ea3	.github/workflows/sudo-package-readonly.yml
100644 blob dbcca2276307357301719ed68a09631b8ab7a10a	.github/workflows/unix-execution-experiment.yml
100644 blob 30027d353dc7e7dd99038b7dc88ed3786b2d2aa2	.github/workflows/windows-identity-probe.yml
100644 blob 3c554ae03483f7a5d0e772215db0c29dc38d671e	.gitignore
100644 blob ca5c350055ccb6dacb495a786f60c8232abed8f2	.node-version
100644 blob 145d3fa25b8c8cdd495ab40849109408792fee0d	.npmrc
100644 blob c2ae25947916672b13180778265354907b75e295	AGENTS.md
100644 blob 483614be7811d25ae6cac786711a3f40f669f605	CONTRIBUTING.md
100644 blob 1c5c0fcf2b577f89ed998826ddf6608338b4b523	Cargo.lock
100644 blob 343f7301c8744ba3ee1b8ebfb5422c70a04479f9	Cargo.toml
100644 blob a028880c724b3ad2c8ec7a867dc9ce3bb2539c2b	LICENSE
100644 blob c338651dd7ba58c4344677f866fbe22039306286	README.md
100644 blob c100d23cd621428fa205584da64974c642c281d7	SECURITY.md
100644 blob 3ad7c587de0344adc7b401f2f1d36cca022d9309	crates/arx-guest/Cargo.toml
100644 blob fc1bbf91cb5052277a0f3752db4b89b328008e00	crates/arx-guest/src/main.rs
100644 blob 7d5212c545400f926a8fd07d49786deaaf063f04	crates/arx-guest/tests/cli.rs
100644 blob 055372c98d6e3f42a87a30725145cded1d7a3531	crates/arxd/Cargo.toml
100644 blob 0d3859e6a5e9011217819c6a8efd7d00a512c940	crates/arxd/README.md
100644 blob 87e3d6a50fc68cbecc6428cfc075bb4f708f0842	crates/arxd/src/client_config.rs
100644 blob dc83fd3fde74fe355d0709e44d4dd901914bdd52	crates/arxd/src/client_config/tests.rs
100644 blob fdc351273e641cf416591895bd50e3155581ddb7	crates/arxd/src/client_config/unix.rs
100644 blob 4e53527cf95e20c4602ae88c5dc66fd38c30abd2	crates/arxd/src/client_config/windows.rs
100644 blob 3ed5899d35cd6030e80a0be9274197834a0dee5c	crates/arxd/src/config.rs
100644 blob 03be8eea75463f52e4cd4f3855921a69ed7a68c6	crates/arxd/src/development/mod.rs
100644 blob f6962c26355fb817dc24bfd9e684f3b030f1ce92	crates/arxd/src/development/tests.rs
100644 blob 754070e807a69f7e0f20de0a8813f2aa55e469e6	crates/arxd/src/development/types.rs
100644 blob 491b1cfe9b2b23a6d1b814b51e8a6853f011b7f1	crates/arxd/src/development/windows.rs
100644 blob b01bd021d3bb1529cd50a93049cb4e9d17e73453	crates/arxd/src/exec/cgroup.rs
100644 blob 252a60449e1217a055129c4aed9d1ed7050912b5	crates/arxd/src/exec/disposable.rs
100644 blob b17a961996d0e8184da0262ea21e20d5773d484d	crates/arxd/src/exec/disposable/a4.rs
100644 blob 7f35fecc5504563c867eb6b568567cecbde533e9	crates/arxd/src/exec/disposable/sudo_nnp.rs
100644 blob e47ced34b2f516a59bfc662949040d5d65e6d2a6	crates/arxd/src/exec/hierarchy.rs
100644 blob 1c7192626056ea4aac5f63f6fea3c71ee9d408a3	crates/arxd/src/exec/hierarchy/a4.rs
100644 blob 71f064ccfb78ff06c75a180a61974d57552ea102	crates/arxd/src/exec/hierarchy/a5.rs
100644 blob 86f4e509814cae28618c84dfc65ec8e3a97b3b6a	crates/arxd/src/exec/hierarchy/fake.rs
100644 blob 7c4196333f4938499188c7f99e5476c955b8e9d0	crates/arxd/src/exec/hierarchy/linux.rs
100644 blob 145a44d4912fc28c39b0832d7decb331b64b465e	crates/arxd/src/exec/hierarchy/parse.rs
100644 blob b33bed7cd5d7022f30c039e761c544567304d28c	crates/arxd/src/exec/hierarchy/setup.rs
100644 blob 2d04f70c3310bd1a5b55d246a8123963ff02a081	crates/arxd/src/exec/mod.rs
100644 blob 1c383aafb66da08693850fe07878737eadea3bf0	crates/arxd/src/exec/output.rs
100644 blob 91c52516fba2b4b1740309c7b986759e6aa60fc4	crates/arxd/src/exec/residue_tests.rs
100644 blob 966bdaa194f5b6c6ad31cf1fec0c50f9072b95d3	crates/arxd/src/exec/stage_b_tests.rs
100644 blob c2b413036247da906d61d5691249a2a2173a2647	crates/arxd/src/exec/tests.rs
100644 blob 0d087e37f31f3e17db652a87f2a0364eb398479e	crates/arxd/src/exec/unix.rs
100644 blob 09ffc537c198eaa47063eaa41b1d8d1d0b039be1	crates/arxd/src/http.rs
100644 blob d7e1917e8f331ae3d2277b10d617d492e72fbeac	crates/arxd/src/lib.rs
100644 blob 72feb0b22335c674848c6fc9742ebf776fe4140b	crates/arxd/src/local.rs
100644 blob 0d23f272d20f516a5269e5b08b09e4f0bb229f12	crates/arxd/src/local_execution_tests.rs
100644 blob 44bdb0be221e5e3220837e65a5e0fc89df529b7b	crates/arxd/src/local_unix.rs
100644 blob b685e90a502896f87121d741622ab563a3014ef1	crates/arxd/src/local_windows.rs
100644 blob cae709bba9d106f0494fec656b356edfe7602634	crates/arxd/src/main.rs
100644 blob f83acbc8b5da33577296a797c2dbfcefd18f396f	crates/arxd/src/mcp.rs
100644 blob f92b7a2f37c59627faee2704a99004cc5fcc1e48	crates/arxd/src/mcp/activity_tests.rs
100644 blob ccf3e75733aba9ab07e25ac8032f152693be8f21	crates/arxd/src/mcp/discovery_tests.rs
100644 blob 4c911a1ad59deff98f029dccd710deb31b0a6523	crates/arxd/src/security.rs
100644 blob 7976171bd2a67d194770e71f33b3f0a347e94a18	crates/arxd/src/service/application.rs
100644 blob 541a8230b1b590a20bb338fa3ff37e8380ed0401	crates/arxd/src/service/application/tests.rs
100644 blob d396250f3cf78ffda14929e2a39102a769b504a0	crates/arxd/src/service/config.rs
100644 blob 648302034a22f5a49e76c9b198e30617f306f20b	crates/arxd/src/service/executor.rs
100644 blob 168d86fe25ec0a729d0c40a8523331e7b69c782b	crates/arxd/src/service/executor/tests.rs
100644 blob b636acea3df84e2c717ae7df3586486ffc1dfe91	crates/arxd/src/service/layout.rs
100644 blob c0488d356ca7b133d7c056c2273da2af2517a824	crates/arxd/src/service/mod.rs
100644 blob 3f9264f670cbf9b53c1e7212ae3d181a4fca4708	crates/arxd/src/service/native.rs
100644 blob acfd3c11e285440604bd6fdcbb3f8278b13dc7b0	crates/arxd/src/service/native_macos.rs
100644 blob 372a62949fe618138b599152ce300939a0f6c346	crates/arxd/src/service/native_macos_policy.rs
100644 blob fb1bb275b4e9f28e2f49648a10a43725347a9a51	crates/arxd/src/service/native_unix.rs
100644 blob 2f4b6fe21a8d0dc5676427b263c8614f31cbdb40	crates/arxd/src/service/native_windows.rs
100644 blob 933330993d4176d123fd5d87d6219e24cb342a27	crates/arxd/src/service/owner_cli.rs
100644 blob 6627ffdc89693ca467d6e2ec04dc4c432d701585	crates/arxd/src/service/owner_cli/tests.rs
100644 blob d1956de63f80a9a2138128e2d9de19f6d4c33fe1	crates/arxd/src/service/owner_install_windows.rs
100644 blob 6334aac5b9d86be697ca32d2392b08820809ebb3	crates/arxd/src/service/owner_install_windows_tests.rs
100644 blob af209f870bd9c63110a74ef8b677342aee3c5de9	crates/arxd/src/service/owner_launchd_capture.rs
100644 blob 32a36454578553d7f8256be98e837bdb3f8885a1	crates/arxd/src/service/owner_plan.rs
100644 blob 20c51c17ba24e5d3b5cecfad2af5d2b662445888	crates/arxd/src/service/owner_query.rs
100644 blob 6254f1dd7fb03682b904315a76c064624458067f	crates/arxd/src/service/owner_scm.rs
100644 blob f9a3b747bce4bf17505d2c34c61e9aea8dd4ae9c	crates/arxd/src/service/owner_scm_tests.rs
100644 blob f006648ade2270b95dd0e5d0983c8e446bc7b431	crates/arxd/src/service/owner_scm_windows.rs
100644 blob cb7e5de582436d44e8015b2b3ea92732b1cd212f	crates/arxd/src/service/owner_scm_windows_tests.rs
100644 blob 433b70381753569661aadc9c06ae1c653415e9df	crates/arxd/src/service/owner_transactions.rs
100644 blob 0f3fd140bcdda0891f0ea2e331def17609cb81c7	crates/arxd/src/service/owner_transactions_tests.rs
100644 blob bd991dd11c75f079bd3f689ec936baf8f3371084	crates/arxd/src/service/owner_unix.rs
100644 blob 4c89e9af853190f92c1f9a18bcbf80f8237fa25b	crates/arxd/src/service/owner_windows.rs
100644 blob 5fd38201a80a766c4779391f7aec70ce4bc190f7	crates/arxd/src/service/owner_windows_tests.rs
100644 blob 534625eeba3569119f1c4e49fc1ef35f1ca3f072	crates/arxd/src/service/preflight.rs
100644 blob 0b9022e8f809fa6b298564b6bd713b97394e556e	crates/arxd/src/service/tests.rs
100644 blob 89f2a922d2f5c5c630f07fb77c34bae062617578	crates/arxd/src/service/unix.rs
100644 blob 01c626dddbbb43ac43f4d332a79c15dcc1bc02d6	crates/arxd/src/service/unix_bridge.rs
100644 blob 1ff55b22f810e1907e21a2edc9c9df07be868f92	crates/arxd/src/service/unix_bridge/readiness.rs
100644 blob f5136186085e03c4225249b569e99debdad6aaa9	crates/arxd/src/service/unix_bridge/tests.rs
100644 blob 781b3b36577794c60a12175e2e843296d9e3ece8	crates/arxd/src/service/unix_bridge/tests/a6.rs
100644 blob 7514f353217fe015974228af9d92fa07cb249095	crates/arxd/src/service/unix_bridge/tests/e6.rs
100644 blob 1263f1c1bd5752c6ba981d928e4bfd36eb14919c	crates/arxd/src/service/unix_bridge/tests/e7-no-cache-golden.txt
100644 blob 0f3df557475c2a496b47de3dffe8527303cc45bf	crates/arxd/src/service/unix_bridge/tests/e7.rs
100644 blob 7e098e3dc8f9f8141c8e7f27b489eeb9731d2e8b	crates/arxd/src/service/unix_bridge/tests/s2b.rs
100644 blob 8059646740cb4b3479c22420e750a5daaa98e355	crates/arxd/src/service/windows.rs
100644 blob f223e99c6bdb57569557e384bec828cf5c40a3ab	crates/arxd/src/store.rs
100644 blob 84531963eeae9e8b31dc8f0c83220f852e640783	crates/arxd/src/store/crash_tests.rs
100644 blob e70d0881964fdfc18f8a51b4b51875e569b374b1	crates/arxd/src/unix_shim.rs
100644 blob b269e00622d48a2bb7dfd8143e1cdec7f25b5177	crates/arxd/src/unix_shim_rights.rs
100644 blob daa2144283fea0d2b4bd859c796fb86a827b631d	crates/arxd/src/unix_shim_rights_linux.rs
100644 blob d02fd13090d0e778100a6fd9a8a8da28b5b60e92	crates/arxd/src/unix_shim_rights_linux_tests.rs
100644 blob d8d879e795965650ec7a38fee896d0b45c2b8dfb	crates/arxd/src/unix_shim_tests.rs
100644 blob f65a1b502295d7d97adb9d2714d7fc1f2ababf95	crates/arxd/tests/cli.rs
100644 blob e92ef2669e6777cede83859d383190c59aef421d	crates/arxd/tests/inventory.rs
100644 blob 40a3c40fd8879a415b0aade734cb7f2ca8312bca	crates/arxd/tests/inventory/activity_v2.rs
100644 blob 4471b3a63696fa16b0957b545bc7d2b5fcbf2063	crates/arxd/tests/service.rs
100644 blob 4108b5c47238e5ef9be9e6c23dad1351cda90c9f	crates/arxd/tests/unix_shim.rs
100644 blob d9c9c78153f07acdb18cdca3b74e62fa2f99c5c5	desktop/main.cjs
100644 blob 2974885dd37ed0e2cfe328fac2e57c9a293dcf06	desktop/package.json
100644 blob 5b1dff857e4931d1d1ffb0bea6ed5b2bbcaa97ab	desktop/playwright.config.ts
100644 blob c612e12d02eeb4ff8b1c9c9c7a7037080b2f72f2	desktop/tests/sandbox.spec.ts
100644 blob 5e17b60985b1efc8ad6598722e040b390d7b105c	eslint.config.mjs
100644 blob 8e9cff2aa19a35fe08264ad6d0c0e48f32b88c99	package-lock.json
100644 blob ed4f1214dcfced8d7814028350de959a7f4cdbe8	package.json
100644 blob fc4812328f8d0e6ebc49d50162cfa873fc30e297	packaging/README.md
100644 blob 93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74	packaging/linux/50-arxcovo-standard-rights.conf
100644 blob 0b337773c60e49fe84de76db018d2235ae076254	packaging/linux/arxd.service
100644 blob cbb41a5785ca2a0261539a2cf8d48584310af84f	packaging/macos/com.arxcovo.arxd.plist
100644 blob 68eb6494a2142383009bcb8e994e95e09dcd424d	playwright.config.ts
100644 blob 3417ee590341edc47a046d94e77ee9bee2b33bf4	rust-toolchain.toml
100644 blob 61ac6e9c78821011afc30ec119d046c7c5c10a4b	scripts/ci-budget.mjs
100644 blob 34e1dba90848c91d3318f738955b1681b1217f9a	scripts/ci-budget.test.mjs
100644 blob 2b8154b5097c331b4df55c134560857c13d5fb00	scripts/ci-workflow.test.mjs
100644 blob 7683fca9dfb722a6565253bceece8189f8db4159	scripts/enable-auto-merge.mjs
100644 blob 2ddb2aa529dceb2d8d73a7d19a8546ca5b17983e	scripts/enable-auto-merge.test.mjs
100644 blob 75c21e6d3342f73ed22c34b40a7262d7a10ae385	scripts/evaluate-budget.mjs
100644 blob df92db35466aecf9938c4f75e34b605194aaf03a	scripts/evaluate-budget.test.mjs
100644 blob 4fb2e5c7a8458a220238d8c5175d242a29c9c6e0	scripts/fixtures/ci-budget-buffer.mjs
100644 blob 3586a49bd349f882ca535967ea988a326636476f	scripts/policy.mjs
100644 blob 5720ce0b745e8e811eede997ca9a0ddeb85cabed	scripts/policy.test.mjs
100644 blob 54b0baf3bfc9a9262a1d9deb7a0b46a2bfcd5437	scripts/test-service-artifacts.py
100644 blob c88069c63f879fa96fb1191aa52f54e7b450d503	scripts/test_discovery_drain_mutations.py
100644 blob 1ac80cdf093fda548fe641ba6933824fcfb0c233	scripts/test_discovery_e7_mutations.py
100644 blob 962e9e4f0d06e84755b5a87ef3332c1e265c30a9	scripts/test_discovery_followup_mutations.py
100644 blob 4a6ecf102b68e22752b099cc4f8638e87cc1c21d	scripts/test_readiness_e6_mutations.py
100644 blob c18dd8d83ceed1806b50b0aaa46beb7e335fff13	tools/ci/.gitignore
100644 blob 2acc3dcf6aa55653378efcbd410cc9a65a1c1be0	tools/ci/README.md
100644 blob fbbb32d8c70a4b58ab705582885b28f210a04233	tools/ci/e3-native/driver.py
100644 blob 6cff8e824c687d5e4bd3cb9e07a284f40b4369af	tools/ci/e3-native/mutations.jsonl
100644 blob ead690c51d712d032c28ef5c0ca4af9467e2d482	tools/ci/e3-native/test_driver.py
100644 blob 1551ab3f6106883c3234198c766f5642fc59ce99	tools/ci/e4-native/driver.py
100644 blob 6891576ab16ee8561f984949cbd727e5083dd320	tools/ci/e4-native/mutations.jsonl
100644 blob 971a21d6477002e20330ab59a482be5ddc265ff3	tools/ci/e4-native/test_driver.py
100644 blob e0120128e2e6da7339e6f214597f9b1ad5e9a810	tools/ci/observe_dns.py
100644 blob 923b0374d28d3623c4cbb3a4d12c645721b8fe7d	tools/ci/observe_launchd_absence.py
100644 blob c7104fe1c827f992a90fa3d40668ada3455a0efa	tools/ci/observe_manager.py
100644 blob 9c86162e26b49e6341e8a5f80011f833f0ab57a9	tools/ci/s2b-native/driver.py
100644 blob 1c04b068c8a46e037580f2c25945f42d283c4696	tools/ci/s2b-native/mutations.jsonl
100644 blob 93f3bfcad6b89bcc84b31d2b1abc5f94edf35f5a	tools/ci/s2b-native/test_driver.py
100644 blob 40f9ea1ffd60ec9c31cf8507ade3000a2dca0eb4	tools/ci/sudo-package-readonly.py
100644 blob 61a204201ab7af88e621b543487049b3500478f9	tools/ci/test-sudo-package-readonly.py
100644 blob f73515ddcc550f2959ae1d8ee10abda3b916a3b4	tools/ci/test_observe_dns.py
100644 blob d78684c1e6f4be95b9787fe12610cd2372e2c2c3	tools/ci/test_observe_launchd_absence.py
100644 blob 2f76bab053e72481895ddcc55d9b9c9bf68939ff	tools/ci/test_observe_manager.py
100644 blob 1521cb29637b5afb62b779af8bac725ad4b7b42b	tools/ci/test_ubuntu_mirror.py
100644 blob e800270f2f0d714e884b9460ecc174ec25e6db46	tools/ci/ubuntu-mirror.md
100644 blob 79b583e57b313eb5c8f968b1f2d441cf548720dc	tools/ci/ubuntu_mirror.py
100644 blob c18dd8d83ceed1806b50b0aaa46beb7e335fff13	tools/unix-exec-validation/.gitignore
100644 blob 6e4b36c937c16aca2681b1ab1bc00acd8b3cda71	tools/unix-exec-validation/README.md
100644 blob 71e92a1fd2ea41385c1b23960a377bd1f547977e	tools/unix-exec-validation/a4-helper/.gitignore
100644 blob 10ab0391abde27e45558c2ceda61bf5e0ed830d0	tools/unix-exec-validation/a4-helper/README.md
100644 blob 5412b0cf345d3bd5dd0106e972a6b10e12827b7b	tools/unix-exec-validation/a4-helper/build.py
100644 blob 5a02cf43d8abaa29e221be4ac54221e2ae05354f	tools/unix-exec-validation/a4-helper/helper.rs
100644 blob 0554592aff7cc97e869f7f8a4dbf25b254aac640	tools/unix-exec-validation/a4-helper/image-pin.json
100644 blob 43e23c1b8597eead804e78cdf69c58139a788bc0	tools/unix-exec-validation/a4-helper/linux-image-pin.json
100644 blob 91ee0a9075a87447bf531d6089c08547e2767fac	tools/unix-exec-validation/a4-helper/recipe.json
100644 blob 1a9d6829e6bb2e40c379b8b3fc208b8440b4ea0c	tools/unix-exec-validation/a4-helper/test_build.py
100644 blob 27f880a5fd4881a08ab777f0e28de66fbc5cfeee	tools/unix-exec-validation/a4-helper/tests.rs
100644 blob 04acaf36ec1940f0d83acd824c57ae8b344d6011	tools/unix-exec-validation/driver.py
100644 blob c8632aa6f65525dbe8a485a0c5c076153a01eba5	tools/unix-exec-validation/mutation_check.py
100644 blob 41ec1325a77669d34c20f00c4b04d40c9d8086a4	tools/unix-exec-validation/sudo_host_qualification.py
100644 blob 520d341633c67dc52ec483e9b3fcc92131a3da1f	tools/unix-exec-validation/sudo_package_profile.py
100644 blob 20831ca8138a09d0aadaf4d0fe5690d4fbd33094	tools/unix-exec-validation/test_driver.py
100644 blob ce2357d1e3c14013f13eac52d72c8e5983814b39	tools/unix-exec-validation/test_f0_blockers.py
100644 blob 294568ab0cadaec82a7497f9a29858d605ccff04	tools/unix-exec-validation/test_f0b_transport.py
100644 blob ee0967be3ef09a256fc4867f81bcab0b9af2c7a4	tools/unix-exec-validation/test_sudo_host_qualification.py
100644 blob 00dbeb7b637df485492168e9199139f1dca85db1	tools/unix-exec-validation/test_sudo_package_profile.py
100644 blob cd42ee34e873b4ddf17cdeeed2e6c0d4e094c62b	tools/windows-identity-probe/.gitignore
100644 blob c62acaeac5f111acaaa1c4de17745e9a214b242d	tools/windows-identity-probe/CiValidation.psm1
100644 blob 4014b7d74ab0f7345f0264397aeba17ac6d15241	tools/windows-identity-probe/DEBUGGING.md
100644 blob 87dbc89ac7d56e08fd998d65df7eca3056f9eac5	tools/windows-identity-probe/IdentityProbe.csproj
100644 blob f1d923809bf9e6ab80ce6f34c6d2472f18385cb7	tools/windows-identity-probe/Invoke-DisposableCi.ps1
100644 blob 612d007b4a704c19b0abe2c0e173e8a06f9e92ce	tools/windows-identity-probe/Native.cs
100644 blob 50cbdaef70ede61f22ec442fbb0c4ba8d64ee3fb	tools/windows-identity-probe/ProbeBatchRight.cs
100644 blob e75f03dae5bb5c8087c981da378ac9b165040393	tools/windows-identity-probe/Program.cs
100644 blob 797ef395ceca74ef56cff8e6b8b8aa7430f588b0	tools/windows-identity-probe/Test-CiValidation.ps1
100644 blob d4d3537d21e70141d78c7d587b7bb30d3deee60d	tools/windows-identity-probe/Test-GuardMutations.ps1
100644 blob df191a56506c00ba9183762efad38283b75c7b30	tools/windows-identity-probe/Test-SetupDiagnostics.ps1
100644 blob f26127281e627d3cca76312846a18243206cd063	tools/windows-native-replay/Admission.cs
100644 blob 6c1185276c27dc71453d40a1ea2bd10bf3f52170	tools/windows-native-replay/Check-Source.py
100644 blob e1c83ed2e3701add6a9feddea9d2e94b13cbfe9a	tools/windows-native-replay/Controller.cs
100644 blob 9526c04b368664ee23e6a6efbb27e554cf5b10b9	tools/windows-native-replay/HeldFiles.cs
100644 blob a4c88570d6012ae356c9b4f1e71b2f0d0d76acba	tools/windows-native-replay/Invoke-BoundedReplay.ps1
100644 blob d9a0fd2e78586b16c3f625cc73fe0928f932cc80	tools/windows-native-replay/NativeJob.cs
100644 blob 2cd40a0e13bb680a9d59136dd1dff0c0942f4518	tools/windows-native-replay/PINNING.md
100644 blob cbc1ddc275b5543741b7f517699d7e7a8a3120a9	tools/windows-native-replay/Prepare-Provenance.py
100644 blob 1a39056c9e5298c6d520794e843eb031d0350ced	tools/windows-native-replay/Test-PureController.ps1
100644 blob dbf05ddef24026e55c6e87b9a38a7c7899412b9d	tools/windows-native-replay/child/Cargo.lock
100644 blob 6d36690a7b1d4406339de7b63d753de4db44a091	tools/windows-native-replay/child/Cargo.toml
100644 blob cd5b7c59da062c25118665b2f164769265545e7c	tools/windows-native-replay/child/src/main.rs
100644 blob 41a9499660e8b0b40b21bd021f56ba45d760474a	tools/windows-native-replay/selectors.json
100644 blob 20ab11d2a18157132247baa727940661c2b592ca	ui/index.html
100644 blob 1799eb63a4394d847182abeda9fab5875909d41b	ui/package.json
100644 blob 78ddaccc4262c91ae9db53ab7b04d312875252c9	ui/src/App.test.tsx
100644 blob f6dc928d2bb381dd2662dab5084236e0ef5d2bc3	ui/src/App.tsx
100644 blob 1c773831afd7ba399ce6d31fa71fdecedcc2ebfe	ui/src/main.tsx
100644 blob 903b1d97e221e4f42900ac4e02b02851b114a652	ui/tests/smoke.spec.ts
100644 blob b212ae2d17ec1ec67ad3fd41b3514573c27bcd7c	ui/tsconfig.json
100644 blob 041854f74501ecbb719e93f24f73e471596ab572	ui/vite.config.ts
100644 blob b916fd8d80a9ae1434207a621d8cfc15291c271a	ui/vitest.config.ts
```

## Coordinator correction after independent review C1

Independent Muse review6b225a3 retainedb635e13 passes Standards/Spec and requires C1 before dispatch. `_stable_stat` now compares dev/ino/mode/uid/gid/nlink/size/mtime_ns/ctime_ns, excluding atime for link, alias directory and C2 stability. Fake readlink and digest ports update atime between copied snapshots; all seven RuntimeAdmission tests pass. Whole-stat in-memory counterfactual fails the alias regression as expected; all six existing admission assertion mutants are killed. Non-atime field regression checks all nine fields. No actual host/adapter/native read occurred. D7i allowed review filename corrected to the actual retained candidate review.

The preceding source table records builder7385785 historically. This exact four-source inventory supersedes it for the corrected publication candidate; all other baseline non-doc blobs/modes remain unchanged:

| Path | Git blob | SHA256 |
| --- | --- | --- |
| `crates/arxd/src/exec/disposable/sudo_nnp.rs` | `7f35fecc5504563c867eb6b568567cecbde533e9` | `0bcddcd6bce6713eb2a8280e701190e44c39c2ad937b23a725861fad99694c2e` |
| `tools/unix-exec-validation/driver.py` | `79ea9be6ad554e64c10e88bc5265655ccf4b3a59` | `978eb656973f0ff010bb79fffdf63fdbd0d7ae40d2914e33295650898bea3d96` |
| `tools/unix-exec-validation/test_driver.py` | `e61ed0864a245719e8add3820ec2904813985d3d` | `a670bc80b85ae56ac33ae0b2b3537b68e96fb0b5fa74ba54051d96a2d3f8e11c` |
| `tools/unix-exec-validation/mutation_check.py` | `c8632aa6f65525dbe8a485a0c5c076153a01eba5` | `6d838247e9553e1d83cab8185d5bea6f091c8e7b122144b88f7ec95dbc724b73` |

Validation: seven focused mock tests, whole-stat refusal counterfactual, six admission assertion kills, UTF8LF and diff checks. Builder broader evidence is reused only for unchanged behavior. Fresh exact-head independent delta review, hosted Linux compilation, all five gates and coordinator merge remain required; no run authority or native acceptance granted.
