# Three-node nextest resource-group repair

Sol coordinator, 2026-11-16. Exact baseline main01a42d5adfedbdbd3d6e6d35648b8adb0d76cad9. Independent Opus diagnosisde6a8f1 retainedd00a94f; Sol design review1ee25f0 precedes source/config commit0ee4d3f. No Rust, Cargo, dependency, assertion, test timeout, retry or skip changes.

## Trigger and before/after evidence

Main CI43788629298 failed Windows T2/U6 leader waits and T3/T4 write waits. Opus's scratch-only shared-flush model reproduces the exact leader-deadline signature with four concurrent three-node fixtures; at the same16ms flush latency, lower concurrency removes that signature. At the calibrated8ms setting, all68 consensus tests pass with the candidate max-one resource group. These are Opus's measured results, not coordinator reruns. The write-helper link is inferred; hosted cause remains supported, not proven. One-CPU normal-storage passes only rule out CPU starvation under that particular control.

Coordinator implementation uses exactly one nextest resource group for arxd library tests matching the anchored cluster::consensus::three_node module. All other tests retain their existing scheduling. The default-profile configuration is discovered by CI's unchanged cargo nextest command. The existing Python-guard step also runs a resource-contract test that rejects widening the group, concurrency changes, retries, skips, timeout overrides and global scheduling directives. It adds no job or dependency.

## Actual coordinator verification

Unique target D:/Temp/arxcovo-nextest-repair-root-target; TEMP/TMP on D:, existing toolchain Rust1.95.0 and cargo-nextest0.9.151, CARGO_PROFILE_DEV_DEBUG=0 as CI. No installs or cache deletion.

- `python -I -m unittest discover -s tools/ci -p test_nextest_groups.py`:2 passed. The second test exercises rejected config mutations, including max-threads2/0, broad filter, global group, retries, slow-timeout, threads-required and default-filter none.
- Existing `python -I -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`:196 passed in1.017s.
- `cargo nextest show-config test-groups --workspace --locked --offline --show-default --no-pager`: accepted config and resolves exactlyeight tests into arxd-three-node-cluster(max threads1): R6 cancelled-proposal, normal/reaped-panic and store-only; T1, T2, T3, T4; U6. The output's next section is @global, holding all other tests. No test is removed.
- `cargo nextest run -p arxd --lib --locked --offline -j4 -E 'test(/^cluster::consensus::/)'`:68 passed,0 failed,462 filtered/skipped,27.459s total. All eight grouped tests run; T2 passes7.725s, T3 1.654s, T4 2.696s, U6 0.861s. This is uninstrumented actual scheduling verification, not a hosted-latency reproduction.
- `cargo fmt --all --check` and `git diff --check`: pass.

No new Clippy run is claimed: no Rust changed. No workspace-wide nextest pass is claimed; existing unrelated local Windows owner/inventory access-denied baseline failures are untouched. Hosted Windows nextest/job duration under unchanged12-minute cap remains required evidence, as do all five required gates and independent source/exact-head review.

## Limits and stop rule

This is suite-level fix attempt3. A hosted cluster deadline or Orphaned failure stops further changes and escalates to the owner for a concrete disposition, rather than a fourth tweak. U6 Orphaned under sufficiently slow applies remains a residual risk. Passing this local run does not prove the natural hosted flake fixed. Main remains red on43788629298; no rerun to manufacture green, no merge until exact-main CI is green or explicit owner steering changes that prerequisite. A green repair PR alone is not green main.

PR149 remains held; its bounded reads do not address election/write helpers. M2 remains incomplete, A7 FINAL NO-GO and A12 consumed/no-retry are unchanged, and hardware paths remain **unverified on real hardware**.

## B1 guard failure propagation

Opus862fbcaa (retained32dde4b) identifies that a Windows PowerShell multiline step could mask an early native-command failure with a later success. Sole workflow correction f63536e joins the existing and new Python guard commands with &&, preserving short-circuit failure propagation under the supported shells. The scheduling config is unchanged; this CI-reporting correction is not a fourth suite-fix attempt.

Coordinator actual PowerShell subprocess control runs a first Python process exiting7 followed by a sentinel command behind &&. PowerShell returns nonzero1 and the sentinel never runs. An initial verification assertion incorrectly required preserving exact exit7; it failed because PowerShell normalizes the chain failure to1. The corrected control asserts the actual CI contract (nonzero, second command not executed) and passes. The real chained guard command passes196 existing tests and2 resource-contract tests. Diff check passes. Fresh independent narrow source recheck remains required; prior68-test/config mapping evidence stays applicable because those inputs did not change. Main remains red; nothing is merged or rerun.
