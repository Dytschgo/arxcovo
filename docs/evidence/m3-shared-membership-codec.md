# M3 L2-A shared membership-v1 codec evidence

Sol 6.1 high implementation, no children, 2026-11-17. Assigned isolated worktree;
required unpublished base `9ddc1212deef0ea641f6707b8ffb23db73a660d1`.
Contract/root review `e22dc933634fc7be8d391f8d749898d7578f5a4c` read with root
clarifications and gate update. Root-supplied exact main
`fa7e3c019484bf2335bd26fbbafd2d65b207f480`, CI44090513191 attempt1 SUCCESS,
604s wall/Windows600s: not queried or re-executed by this builder.

Source commit: `d4694d575f186541c5d6338230ea1287b3e252e9`.

## Scope and result

Extracted private validated Membership and unit MembershipError, immutable
accessors, consuming parts and private checked reader into std-only membership.rs.
Consensus converts OpenRaft parts, bounds counts/address sizes before cloning,
and maps failures to StoreError::Membership. Module binding is pub(crate).
No acceptance/schema/command/state/log/PKI/enrollment/transport/control change.
Duplicate/overlapping/reverse configs, learners, full nonzero u64 IDs, old ASCII
address syntax, envelope4096 and canonical maximum3605 retained.
No architectural deviation. All old consensus test bytes remain an exact prefix;
all consensus code from to_stored onward remains exact. The source commit changes
only the four authorized source paths. PROGRESS.md remains coordinator-owned.

## Actual verification

One bounded natural round, Windows host. No engine/native/cluster test executed.
Environment: CARGO_NET_OFFLINE=true, CARGO_PROFILE_DEV_DEBUG=0,
CARGO_PROFILE_TEST_DEBUG=0. Every cargo dependency command used --locked --offline.
Task-owned CARGO_TARGET_DIR=target/l2a-9ddc1212-20261117/build;
TEMP and TMP=target/l2a-9ddc1212-20261117/tmp. No existing cache deleted.
All command subprocesses were awaited and exited; no owned service was launched.
Logs, scripts, raw JSON, baseline and full mutant patches remain in
`target/l2a-9ddc1212-20261117` in the assigned worktree.

Commands, exits and elapsed wall seconds:

```text
fmt exit=0 seconds=0.8897708
clippy exit=101 seconds=49.0618088
clippy-correction1 exit=0 seconds=5.6430914
focused-list exit=0 seconds=57.859 command=['cargo', 'nextest', 'list', '-p', 'arxd', '--lib', '--locked', '--offline', '-E', 'test(/^cluster::membership::tests::/) | test(/^cluster::consensus::tests::shared_membership_codec_/) | test(=cluster::consensus::tests::maximal_codec_is_canonical_and_every_truncation_fails) | test(=cluster::consensus::tests::codec_rejects_noncanonical_and_out_of_bounds_memberships)']
focused-run exit=0 seconds=1.141 command=['cargo', 'nextest', 'run', '-p', 'arxd', '--lib', '--locked', '--offline', '-E', 'test(/^cluster::membership::tests::/) | test(/^cluster::consensus::tests::shared_membership_codec_/) | test(=cluster::consensus::tests::maximal_codec_is_canonical_and_every_truncation_fails) | test(=cluster::consensus::tests::codec_rejects_noncanonical_and_out_of_bounds_memberships)']
guard exit=0 seconds=0.328 command=['python', '-I', '-m', 'unittest', 'discover', '-s', 'tools/ci', '-p', 'test_windows_cluster_quarantine.py']
sampler exit=0 seconds=0.391 command=['python', '-I', '-m', 'unittest', 'discover', '-s', 'tools/ci', '-p', 'test_windows_diag_sample.py']
inventory exit=0 seconds=13.234 command=['cargo', 'nextest', 'list', '-p', 'arxd', '--locked', '--offline', '--message-format', 'json']
```

Fmt passed before the import correction. Initial Clippy red was E0433 at unchanged
consensus/found.rs:202: inherited BTreeMap import became test-only. Mechanical
correction1 restored production BTreeMap and kept only BTreeSet under cfg(test).
Clippy then passed all workspace targets with -D warnings before any behavior
run. No second correction; no algorithm/acceptance/limit repair. No final fmt
rerun claimed after that import-only correction.

