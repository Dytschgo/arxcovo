# Grok source review: Stage F protected staging parent

2026-11-14; Grok (grok-4.7); single-agent source and security review. This
reviewer did not write the implementation. Both axes were read in this pass
because the task forbade further reviewers. No driver CLI, native adapter,
host `stat`, provision, service, account, process, system, network, GitHub,
CI, or dispatch operation was run.

Standards: pass. Spec: pass. Security: pass. The source correction is
eligible for ordinary publication gates (exact-head review, required hosted
checks, coordinator merge). This document does not merge, dispatch, or grant
a run. A later native attempt still needs its own reviewed execution
disposition. D7a stays consumed. M2, Stage F, Stage G, and real-Runner PA6
stay open. `/var/lib` on a runner remains **unverified on real hardware**.
Optional diagnostic tracks stay parked.

## Pins

- Prepared HEAD `9b3887e658e0bd26dcb8f55acd61c7dbcdae656e` was clean before
  this review file. It is two `docs/PROGRESS.md` commits above the correction.
  Those progress commits are outside this review.
- Retained correction `1ed9e9eba5f975eef015d9410ed30231b6ac5995` is
  tree-identical to supplied Sol commit
  `46a1bf99251c9c381232670abd37179405bfab78`. Both have parent
  `2bf8fdc8f9a14b480750975f10722c8307a721bb` and tree
  `d0d66295761e32dbce72a206a9ef53e798ef319a`.
- Reviewed patch: `git diff 2bf8fdc8f9a14b480750975f10722c8307a721bb 1ed9e9eba5f975eef015d9410ed30231b6ac5995`.
  Six files:
  `docs/evidence/linux-stagef-protected-parent-fix.md`,
  `tools/unix-exec-validation/README.md`,
  `tools/unix-exec-validation/driver.py`,
  `tools/unix-exec-validation/mutation_check.py`,
  `tools/unix-exec-validation/test_driver.py`,
  `tools/unix-exec-validation/test_f0b_transport.py`.
  `git diff --check` on that range passed.
- Real record `docs/evidence/linux-stagef-a1a2-run-43622157531.json` is blob
  `c78587c87545fcf710bd9246ac3ee698d65b5689` at the parent, the correction,
  and prepared HEAD. LF SHA-256
  `eda437f17e697d5afaa2cec7a424f88b56472e91b9f18f53e57cd9d16c27d8c9`.
  Worktree bytes match the blob. The line ends in one newline.
- Driver blob `d02508eb74b931313e86e164988cd63b18ca60f1`. LF SHA-256
  `8b0e1efeada395f8dd6b03e18c23b0648e99f9c29e8349ad2c3926735d6a9d59`.
  Worktree bytes match the blob. Parent driver blob remains
  `c0f4f9ff487878293ab31f5c74ed16f1f842cda8`.
- Unchanged at parent, correction, and prepared HEAD:
  unit `packaging/linux/arxd.service`
  `0b337773c60e49fe84de76db018d2235ae076254`;
  drop-in `packaging/linux/50-arxcovo-standard-rights.conf`
  `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`;
  workflow `.github/workflows/unix-execution-experiment.yml`
  `e5f1a96dbfa522bda602db45bf24db000ebddd5c`;
  setup `crates/arxd/src/exec/hierarchy/setup.rs`
  `b33bed7cd5d7022f30c039e761c544567304d28c`;
  Linux port `crates/arxd/src/exec/hierarchy/linux.rs`
  `ae26fad095242b2d01e48e84e8695261e295cad3`;
  parser `crates/arxd/src/exec/hierarchy/parse.rs`
  `54dd52043e863d12189fda7216da43e095c96ffb`.

## Checks actually run

The documented in-memory command:

`python -B -c "import sys; sys.path.insert(0,'tools/unix-exec-validation'); import mutation_check,test_f0_blockers,test_f0b_transport; mutation_check.main(); test_f0_blockers.mutations(); test_f0b_transport.mutations()"`

Exit 0.

- `mutation_check.safe_suite()`: 62 tests, OK.
- Driver assertion kills: 14, including `restore writable parent`,
  `skip parent protection`, and `reuse staging directory`. Zero error credit.
- F0 assertion kills: 23. Zero error credit.
- F0b assertion kills: 36. Equivalents, with no kill credit:
  `writer_site_unchecked`, `writer_drops_nofollow`, `reader_ascii_dropped`,
  `reader_mode_mismatch`.
- Driver SHA-256 after those mutations remained
  `8b0e1efeada395f8dd6b03e18c23b0648e99f9c29e8349ad2c3926735d6a9d59`.
- The suite still reports internal maximum 757 and outer maximum 2447.

`safe_suite()` is Guards except `test_cli_wrong_platform_refuses`, A1A2,
CgroupScan, F0Blockers, and F0b Transport/Records/EntryRun. Walk, lstat,
`os`, subprocess, and unit commands on those paths are mocked.
`mutation_check.main()` does not call `rust_mutants()`. This diff changes no
Rust source, so those mutants were not re-run. RuntimeGuards, CopyGuards, and
the driver CLI stayed outside the suite.

## Standards

