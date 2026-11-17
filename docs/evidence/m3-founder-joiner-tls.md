# Founder/joiner real-identity mutual TLS: local evidence

2026-11-17. Provider **Sol**, effort **high**, single builder, no children.
Prepared base `0bbd8e6b5a63c92a46306c91a99adf291b82f5fa`; source main
`67cc62d538905f3d2ed89a7676d1a71b9f4617ff`; source commit
`1f315bcb0b83f550ee3564e3c667f9477df11d18`.
Contract: [accepted brief](../briefs/m3-founder-joiner-tls-proof.md), including
root's N1 clarification and supplied green-main prerequisite.

## Scope and demonstrated behavior

One new required all-OS test, `founder_joiner_mutual_tls_real_identities`, uses
one real single-voter OpenRaft founder, actual `found()` and `enroll()` issuance,
two distinct CSR keys, one ephemeral CA and the founder's committed registry
for both verifier roles. It checks CA signatures, exact SPKI and SAN bindings,
different serials, correct and incorrect direct registry bindings, both TLS 1.3
peer pairs `(1, 2)` and `(2, 1)`, and wrong-target refusal. The four retained
production-built configs reject node 2 in both directions after committed
`Remove`, while founder authorization remains valid. Shutdown succeeds with
all four configs alive; retained configs then refuse `Unavailable`.
All synchronous verification and direct registry calls in the new test run in
awaited `spawn_blocking` tasks. The sole outer deadline is 20 seconds.

Only test-file imports/new test and the quarantine guard changed. Existing engine
bodies were compared unchanged against the prepared base. Guard coverage adds
required inventory, gate/inventory-drift probes, five required proof-token removals
and eight forbidden-token injections. Constructor classification, quarantine27
(8 + 5 + 11 + 3) and the original sampler8 stay unchanged. Production files,
Cargo/lock, workflows, nextest, helpers, retries and existing deadlines are frozen.

## Actual local validation

Private target and process-local TEMP/TMP: `D:/Temp/arx-f2-sol-dad95efe2d/target`
and `D:/Temp/arx-f2-sol-dad95efe2d/tmp`. No existing cache deletion, install,
admin, VM or global environment action. Logs and runner scripts are retained
in `D:/Temp/arx-f2-sol-dad95efe2d`. Timings below include compilation.