Selection was LISTED and inspected before RUN: exactly11 = new pure7 + new
synchronous wrappers2 + unchanged old pure codec2. Exact nextest expression:

```text
test(/^cluster::membership::tests::/) | test(/^cluster::consensus::tests::shared_membership_codec_/) | test(=cluster::consensus::tests::maximal_codec_is_canonical_and_every_truncation_fails) | test(=cluster::consensus::tests::codec_rejects_noncanonical_and_out_of_bounds_memberships)
```

Natural result:

```text
    Finished `test` profile [unoptimized] target(s) in 0.40s
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
 Nextest run ID ab0a3d2f-6280-4f16-9288-7601e48fe9a0 with nextest profile: default
    Starting 11 tests across 1 binary (601 tests skipped)
        PASS [   0.028s] ( 1/11) arxd cluster::consensus::tests::codec_rejects_noncanonical_and_out_of_bounds_memberships
        PASS [   0.032s] ( 2/11) arxd cluster::membership::tests::constructor_domain_limits
        PASS [   0.038s] ( 3/11) arxd cluster::membership::tests::well_formed_bad_wire_fields
        PASS [   0.049s] ( 4/11) arxd cluster::membership::tests::literal_u_encode_preserves_learner
        PASS [   0.056s] ( 5/11) arxd cluster::membership::tests::literal_joint_and_ordered_domain
        PASS [   0.066s] ( 6/11) arxd cluster::membership::tests::constructor_preserves_identical_configs
        PASS [   0.077s] ( 7/11) arxd cluster::membership::tests::constructor_refuses_well_formed_invalid_character
        PASS [   0.084s] ( 8/11) arxd cluster::consensus::tests::shared_membership_codec_error_normalization
        PASS [   0.088s] ( 9/11) arxd cluster::consensus::tests::shared_membership_codec_literal_vectors
        PASS [   0.119s] (10/11) arxd cluster::membership::tests::maximal_and_every_truncation
        PASS [   0.144s] (11/11) arxd cluster::consensus::tests::maximal_codec_is_canonical_and_every_truncation_fails
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
     Summary [   0.145s] 11 tests run: 11 passed, 601 skipped
```

Static suites:

```text
.....
----------------------------------------------------------------------
Ran 5 tests in 0.153s

OK
............................................................
----------------------------------------------------------------------
Ran 60 tests in 0.170s

OK
```

Nextest full listing only: protected quarantine28/sample8/founder13/requiredTLS14
and joint ignored classification verified from JSON; founder13/TLS14 unignored.
Sample8 compared with unchanged sampler and guard identifiers; no ignored cases
executed and no ignored-selection option used. Full listing total includes
unrelated tests; no inference that all ignored tests are the protected28.
Protected inventory:

