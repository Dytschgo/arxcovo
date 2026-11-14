# Grok source review: Stage F A1/A2

2026-11-14; Grok (grok-4.7); single-agent source and security review. This
reviewer did not write the implementation. Both axes were read in this pass
because the task forbade further reviewers. No driver CLI, ignored fixture,
native adapter, proc, systemd, sudo, account, service, or network operation
was run.

Source publication for an ordinary hosted compile only. This does not authorize
a manual run, a native acceptance, or closure of M2, Stage F, Stage G, or
real-Runner PA6. Hardware remains **unverified on real hardware**. Optional
diagnostic tracks stay parked.

## Pins

- Clean isolated top: `t3/review-a1a2-source-security`, HEAD
  `f3128ebf6896cabba530d40ee0ed21f6c29d8704`. cwd matched Git top-level.
  Status was clean before this review file.
- Supplied head contains retained source `3e967be69e0758e26726e554b48011178132ecb6`
  (tree-identical to `7fa9129e22df58218fcb7c083120f168ddfbfd24`, same parent
  `be5bed8`) and retained evidence `d374860d5a49f158257cce239b0721d1e721fb1b`
  (tree-identical to `7295dc88c1a0f72a7ea2240db8d2c50d4b9fc4c6`). Progress
  `f3128eb` is docs-only on top of that evidence. HEAD differs from `7fa9129`
  only in `docs/PROGRESS.md` and the evidence note.
- Code base is actual main `625eeb062bd89c1b2692b2cc99759a809e69e849`.
  `git diff 625eeb0 HEAD` is the seven files below plus the Stage F docs.
  Protected blobs match main, the source commit, and HEAD:
  unit `0b337773c60e49fe84de76db018d2235ae076254`,
  drop-in `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`,
  workflow `e5f1a96dbfa522bda602db45bf24db000ebddd5c`,
  setup `b33bed7cd5d7022f30c039e761c544567304d28c`,
  Linux port `ae26fad095242b2d01e48e84e8695261e295cad3`,
  parser `54dd52043e863d12189fda7216da43e095c96ffb`.
- Driver blob `c0f4f9ff487878293ab31f5c74ed16f1f842cda8`. Content SHA-256
  `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`
  (worktree equals blob, LF). The evidence note's
  `bec6460b9609339fe54ff01df9e13591c3eeed00706e303af07a176c4a5259c2` does not
  match. Do not cite that fingerprint.
- Corrected proposal is HEAD
  `docs/reviews/m2-stagef-a1a2-alignment-proposal.md`. Retained design review
  `7e0b01f417583981f80607a8bc31b3aff880a477` is tree-identical to `43656c2`.
- Seven-file delta: `tools/unix-exec-validation/{driver.py,test_driver.py,mutation_check.py,test_f0_blockers.py,test_f0b_transport.py}`,
  `crates/arxd/src/exec/disposable.rs`,
  `crates/arxd/src/exec/hierarchy/fake.rs`.

## Checks actually run

- `python tools/unix-exec-validation/mutation_check.py`: 58 tests OK;
  11 driver mutants and 4 Rust orchestration mutants killed by named
  assertions; zero error credit; source fingerprint unchanged. Rust stdin
  harness reported `3 passed` before the mutants.
- `python tools/unix-exec-validation/test_f0_blockers.py --mutants`: 23 kills.
- `python tools/unix-exec-validation/test_f0b_transport.py --mutants`: 36 kills,
  4 equivalents, no error credit. Internal maximum 757; outer maximum 2447.
- `cargo test -p arxd --lib --locked --offline exec::hierarchy::fake::` with
  `--skip h15_panic_payload_subprocess --test-threads=1`: 26 passed.
  `exec::hierarchy::parse::`: 11 passed. H15 was not run.
- Not run: Linux `cargo check`/`clippy` (cross C compiler absent; nothing
  installed), fmt, driver CLI, ignored fixture, native adapter, proc, cgroup,
  systemd, sudo.

`mutation_check.safe_suite()` is Guards except `test_cli_wrong_platform_refuses`,
A1A2, CgroupScan, F0Blockers, and F0b Transport/Records/EntryRun. Those paths
mock walk, lstat, `os`, subprocess, and unit commands. RuntimeGuards,
CopyGuards, the driver CLI, and native filesystem calls are outside that suite.

## Standards

