# A12 residue producer source preparation

OpenAI / gpt-6.1-sol / high; parent `f7697f2` atop main `07d8869`.
Source only. A7/A11 hosted NO-GO remains closed, pending dedicated host.
No retry, diagnosis, grant, host reuse, rights query or acceptance credit.

## Call graph audit (before all verification)

Read AGENTS/KICKOFF/PROGRESS, brief 0020 A12, ADRs 0007/0009/0017/0020,
existing A1/A2, A5 and A4 fixtures and the retained
`docs/evidence/m2-normal-cancel-residue-join.md` consumer-only evidence.

- Native producer, read only: `exec::Runner::run_phased` validates the fixed
  spec, then `run_core -> unix::run -> Group::create`. The child joins its
  freshly created leaf before dropping identity/exec. A successful result
  includes child-observed identity, stop reason and cleanup/reap confirmation.
  `ChildGuard::kill_tree -> Group::kill_and_confirm` removes the leaf;
  `Group::drop` retries cleanup and monotonically latches uncertainty.
- New dormant fixture: `exec::a12::native::disposable_linux_a12` first calls
  `prepared_delivery`, which returns literal `None`. Thus even explicit ignored
  selection refuses before environment, UID, filesystem, account or setup work.
  No driver mapping or selection has been added; actual driver TEST stays A7.
- Behind that closed gate: identical fresh disposable hosted/root gates;
  existing A5 preflight and literal runs absence before `prepare_a1a2`
  (setup/empty fresh runs; no rights discovery), then empty observations;
  READY then a bounded identity ACK
  before any Runner child. Fixed `/usr/bin/true` and `/usr/bin/sleep 15` only.
- `exercise` owns exactly one scoped call. Cancellation requires a sole
  populated leaf with a nonzero PID, expected Runner name prefix and matching
  held/reopened device+inode observations. Unknown/multiple children refuse.
  Failed observations and panics cancel before explicit join. Postconditions
  read the real Runner residue flag after join, check empty runs tasks/events
  and literal ENOENT of the held observed leaf. No fixture rmdir or signalling
  can substitute for Runner cleanup. Normal successful result itself proves
  spawned/terminal status, including a normal leaf too brief to sample.
- Portable tests call only `accepts`, fixed spec construction, and `exercise`
  with mock closures/tokens/atomics. No Runner/LinuxFs invocation. Existing W8
  inventory is read-only source inspection. No SDK consumer duplicate.
- Pin-maintenance follow-up audit: S2b's `PINS` freezes `exec/mod.rs` bytes.
  Only that SHA256 constant changes, from
  `f8d425f7a57a309b7e10c29b4609f2c0c75002a33bc1eb0c00e04cd47dd3f9b4` to
  `c46c69bf36b3e3225f94e6fafe732771971244c6716e9aa617e17f7fbfa3c90e`.
  Its exact in-memory applicability test calls `load_mutations`, bounded source
  reads, `apply` on bytes, and command construction only. No driver main,
  execute, subprocess or native path is invoked. Other constants, mutations,
  fences, workflow and test selection remain unchanged.

Budget: fixed 15s Runner timeout/sleep, 5s population proof, <=25s whole call,
<=15s preparation, 5s identity ACK, <=90s total assertion. Packaged 30s startup,
120s monitor and RuntimeMaxSec=140 remain unchanged. Scoped join preserves
ownership even on stalls; elapsed assertions are rejection checks, **not** a
hard OS deadline or an orphan-thread workaround. No timeout was increased.

## Remaining gap and separate run grant

The A12 delivery reader intentionally remains `None`. No A7 sudo/profile/view,
bind, package, policy, helper or driver logic is borrowed or changed. A later
reviewed delivery must pin the exclusive owned stage, fresh account/group,
workspace/protected root, artifact identities, exact unit/runs path and owned
cleanup; reject prior resources/reparse points and quarantine collisions.
That preparation and dedicated fresh-host selection/qualification require their
own review and a separate explicit run grant. This file grants neither.

Actual Linux fixture compilation awaits hosted Linux; no native fixture is run.
A12, A7/A11, M2, full F/G and PA6 remain **unverified on real hardware**.
Fresh independent Opus review remains with the coordinator. M3 is outside this
bounded implementation assignment and may proceed independently per steering.

## Verification and source identity

All build outputs, TEMP and TMP use `D:/Code/arxcovo-a12-source/`.

Windows checks actually run (Rust 1.95.0, locked/offline):

- `cargo fmt --all -- --check`: passed. Initial formatting invocation found an
  unsafe-expression comparison parse error, corrected before compilation.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  passed, final run 3.17s (initial build 35.71s).
- `cargo test --locked --offline -p arxd --lib <exact-name> -- --exact`:
  each passed 1 test, 0 failed, 411 filtered out:
  - `exec::a12::a12_acceptance_counterfactuals` (0.00s; initial build 48.69s).
  - `exec::a12::a12_owned_call_mock_counterfactuals` (0.04s).
  - `exec::a12::a12_fixed_targets_and_closed_source_gate` (0.00s).
  - `exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory` (0.10s).
- Counterfactuals reject cleanup false, residue true, wrong UID/GID/root flag,
  boundary, reason and exit status, not-started/spawn-failed/after-spawn errors,
  absent proof, missing join, residual leaf/task observation, >25s elapsed,
  inode mismatch, unowned/unpopulated/zero-PID proof, observation error/panic
  and worker panic. Mock postconditions assert worker completion after join.
- `git diff --check`: passed.
- `python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py
  -k test_pinned_manifest_applicability_in_memory_only`: passed 1 test (0.015s),
  no subprocess or native entry. This checks every frozen source hash and both
  existing mutations in memory after the single constant refresh.

Linux-only native code is parsed by rustfmt but is not compiled on Windows.
No actual Runner/LinuxFs/proc/sys/account/root/policy/cgroup/helper/sudo,
network, dispatch, variables, native ignored test, delivery, push, PR, merge
or watch was invoked. No install, cache/DB deletion or owner admin step.

Source blob identities (Git SHA1; all mode 100644):

| Exact path | Blob |
| --- | --- |
| `crates/arxd/src/exec/disposable/a12.rs` | `d40310460c0cbd86c3e455f315ee00843c39b203` |
| `crates/arxd/src/exec/mod.rs` | `c310de1c3ea2426b46fb2ab608a1a659e144dc56` |
| `crates/arxd/src/exec/stage_b_tests.rs` | `ff20039925c40835a02a05147e5e10656bcbe0a1` |
| `tools/ci/s2b-native/driver.py` | `d71142697e6542c236603ac6ac7949605e4c5f5a` |

The only `exec/mod.rs` change is the four-line test-only path declaration;
the only inactive-inventory change is the new source path and exact declaration.
PROGRESS, ADRs, Cargo and production behavior are unchanged by this slice.