```json
{
  "quarantine28": [
    "cluster::consensus::three_node::t3_snapshot_catchup_then_consecutive_append",
    "cluster::consensus::three_node::t1_follower_offline_enrollment_and_reopen",
    "cluster::consensus::three_node::t2_leader_offline_reads_and_unknown_retry",
    "cluster::consensus::three_node::u6_cancelled_callers_keep_engine_permits_until_resolution",
    "cluster::consensus::three_node::t4_registry_revocation_snapshot_and_prune",
    "cluster::consensus::three_node::r6_shutdown_report_cancelled_proposal",
    "cluster::consensus::three_node::r6_shutdown_report_store_only",
    "cluster::consensus::three_node::r6_shutdown_report_normal_and_reaped_panic",
    "cluster::consensus::three_node::r1_apply_wait_deadline",
    "cluster::consensus::three_node::r2_isolated_read_refusal",
    "cluster::consensus::three_node::r3_store_wait_deadline_and_heartbeat",
    "cluster::consensus::three_node::r4_read_admission_and_caller_abort",
    "cluster::consensus::three_node::r5_shutdown_read_owners",
    "cluster::consensus::issue::issue_tests::i1_happy_retry",
    "cluster::consensus::issue::issue_tests::i2_follower_no_append",
    "cluster::consensus::issue::issue_tests::i3_single_consumption_race",
    "cluster::consensus::issue::issue_tests::i4_leadership_loss",
    "cluster::consensus::issue::issue_tests::i5_remove_between_guards",
    "cluster::consensus::issue::issue_tests::i6_actual_post_wait_clock",
    "cluster::consensus::issue::issue_tests::i7_adapter_rejects_forged_shapes_and_mismatches",
    "cluster::consensus::issue::issue_tests::i8_removed_leaf_remains_chain_valid_stale_registry",
    "cluster::consensus::issue::issue_tests::i9_private_material_never_persisted",
    "cluster::consensus::issue::issue_tests::hook_store_identity_and_admission",
    "cluster::consensus::issue::issue_tests::retained_record_is_required_even_while_member_remains_authorized",
    "cluster::consensus::issue::tls_engine_tests::enrolled_target_binding_revocation_and_stale_acceptance",
    "cluster::consensus::issue::tls_engine_tests::authenticated_sender_mismatch_and_assumed_founder_control",
    "cluster::consensus::issue::tls_engine_tests::store_lock_wait_does_not_block_runtime",
    "cluster::consensus::membership_contract_tests::interrupted_joint_exact_guarded_flatten"
  ],
  "sample8": {
    "t1_follower_offline_enrollment_and_reopen": "T1",
    "t2_leader_offline_reads_and_unknown_retry": "T2",
    "t3_snapshot_catchup_then_consecutive_append": "T3",
    "t4_registry_revocation_snapshot_and_prune": "T4",
    "u6_cancelled_callers_keep_engine_permits_until_resolution": "U6",
    "r6_shutdown_report_cancelled_proposal": "R6c",
    "r6_shutdown_report_store_only": "R6s",
    "r6_shutdown_report_normal_and_reaped_panic": "R6p"
  },
  "founder13": [
    "cluster::consensus::issue::found::found_tests::f10_drop_bounded",
    "cluster::consensus::issue::found::found_tests::f10_outer_budget",
    "cluster::consensus::issue::found::found_tests::f10_shutdown_owns_closure",
    "cluster::consensus::issue::found::found_tests::f1_happy_registry",
    "cluster::consensus::issue::found::found_tests::f2_once_and_concurrent",
    "cluster::consensus::issue::found::found_tests::f3_preinitialize_refusals",
    "cluster::consensus::issue::found::found_tests::f3b_persisted_allocation",
    "cluster::consensus::issue::found::found_tests::f4_registry_restart",
    "cluster::consensus::issue::found::found_tests::f5_joiner_two",
    "cluster::consensus::issue::found::found_tests::f6_adapters",
    "cluster::consensus::issue::found::found_tests::f7_clock_guard_order",
    "cluster::consensus::issue::found::found_tests::f8_store_privacy",
    "cluster::consensus::issue::found::found_tests::f9_post_deadline"
  ],
  "tls14": [
    "cluster::pki::tls_tests::adversarial_pair_control_can_resume",
    "cluster::pki::tls_tests::bounds_root_key_time_and_static_errors",
    "cluster::pki::tls_tests::client_certificate_chain_time_and_registry_refusals",
    "cluster::pki::tls_tests::mutual_tls_target_and_incomplete_identity",
    "cluster::pki::tls_tests::production_client_refuses_resumption_from_ticket_issuing_server",
    "cluster::pki::tls_tests::production_server_refuses_resumption_from_resuming_client",
    "cluster::pki::tls_tests::reused_configs_always_perform_full_handshakes_and_recheck_revocation",
    "cluster::pki::tls_tests::reused_configs_recheck_expiry",
    "cluster::pki::tls_tests::strict_san_der_and_self_signed",
    "cluster::pki::tls_tests::tls_signature_checks_both_sides_and_tls12_refusal",
    "cluster::consensus::issue::tls_engine_tests::live_verifier_does_not_retain_store_and_poison_refuses",
    "cluster::consensus::issue::tls_engine_tests::founder_joiner_mutual_tls_real_identities",
    "cluster::consensus::issue::tls_engine_tests::learner_catch_up_gives_joiner_own_registry",
    "cluster::consensus::issue::tls_engine_tests::tls_single_node_directory_precondition"
  ],
  "joint_ignored": "cluster::consensus::membership_contract_tests::interrupted_joint_exact_guarded_flatten"
}
```

## Mutation evidence

Exactly three probes, each once and only its new pure test. All compiled, exit100
with the designated semantic assertion, one failed test each. No unwrap or
redundant-fence credit. Preregistration was appended before applying each edit;
full source patch retained. finally restored exact baseline bytes and hash after
EACH. No natural suite replay after restoration. Raw logs retained without edits;
embedded Unicode log symbols below are escaped to keep this evidence ASCII.