Pass. The delta stays inside the accepted driver, Python tests, mutation
runner, ignored fixture, and fake-port tests. Packaging, workflow, production
setup, parser, and Linux port are byte-identical. The new test is `#[ignore]`.
Normal CI (`cargo nextest run`, no `--ignored`) does not select it or the
legacy cross-account test. F0/F0b site tokens, precedence, and 1024/4096 bounds
are unchanged. The reachable-site assertion now subtracts `service_cg` and
`service_runs` because this path no longer calls `protected` there; the closed
token list still contains them. No baseline smell is worth a change.

## Spec

Pass. The prebuilt ignored test is `exec::disposable::disposable_linux_a1_a2`.
Its argv is `[artifact, that test, --ignored, --exact, --test-threads=1, --color=never]`.
`cargo test --lib --locked --no-run` finishes before any unit file. Exec is
`os.execve` of that artifact with only the four disposable gates plus
`NOTIFY_SOCKET`. No Cargo, timeout, or Python monitor remains in the unit.

The fixed runtime unit is packaged `arxd.service` plus the standard drop-in.
The only fixture drop-in is `ExecStart=` reset, the staged launcher,
`Restart=no`, and `RuntimeMaxSec=140`. It does not set `StandardOutput=` or
`StandardError=`. The launcher alone creates `test-output` with `O_EXCL` and
`O_NOFOLLOW`, dup2s both descriptors, and execs. Outer compares MainPID,
`/proc/<pid>/exe`, and `0::/system.slice/arxd.service/supervisor` while the
test is held, writes one private ack, then requires `test … ok`, `1 passed;
0 failed`, `Result=success`, and `ExecMainStatus=0`. Over 1 MiB raises in the
monitor; `run_body` then stops only the unit this run created.

`prepare_a1a2` calls production `setup` before the fixture sends `READY=1`.
Fake tests refuse a missing or extra supervisor PID, a non-`arxd.service`
path, bad delegation, identity, and hardening before writes, and they refuse
release on cancel, kill timeout, and mode `0755`. Failed prepare never reaches
notify. No account, workload, Runner, or A13 path was added. Legacy
`disposable_linux_cross_account` stays ignored and unselected, with its
cleanup guards still present and uncalled from the A1/A2 finally.

Collision handling matches the fixed name. `O_EXCL` plus nonblocking `flock`
reserves one lock. A loaded unit, alias, search-path fragment, drop-in,
`/run/arxcovo`, or cgroup name refuses before install. That failure does not
stop or delete a unit. Cleanup stops `arxd.service` only after the owned
inodes and `FragmentPath=/run/systemd/system/arxd.service` match, then requires
the cgroup to be gone before removing those files. Uncertain install or
cleanup keeps the lock. Stop timeout is 40 seconds. `Restart=no` is the
runtime override of packaged `Restart=on-failure`.

## Security

Pass. Root, `NoNewPrivileges`, `CapBnd=00000000000000c1`, `ProtectSystem=full`,
`ProtectHome=yes`, `PrivateTmp=yes`, `Delegate=memory pids`, and
`DelegateSubgroup=supervisor` stay the packaged bytes. The manual workflow's
actor, ref, `workflow_dispatch`, and reviewed-SHA gates are unchanged.
`NEW_SHA` is not a bypass. Run `43091339273` is not reused. The driver does
not adopt, reset, or delete an existing `arxd.service`. Partial install
quarantines instead of stopping a unit it did not finish creating.

Linux-only `disposable.rs` was inspected against the crate APIs it calls
(`LinuxFs::system`, `prepare_a1a2`, `setup::Clock`, `Notify::send_to`,
`OpenOptionsExt`, `MetadataExt`). No signature or visibility mismatch showed
up. This Windows `cargo test` did not compile that file. Hosted
`cargo clippy --all-targets` and nextest remain the ordinary compile proof.

## Advisories

- Cite driver content SHA-256 `a0a1a975520fe4d23e32aaf9f8b5e27d9af99b1808413c01b7a73aeaf5426752`,
  not the evidence note's `bec6460b`.
- The outer cgroup check is the literal system.slice line. `locate` accepts
  other slice depths; another slice fails closed. ubuntu-24.04 system units
  use system.slice.
- `service_cg` and `service_runs` remain in the closed site list and cannot be
  produced by this path. That is bookkeeping, not a new authority.

## Verdicts

- **Blockers:** none.
- **Standards: pass.** 0 findings.
- **Spec: pass.** 0 findings.
- **Security: pass.** 0 findings.
- **Source publication:** ready for ordinary hosted compile and the normal
  different-provider review gates. Not a manual-run approval and not M2,
  Stage F, Stage G, or PA6 acceptance.

Standards 0. Spec 0. Security 0.