Pass. The behavior change is the fixed plan path and the matching F0b
descriptor walk, pinned by the new mocked tests and the three new assertion
mutants. README and the evidence note follow that path and keep the runner
permission caveat. Packaging, workflow, production Rust, and the native JSON
are byte-identical to the parent. `git diff --check` is clean. No baseline
smell is worth a change. Tooling already covers the mutant anchor counts and
the assertion-only kill rule.

## Spec

Pass. `plan` returns `Path('/var/lib/arx-ci-' + run), 'arxd.service'` for an
id matching `[1-9][0-9]{0,19}-[1-9][0-9]{0,19}`. For run `43622157531-1` that
is `/var/lib/arx-ci-43622157531-1`. `ARX_STAGE_PARENT` and `HOME` do not
change it. A second argument raises `TypeError`. A non-id raises `E_ID`.

`open_stage` walks `('/', 'var', 'lib', stage.name)` with
`O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC` and descriptor-relative
`dir_fd` chaining. Each component must be a root-owned directory with
`st_mode & 0o022 == 0`. The stage itself must also have `st_mode & 0o077 == 0`.
`test_stage_walk_guards` refuses a foreign uid, a symlink, and mode `0o722`
on `/`, `var`, `lib`, and the stage name, and refuses stage mode `0o710`.
The ancestor-skip mutant `('/', stage.name)` and the dropped-`O_NOFOLLOW`
mutant die by assertion.

The real-record regression is a meaningful fence. It reads
`docs/evidence/linux-stagef-a1a2-run-43622157531.json` and requires `replay`
to return `{'first': (1, 'write'), 'final': (1, 'write')}`, provenance run
`43622157531`, and mutation/cleanup `none` / `not_reached`. With mocked
lstat, `/opt` is a root-owned directory whose mode is `0o777` and every other
path is a root-owned directory whose mode is `0o755`. `protected('/opt', 'run_parent')`
produces a Refusal whose record equals the file's `first` object:
`E_PROTECTION`, `run_parent`, `initial`, ordinal 1, kind `dir`, owner zero,
write_bits 18, predecessor `['dir', true, 0]`. `protected` of the planned
parent then walks `/`, `/var`, `/var/lib` and does not refuse. Restoring the
`/opt` return value makes that second call refuse; that is how
`restore writable parent` is killed. The mock forces non-`/opt` paths safe, so
this test does not measure a runner. Hostile `/`, `/var`, and `/var/lib`
cases live in `test_new_parent_keeps_every_ancestor_guard`: link, uid 12,
group write `0o775` (write bits 16), and other write `0o757` (write bits 2).
Each refuses while mutation is still `none`, with `reserve_unit` uncalled and
with no mkdir.

On a clean parent the same helper records those three checks at mutation
`none`, then one `mkdir(mode=0o700)` of `/var/lib/arx-ci-123-1`. `reserve_unit`
is called once. The mocked mkdir raises `FileExistsError` before copy,
install, or subprocess. Cleanup stays `not_reached`. Replacing that mkdir
with `exist_ok=True` changes the recorded kwargs and kills
`reuse staging directory`. Replacing `protected(stage.parent, 'run_parent')`
with `pass` kills `skip parent protection`.

The id grammar cannot make the stage component equal `arxcovo`,
`arxcovo-build`, or `arxexec<PID>`. `fixture_cleanup` still targets only
`/var/lib/arxexec<PID>`. `shutil.rmtree` still runs only after
`stage_created`, `same_object`, and `protected(stage, 'run_cleanup')`.

## Security

Pass. The diff adds no chmod, parent override, fallback path, retry, or
relaxed ancestor predicate. `protected` still walks every ancestor with
`lstat`, refuses a link, a non-root owner, and any group or other write, and
runs before `reserve_unit` and before `mkdir`. The failed run's `/opt`
predicate remains a refusal under the regression mock.

`packaging/linux/arxd.service` and
`packaging/linux/50-arxcovo-standard-rights.conf` are the bytes above. They
declare `ProtectSystem=full`, `ProtectHome=yes`, `PrivateTmp=yes`,
`NoNewPrivileges=yes`, and
`CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN`. They declare no
`ReadWritePaths`, `ReadOnlyPaths`, `InaccessiblePaths`, `BindPaths`,
`TemporaryFileSystem`, `RootDirectory`, `ExecPaths`, `StateDirectory`, or
`User`. The fixture drop-in is still only the `ExecStart=` reset, the staged
`python3 -I` launcher, `Restart=no`, and `RuntimeMaxSec=140`. The stage path
is outside `ProtectHome` and outside `PrivateTmp`'s `/tmp` and `/var/tmp`.
The outer build and the `/home/runner` toolchain copy remain outside the
unit. That is compatibility with the declared unit text. It is not a
measurement of `/var/lib` ownership, mode, or mount flags.

The manual workflow blob is unchanged, including its actor, ref,
`workflow_dispatch`, and reviewed-SHA gates. Run `43091339273` is untouched.
No stage or unit is created by this commit. D7a remains the consumed one-run
grant. This pass authorizes publication of the source correction through the
ordinary gates only.

Reversibility: two-way door. Reverting the six-file commit restores the
`/opt` plan. No native directory was created here.

Standards: 0 findings. Spec: 0 findings. Security: 0 findings.