```text
PREREGISTER learner test=literal_u_encode_preserves_learner assertion='literal U encode equality' command=['cargo', 'nextest', 'run', '-p', 'arxd', '--lib', '--locked', '--offline', '-E', 'test(=cluster::membership::tests::literal_u_encode_preserves_learner)'] baseline=6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3 changed=ceabd749fdfc381b3399540ad3a986384a1e90fd8dc054a446ab7e1ab9a1511f
Exact old='        b.push(self.nodes().len() as u8);\n        for (n, a) in self.nodes() {'
Exact new='        b.push(self.nodes().keys().filter(|n| self.configs().iter().any(|c| c.contains(n))).count() as u8);\n        for (n, a) in self.nodes() {\n            if !self.configs().iter().any(|c| c.contains(n)) { continue; }'
RESULT learner exit=100 seconds=9.562 designated_semantic_kill=True
RESTORED learner sha256=6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3 exact=True
PREREGISTER duplicate test=constructor_preserves_identical_configs assertion='direct constructor duplicate configs' command=['cargo', 'nextest', 'run', '-p', 'arxd', '--lib', '--locked', '--offline', '-E', 'test(=cluster::membership::tests::constructor_preserves_identical_configs)'] baseline=6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3 changed=654a5597bc0ed6e28fe1bf3fd20cbdc100422d2f8671b15d798306ea0ffdda9d
Exact old='        Ok(Self { configs, nodes })'
Exact new='        let mut configs = configs;\n        configs.dedup();\n        Ok(Self { configs, nodes })'
RESULT duplicate exit=100 seconds=11.250 designated_semantic_kill=True
RESTORED duplicate sha256=6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3 exact=True
PREREGISTER address test=constructor_refuses_well_formed_invalid_character assertion='direct constructor invalid character' command=['cargo', 'nextest', 'run', '-p', 'arxd', '--lib', '--locked', '--offline', '-E', 'test(=cluster::membership::tests::constructor_refuses_well_formed_invalid_character)'] baseline=6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3 changed=cae781c4f2914197e0b7bb321f69971885d03e3b01c5ba6683651e341c2b9935
Exact old='b"._:[]-".contains(&c)'
Exact new='b"._:[]-!".contains(&c)'
RESULT address exit=100 seconds=10.578 designated_semantic_kill=True
RESTORED address sha256=6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3 exact=True
```

### learner

```diff
--- baseline/membership.rs
+++ mutant/membership.rs
@@ -51,8 +51,9 @@
                 b.extend(n.to_be_bytes());
             }
         }
-        b.push(self.nodes().len() as u8);
+        b.push(self.nodes().keys().filter(|n| self.configs().iter().any(|c| c.contains(n))).count() as u8);
         for (n, a) in self.nodes() {
+            if !self.configs().iter().any(|c| c.contains(n)) { continue; }
             b.extend(n.to_be_bytes());
             b.push(a.len() as u8);
             b.extend(a.as_bytes());
```

```text
   Compiling arxd v0.1.0 (C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d4e98ad2-6766d0fb3dad533a\crates\arxd)
    Finished `test` profile [unoptimized] target(s) in 8.56s
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
 Nextest run ID 797c609c-edda-4ae3-9c3b-ac3ebc9c5dcc with nextest profile: default
    Starting 1 test across 1 binary (611 tests skipped)
        FAIL [   0.023s] (1/1) arxd cluster::membership::tests::literal_u_encode_preserves_learner
  stdout \xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac

    running 1 test
    test cluster::membership::tests::literal_u_encode_preserves_learner ... FAILED

    failures:

    failures:
        cluster::membership::tests::literal_u_encode_preserves_learner

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 611 filtered out; finished in 0.00s

  stderr \xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac

    thread 'cluster::membership::tests::literal_u_encode_preserves_learner' (20932) panicked at crates\arxd\src\cluster\membership.rs:165:9:
    assertion `left == right` failed: literal U encode equality
      left: [1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 97]
     right: [1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 1, 1, 97, 0, 0, 0, 0, 0, 0, 0, 2, 1, 98]
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

  Cancelling due to test failure
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
     Summary [   0.035s] 1 test run: 0 passed, 1 failed, 611 skipped
        FAIL [   0.023s] (1/1) arxd cluster::membership::tests::literal_u_encode_preserves_learner
error: test run failed
```