- `cargo fmt --all --check` — exit 0, 0.750 s.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings` — exit 0, 46.672 s.
- `cargo nextest run --locked --offline -p arxd -E test(founder_joiner_mutual_tls_real_identities) | test(cluster::consensus::found::found_tests::) | test(cluster::pki::tls_tests::) | test(live_verifier_does_not_retain_store_and_poison_refuses) | test(tls_single_node_directory_precondition)` — exit 0, 85.500 s.
- `python -m unittest tools/ci/test_windows_cluster_quarantine.py` — exit 0, 0.266 s.
- `cargo nextest run --locked --offline -p arxd -E test(cluster::consensus::issue::found::found_tests::)` — exit 0, 23.016 s.

The initial nextest expression omitted `issue::` in the founder module path and
therefore selected only the new test plus the existing12TLS: 13/13 PASS,
626 skipped, test execution 0.365 s, new test 0.363 s. This was a selection
mistake, not 26-test coverage. The missing13founder were subsequently run alone:
13/13 PASS, 626 skipped, execution 5.500 s. No passing test was replayed.
Actual order: fmt, Clippy, initial13TLS, guard, N1, missing13founder, N2.
There was no natural test/compiler/lint red and no source correction (0/2).
Two runner corrections: founder selection and UTF-8 console output after N1.
The latter happened after N1 had finished and exact bytes were restored; N1 was
not repeated. The guard reported four tests PASS (0.093 s internal).
No unchanged/quarantined fixture or sampler suite was executed.

All 26 unique required cases actually executed:

| Test | Seconds |
| --- | ---: |
| `cluster::consensus::issue::tls_engine_tests::tls_single_node_directory_precondition` | 0.026 |
| `cluster::pki::tls_tests::production_client_refuses_resumption_from_ticket_issuing_server` | 0.038 |
| `cluster::pki::tls_tests::reused_configs_always_perform_full_handshakes_and_recheck_revocation` | 0.049 |
| `cluster::pki::tls_tests::strict_san_der_and_self_signed` | 0.083 |
| `cluster::pki::tls_tests::production_server_refuses_resumption_from_resuming_client` | 0.094 |
| `cluster::pki::tls_tests::adversarial_pair_control_can_resume` | 0.097 |
| `cluster::pki::tls_tests::tls_signature_checks_both_sides_and_tls12_refusal` | 0.099 |
| `cluster::pki::tls_tests::mutual_tls_target_and_incomplete_identity` | 0.106 |
| `cluster::consensus::issue::tls_engine_tests::live_verifier_does_not_retain_store_and_poison_refuses` | 0.115 |
| `cluster::pki::tls_tests::client_certificate_chain_time_and_registry_refusals` | 0.116 |
| `cluster::pki::tls_tests::reused_configs_recheck_expiry` | 0.123 |
| `cluster::pki::tls_tests::bounds_root_key_time_and_static_errors` | 0.137 |
| `cluster::consensus::issue::tls_engine_tests::founder_joiner_mutual_tls_real_identities` | 0.363 |
| `cluster::consensus::issue::found::found_tests::f3b_persisted_allocation` | 0.238 |
| `cluster::consensus::issue::found::found_tests::f10_drop_bounded` | 0.269 |
| `cluster::consensus::issue::found::found_tests::f5_joiner_two` | 0.271 |
| `cluster::consensus::issue::found::found_tests::f8_store_privacy` | 0.273 |
| `cluster::consensus::issue::found::found_tests::f1_happy_registry` | 0.276 |
| `cluster::consensus::issue::found::found_tests::f6_adapters` | 0.276 |
| `cluster::consensus::issue::found::found_tests::f4_registry_restart` | 0.333 |
| `cluster::consensus::issue::found::found_tests::f7_clock_guard_order` | 0.333 |
| `cluster::consensus::issue::found::found_tests::f2_once_and_concurrent` | 0.370 |
| `cluster::consensus::issue::found::found_tests::f3_preinitialize_refusals` | 0.389 |
| `cluster::consensus::issue::found::found_tests::f10_shutdown_owns_closure` | 5.132 |
| `cluster::consensus::issue::found::found_tests::f10_outer_budget` | 5.204 |
| `cluster::consensus::issue::found::found_tests::f9_post_deadline` | 5.499 |

## Once-only negative controls and restoration

Each command was `cargo nextest run --locked --offline -p arxd -E
'test(founder_joiner_mutual_tls_real_identities)'`, selecting one test only.

- **N1**: exit 100, command wall 20.281 s. Kept Weak upgrade, store lock, poison-to-Unavailable and store release. Evaluated
the locked `authorize_peer` call but discarded only its result for `Ok(())`.
Observed failure at engine line448 (0.140 s): `Ok(())` vs `Err(KeyMismatch)`.
Step8 was not reached; no claim of an observed N1 revocation catch.
- **N2**: exit 100, command wall 17.578 s. Changed only `from_verified_certificates` extraction to constant node1 (retaining
certificate parsing). Observed failure at engine line507 (0.149 s): `(1, 1)`
vs `(1, 2)` on the first positive direction.

These observations show that this new test catches each mutant. They do not
establish that it is the only catching test. No other historical mutant was
replayed. Both production files were restored after each mutant in `finally`;
SHA-256 before, after N1, after N2 and final readback were equal:

| Frozen file | SHA-256 |
| --- | --- |
| `crates/arxd/src/cluster/consensus/issue.rs` | `3c3b59591720b1e8e649fae327cf36c46f7a3ed7d2882fcaf3e1441271f567fd` |
| `crates/arxd/src/cluster/pki/verify.rs` | `23078f4bd146611ab11fe857ff6ebc21ce9aefd9e75f5aa3a9b5789beeba401c` |

Local raw artifact hashes (UTF-8 logs; byte SHA-256):

| Artifact | SHA-256 |
| --- | --- |
| `fmt.log` | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| `clippy.log` | `8692639747598dafada58a538beb6798b88c0ffc66fa347c90a99f348290f904` |
| `nextest.log` | `6c348c3128bbf57f7156e2fd1bbfab5eba0b1937599ad6067745d01942a6a28f` |
| `guard.log` | `32ad850c0b7f7736902e3825cb85cec5238a3810f78f10eabc9adce799fcdd39` |
| `founder-missing.log` | `4648c8bf7838bd353998cad4702573ab58014f93ae5ca190fe8cfae94d336643` |
| `N1.log` | `e74904a69acfdd2cfe60bad243b12a6808610332eb11a6370cac740fa602d651` |
| `N1.patch` | `6ab1e04118751ecf7b5eb84630dd3c6c980ae74032680242c6b1fb8d4a38e1e8` |
| `N1-restore.json` | `854d82e74a61a40a729f8d94144a4d347e56d0c80ddb08a3924ed5ee0caf5188` |
| `N2.log` | `2a787acca4a243699f6e417d0db8cd6b2a616b3f845215524bf05c3c37fba4f9` |
| `N2.patch` | `03b02f11a40d4210e9adb1c7826bb7ba32faebed016f5db2a99c43e1050f387f` |
| `N2-restore.json` | `854d82e74a61a40a729f8d94144a4d347e56d0c80ddb08a3924ed5ee0caf5188` |

## Reused evidence, claims and remaining gates

Root supplied exact main CI44064366449 attempt1 GREEN, wall610 / Windows606,
Ubuntu S8 and Linux/macOS fixture27 PASS, requiredTLS PASS, sample original8
all_pass but non-gating/no acceptance. These are reused coordinator observations;
this builder did not fetch, rerun or independently verify hosted CI. Previous
mutation/fixture evidence is also reused, not re-executed here.

Claim limited to one-process in-memory TLS under one shared committed founder
registry. No joiner Raft replica, second host, socket/endpoint separation,
production CA or custody, learner/membership change, freshness policy, renewal,
LAN/Tailscale or real hardware credit. All hardware paths are **unverified on
real hardware**. No M2 acceptance; M2 open, M3 unchecked; Sol recovery3/3 and
Opus recovery0/2 unchanged. No architectural deviation or ADR change.

Local commits only: no push, PR, status/comment, watch, merge, protection change,
hosted dispatch or rerun. Independent Opus source review remains pending, then
coordinator publication needs exact-head review, fresh exact-main green and all
five normal gates, including hosted Windows verify <=660 s. None of those new
hosted gates or independent reviews was run by this builder. No children were
started. All owned build/test runner processes completed and exited.