### duplicate

```diff
--- baseline/membership.rs
+++ mutant/membership.rs
@@ -30,6 +30,8 @@
         {
             return Err(MembershipError);
         }
+        let mut configs = configs;
+        configs.dedup();
         Ok(Self { configs, nodes })
     }
 
```

```text
   Compiling arxd v0.1.0 (C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d4e98ad2-6766d0fb3dad533a\crates\arxd)
    Finished `test` profile [unoptimized] target(s) in 10.26s
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
 Nextest run ID ef6adf31-2159-4987-9ee6-e1c029f4df04 with nextest profile: default
    Starting 1 test across 1 binary (611 tests skipped)
        FAIL [   0.023s] (1/1) arxd cluster::membership::tests::constructor_preserves_identical_configs
  stdout \xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac

    running 1 test
    test cluster::membership::tests::constructor_preserves_identical_configs ... FAILED

    failures:

    failures:
        cluster::membership::tests::constructor_preserves_identical_configs

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 611 filtered out; finished in 0.00s

  stderr \xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac

    thread 'cluster::membership::tests::constructor_preserves_identical_configs' (66696) panicked at crates\arxd\src\cluster\membership.rs:176:9:
    assertion `left == right` failed: direct constructor duplicate configs
      left: [{1}]
     right: [{1}, {1}]
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

  Cancelling due to test failure
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
     Summary [   0.034s] 1 test run: 0 passed, 1 failed, 611 skipped
        FAIL [   0.023s] (1/1) arxd cluster::membership::tests::constructor_preserves_identical_configs
error: test run failed
```

### address

```diff
--- baseline/membership.rs
+++ mutant/membership.rs
@@ -13,7 +13,7 @@
 fn valid_address(a: &str) -> bool {
     (1..=200).contains(&a.len())
         && a.bytes()
-            .all(|c| c.is_ascii_alphanumeric() || b"._:[]-".contains(&c))
+            .all(|c| c.is_ascii_alphanumeric() || b"._:[]-!".contains(&c))
 }
 
 impl Membership {
```

```text
   Compiling arxd v0.1.0 (C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d4e98ad2-6766d0fb3dad533a\crates\arxd)
    Finished `test` profile [unoptimized] target(s) in 9.62s
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
 Nextest run ID b7f2521a-07d1-4419-8a00-187a12fea0d9 with nextest profile: default
    Starting 1 test across 1 binary (611 tests skipped)
        FAIL [   0.023s] (1/1) arxd cluster::membership::tests::constructor_refuses_well_formed_invalid_character
  stdout \xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac

    running 1 test
    test cluster::membership::tests::constructor_refuses_well_formed_invalid_character ... FAILED

    failures:

    failures:
        cluster::membership::tests::constructor_refuses_well_formed_invalid_character

    test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 611 filtered out; finished in 0.00s

  stderr \xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac

    thread 'cluster::membership::tests::constructor_refuses_well_formed_invalid_character' (15032) panicked at crates\arxd\src\cluster\membership.rs:184:9:
    assertion `left == right` failed: direct constructor invalid character
      left: Ok(Membership { configs: [{1}], nodes: {1: "!"} })
     right: Err(MembershipError)
    note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace

  Cancelling due to test failure
\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac\xe2\u201d\u20ac
     Summary [   0.025s] 1 test run: 0 passed, 1 failed, 611 skipped
        FAIL [   0.023s] (1/1) arxd cluster::membership::tests::constructor_refuses_well_formed_invalid_character
error: test run failed
```

## Blob pins and reversibility

Source Git blob IDs at the source commit:

```text
100644 blob 1f5828b1cae80678ff31a4bc6ab106d6abc39d4c	crates/arxd/src/cluster.rs
100644 blob ab95f2eea891abf02b869dce56f6f8131ab0f4a5	crates/arxd/src/cluster/consensus.rs
100644 blob 7721992935651cd9a419fbadfa01c7c2ea493c0c	crates/arxd/src/cluster/consensus/tests.rs
100644 blob 78f23118696b34547affd351171ffebbe2d89107	crates/arxd/src/cluster/membership.rs
```

Frozen relevant paths (all same blobs as required base):

```text
100644 blob 5568feb19d9391ace8c6434909d4f19e9a4e36e8	.github/workflows/a7-admission-readonly.yml
100644 blob 0c7af38b33f6c127029d8312a0c9ecd92f6361da	.github/workflows/ci.yml
100644 blob 30167418c0993d83d8d447f5f21ab3f57dd94835	.github/workflows/e3-native-evidence.yml
100644 blob 2be8e3226b0b07ce9791bbf2f63bc51863c52d3a	.github/workflows/e4-native-evidence.yml
100644 blob 1898f78e1f4f529ab575af6d5a1db3961631b6ea	.github/workflows/policy-signal.yml
100644 blob 4925b7f12bdebcd624024e64af5ce9e000743b67	.github/workflows/policy.yml
100644 blob 8b01486932f9eb7346a21f3bbf1c5aa2844c4daf	.github/workflows/s2b-native-evidence.yml
100644 blob fb4f0a5e28304e8dfc61cd3770c2ddff4f462ea3	.github/workflows/sudo-package-readonly.yml
100644 blob dbcca2276307357301719ed68a09631b8ab7a10a	.github/workflows/unix-execution-experiment.yml
100644 blob 30027d353dc7e7dd99038b7dc88ed3786b2d2aa2	.github/workflows/windows-identity-probe.yml
100644 blob 6bb1f1c0840176b4411aa7b88a469c448f34d925	Cargo.lock
100644 blob 343f7301c8744ba3ee1b8ebfb5422c70a04479f9	Cargo.toml
100644 blob 181e9dce6765b1db9339c254ec2dcead9b88a0b9	crates/arxd/src/cluster/consensus/adapter.rs
100644 blob b4de3dac384a3f1d7c004d81f4472a5bb4ded20c	crates/arxd/src/cluster/consensus/found.rs
100644 blob 60c41dc5a3d0f585ef945e6e28ea878af782577a	crates/arxd/src/cluster/consensus/found_tests.rs
100644 blob 27a4b0f794c2ee8dd222da140c32124560d1d5d0	crates/arxd/src/cluster/consensus/membership_contract_tests.rs
100644 blob 0fd3ad557ad94a1f76b489bfd9c44a0c3afe7947	crates/arxd/src/cluster/consensus/three_node.rs
100644 blob 2d581e7c3695eb33d6889358b81a90a419eeddf3	crates/arxd/src/cluster/consensus/tls_engine_tests.rs
100644 blob a0c13949f7a4731721ade92cdc6fb2d2892243da	crates/arxd/src/cluster/enrollment/code.rs
100644 blob ecaeb9ffa0e614bb99da015ea4ee485b621e674b	crates/arxd/src/cluster/enrollment/codec.rs
100644 blob ccf1535e79709c929ffd14c5bfc532253dbb2736	crates/arxd/src/cluster/enrollment/registry.rs
100644 blob 4459a3a5e8535cb58aa43bdf88fbe9fbe12ef64a	crates/arxd/src/cluster/enrollment/tests.rs
100644 blob fb94696239f2ad102b8952b2db55e5cc7f608659	crates/arxd/src/cluster/log_store.rs
100644 blob 288be77634295529df6e0fb8e85fff46b9e4cb12	crates/arxd/src/cluster/pki/ca.rs
100644 blob cf007df0322fafcc53c2a7a4176c47ae22e71098	crates/arxd/src/cluster/pki/csr.rs
100644 blob ab3293c11c70ac86fcfdfd10b594c3eb37e0aea6	crates/arxd/src/cluster/pki/fixtures/p384_sha256.csr.der
100644 blob b43cd1e4c0b84ddd4955a8715fbe359ce4aeb1f0	crates/arxd/src/cluster/pki/fixtures/rsa2048_sha256.csr.der
100644 blob e531935a995344e08172760e92cd5e8fec42922e	crates/arxd/src/cluster/pki/tests.rs
100644 blob 1626658eb4277f7c7f1ad4c930bdb08af533c01f	crates/arxd/src/cluster/pki/tls_tests.rs
100644 blob c8c198f4ee784c093c339930f83c91214b617f22	crates/arxd/src/cluster/pki/verify.rs
100644 blob c739f138fcbd6f6dcdd6c530c490726daef62479	crates/arxd/src/cluster/state_store.rs
100644 blob 83b1fcd5befe5e4297c9eda740a616c45af82e85	docs/PROGRESS.md
100644 blob 3417ee590341edc47a046d94e77ee9bee2b33bf4	rust-toolchain.toml
100644 blob c18dd8d83ceed1806b50b0aaa46beb7e335fff13	tools/ci/.gitignore
100644 blob 2acc3dcf6aa55653378efcbd410cc9a65a1c1be0	tools/ci/README.md
100644 blob e0998e5ed901e70aca54343e8052fcdbd5c9607a	tools/ci/a7-admission-readonly.py
100644 blob fbbb32d8c70a4b58ab705582885b28f210a04233	tools/ci/e3-native/driver.py
100644 blob 6cff8e824c687d5e4bd3cb9e07a284f40b4369af	tools/ci/e3-native/mutations.jsonl
100644 blob ead690c51d712d032c28ef5c0ca4af9467e2d482	tools/ci/e3-native/test_driver.py
100644 blob 846fc5ce5892eb299ed1b378987c1f9e4c02c0b1	tools/ci/e4-native/driver.py
100644 blob 35675e7aaeaff387dbfd7c96ea2922e35680fbb4	tools/ci/e4-native/mutations.jsonl
100644 blob 971a21d6477002e20330ab59a482be5ddc265ff3	tools/ci/e4-native/test_driver.py
100644 blob e0120128e2e6da7339e6f214597f9b1ad5e9a810	tools/ci/observe_dns.py
100644 blob 923b0374d28d3623c4cbb3a4d12c645721b8fe7d	tools/ci/observe_launchd_absence.py
100644 blob c7104fe1c827f992a90fa3d40668ada3455a0efa	tools/ci/observe_manager.py
100644 blob 2239703768990efec7324539a71193d0ae62da2b	tools/ci/s2b-native/driver.py
100644 blob 1c04b068c8a46e037580f2c25945f42d283c4696	tools/ci/s2b-native/mutations.jsonl
100644 blob 93f3bfcad6b89bcc84b31d2b1abc5f94edf35f5a	tools/ci/s2b-native/test_driver.py
100644 blob 40f9ea1ffd60ec9c31cf8507ade3000a2dca0eb4	tools/ci/sudo-package-readonly.py
100644 blob e70649566ce39d0840df85696c6479d3ff1586ff	tools/ci/test-a7-admission-readonly.py
100644 blob 61a204201ab7af88e621b543487049b3500478f9	tools/ci/test-sudo-package-readonly.py
100644 blob f73515ddcc550f2959ae1d8ee10abda3b916a3b4	tools/ci/test_observe_dns.py
100644 blob d78684c1e6f4be95b9787fe12610cd2372e2c2c3	tools/ci/test_observe_launchd_absence.py
100644 blob 2f76bab053e72481895ddcc55d9b9c9bf68939ff	tools/ci/test_observe_manager.py
100644 blob 1521cb29637b5afb62b779af8bac725ad4b7b42b	tools/ci/test_ubuntu_mirror.py
100644 blob 266d1cbcf022f11ea325985b1797c9ffad23439c	tools/ci/test_windows_cluster_quarantine.py
100644 blob 4060b5b3dfefc7f2a461223e0a5d69ab469c7aad	tools/ci/test_windows_diag_sample.py
100644 blob e800270f2f0d714e884b9460ecc174ec25e6db46	tools/ci/ubuntu-mirror.md
100644 blob 79b583e57b313eb5c8f968b1f2d441cf548720dc	tools/ci/ubuntu_mirror.py
100644 blob f5b9f589f04d4e89bdfb1e6d4e365c94bef72917	tools/ci/windows-diag-sample.nextest.toml
100644 blob 8002788b934a06b474b0cc9db41c687ff7d0b071	tools/ci/windows_diag_sample.py
```

Original complete consensus/tests.rs byte-prefix SHA256: `0ea8b1055b47d1823e09cef5f6aef81ebb4c15bcb6a05c8e0ae856aeac2e6410`.

Baseline membership.rs SHA256: `6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3`.

Artifact SHA256 manifest (raw files retained locally):

```text
05ab6c148ea6bb0340d1920f5c5640e5a899c173488bbccb01a40c871f877468  address.log
d0994fd5083acc6b1339c321fe0bc07376a6b0e068fa4397371087a7005f8c28  address.patch
b3712324a0bfb4283af0c483fd09b2a8188864add18f651502667655bfaad1d7  clippy-correction1.log
56ce7c509aebde4a40440c1dcaf043a08b2099ae3f56de4b4b99194bd2827232  clippy.log
a3cd85c994f35fa0d81f18421b337f6a1e3762f279d0eaee1f4eca6c228ee9b9  duplicate.log
06a8c3206f5fbe6dcc396fb2bab764661627034217449819a8e558ccf1a46449  duplicate.patch
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt.log
be7e29f43e4286dbb3cd96c130f8c018d5244f1609bf0f521dafd60d9bdcb45a  focused-list.log
1ea8226a091c67289f8ce55739c34505f9e8735566390d361820b2d231ecd468  focused-run.log
1632dcc03294ad0a6f731dd28a326324cc0fd2f2f1cadadf9a9a9d2423f7c7a2  guard.log
65d2a11abb22a49e6c537eed917216f2576da643a8df90366e25e7f0000d9715  inventory.json
0f9b0d5f3c9b31f3e0b717ae4e2b5b4e52f14a61f6700468b08d3a24fb968751  inventory.log
050029a01b1340c27cbe32973228b5608cf49cb456413f6f0604f1db8c0a4598  learner.log
43a998c6822cd782b056c75739a02d0ccced71f51d58f266a8391c1c565d8864  learner.patch
6b0a0fad18f8588d920935f455e84b9957d16b8bf9a9443ca13706452e8950f3  membership.baseline.rs
475086a95e9d9f72cd6bd543471fabc74e4cae6f04e84c99c3a9706456afafa9  mutation-log.txt
e7c777b9623ecbc2eb3e4bf712af088cb3c4a03857bcc19757d5adba5b95bf17  mutations.py
44d8492ea3bb8e83ba466988ada994091940c26882a938dec25a0f866f7449ac  natural.py
2c56b0cdd7fd43cf915a7b8c78f8c1749de03648a8b7949ff831c170140a2a4b  protected-inventory.json
1ea79f6e36406d6aa7a68b4b3625a9af15a982a946982f99c2724649fe339707  results.txt
80f943be4c173197cfb1b0fe6ee2f23d39fdf5474c58a2965b24d1d672edf9f8  sampler.log
b4a70e90c0be69d704fcaa3e940f460c15417185db1304ddc16b9ebaaa6a21d2  selection.txt
3660a1e59bff224249bfa71cdb454d9afb44fc38a77ada26cb90ce4b4ad4e58f  session.txt
9affb12fc53af31208b7157e9207b9e9eb3a216639d2a41c58b612c76dae6d6d  verify.py
```

Reversibility: restore source by reverting the single source commit; evidence is
a separate documentation commit. No DB, dependency, fixture or runtime migration.
Final source is the exact restored baseline; no mutant remains. No installed
software, admin/VM actions, network fetch/push/PR/merge/watch/link,
settings/timeouts/fix-counter changes or cache deletion performed.
An initial read included git status --short (empty); no remote status query ran.

## Unrun and remaining gates

Independent exact-head Sol source review, all five hosted gates (three OS verify,
ci-time-budget and agent-review), fresh green main and feature Windows <=660s
remain coordinator work. No engine/new join/recovery acceptance claim. Linux/macOS
execution, hardware, production L2 B3+ apply-time decoding/anchor/schema/custody/
transport/control remain unrun and outside scope. Hardware paths are
**unverified on real hardware**. M2 open; M3 unchecked; Sol shared3/3 and Opus0/2
unchanged. No owner preference request or new repair allowance inferred.

Proposed root PROGRESS entry (not applied): Sol6.1 high L2-A extracted canonical
membership-v1 at source commit above: fmt passed before import-only correction1;
locked/offline workspace all-target Clippy passed after that correction; listed
focused11 passed; guard5/sampler60 passed; protected inventory unchanged;
three designated compiled semantic mutants killed once each, exact hashes
restored. Independent source/exact-head review and all hosted feature gates,
fresh main and Windows<=660 remain. No engine/join/recovery acceptance.
M2open/M3unchecked; hardware unverified on real hardware; Sol3/3 Opus0/2 unchanged.
Last merged PR171 per root; PROGRESS remains root-owned.
