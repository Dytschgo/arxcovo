# Recheck: Sol A5 seeder correction (Muse, narrow recheck)

2026-11-14; Muse; narrow recheck of the Sol correction against the retained
prior Muse review. Read-only: `git` CLI reads only; no tests, builds, native
or product CLI, process/system/service/account/owner/probe/network/GitHub,
dispatch, or subagents. No scope, code, implementation, or run permission.

## Exact pins

- Sol correction reviewed:
  `2aec267bf29865866136f9db09b2abb95f9ffd58`
  (`docs(plan): clarify bounded A5 seeder and preconditions`).
- Parent: `0f9168df6978e9b5466480e38e4b8dc15d26c0de`
  (`docs(review): independent A4/A5 janitor plan review`).
- Retained prior Muse review: `6cbc7e669d1ec0eefdaa56cdd2079b1a6dac0031`
  ([m2-stagef-a4a5-janitor-plan-muse-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\docs\reviews\m2-stagef-a4a5-janitor-plan-muse-review.md)),
  with required blockers S1–S3 plus the disposable-host gate line.
- Amended section: "Independent bounded A5 preparation" in
  [m2-stagef-a4a5-janitor-plan.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\docs\reviews\m2-stagef-a4a5-janitor-plan.md)
  (lines 84–111). Diff `0f9168d...2aec267` touches only that section
  (21 insertions, 5 deletions); the A4 section is byte-identical, so A4 stays
  open and independent with no plan change, as the prior review accepted.

Scope: A5 source preparation only. M2 A4/A5, remaining Stage F/Stage G, and
real-Runner PA6 stay open; all hardware paths **unverified on real hardware**.

## Standards

PASS, no findings. The correction is a single-concern docs-only change to one
review file: it names the seeder home, enumerates the pre-mutation port calls,
and states the host/leaf preconditions without adding any implementation,
native run, dispatch, CI, publish, owner, secret, or approval-flow content.
No smell-baseline hit: no speculative generality (one literal leaf
`arx-0-0`, no new selector API, explicit "no ... new selector API"), no
divergent change, no mysterious name. The correction narrows rather than
widens the plan.

## Spec

ACCEPT source preparation: all three prior blockers are closed in the plan
text, verified against source at the pinned base.

- **S1 (closed).** Plan homes the seeder in `hierarchy/linux.rs` as one narrow
  `#[cfg(test)]` inherent `LinuxFs` helper returning held runs/leaf `Dir`
  handles, with no raw fds and no new `CgroupFs` method. Reachability holds:
  `Dir` is `pub(crate) struct Dir(OwnedFd)` with a private tuple field
  ([linux.rs:19](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\linux.rs:19)),
  so only same-module code can touch the descriptor; the `CgroupFs` port
  ([setup.rs:48-64](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\setup.rs:48))
  exposes `mkdir_runs` (runs only) and no leaf-mkdir or fd accessor, so the
  inherent helper is the minimal access path and the "no new `CgroupFs`
  method" constraint is implementable. Descriptor-relative exclusive
  `mkdirat` for `arx-0-0` only matches the existing `mkdirat` pattern
  ([linux.rs:277-287](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\linux.rs:277)).
- **S2 (closed).** Plan enumerates the exact read-only S0–S4 port calls before
  either mkdir, failing on any mismatch: `proc_status` with PROC_MAGIC and
  `hardening == Standard`; `identity == (0,0)`; `proc_cgroup` with PROC_MAGIC
  and `locate` equal to the fixture location; `open_root`/`stat` plus each
  `open_child`/`stat` along `unit_components` (root-owned cgroup2, no
  group/other writes); `xattr_delegate == b"1"`; plus unit type/procs/
  controllers and supervisor procs (domain, no unit processes, memory/pids
  available, trusted supervisor holding only `pid()`). Each maps to a
  production-setup anchor in order
  ([setup.rs:206-248](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\setup.rs:206)):
  hardening (206–210), identity (212), locate (214–219), root walk (221–231),
  delegation (232–235), unit state via `domain`/`empty_procs`/`controllers`
  (236–239; `trusted` = uid 0, mode `& 0o022 == 0`, cgroup magic, directory
  per [setup.rs:128-135](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\setup.rs:128)),
  supervisor sole-PID check (240–248). The plan states these are read-only
  copies — not a setup call, no enable/write — and that production setup
  repeats them later, which resolves the drift concern by construction
  (duplication is intentional and the setup copy remains authoritative).
- **S3 (closed).** Plan fixes the leaf to the literal `arx-0-0` and requires
  the unchanged packaged Standard drop-in with a None-profile host ineligible.
  `arx-0-0` passes `parse::leaf_name` (`arx-` prefix, fields `0`/`0`, each a
  one-digit decimal per the no-leading-zero rule), so S7's
  `parse::leaf_name` gate accepts it. S0 requires `Hardening::Standard`
  ([setup.rs:206-210](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\setup.rs:206)),
  so the Standard-drop-in precondition is necessary and now stated.
- **Seed lifecycle (consistent).** `mkdir_runs == Created`, never adopting an
  existing directory, matches the `Created`/`Exists` distinction
  ([linux.rs:277-287](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\linux.rs:277));
  seeder-created `runs/` therefore yields `runs_created: false` ("runs
  reused") from the production setup
  ([setup.rs:252](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\setup.rs:252)).
  Empty procs, `populated == 0`, no child directories, held handles through
  setup/postconditions, one removal, no early notification, no spawned child,
  and a single real setup call all match the S7/S9 evidence contract
  (`leaves_removed: 1`, `runs_created: false`, both writes true). The
  historical A1/A2 `leaves_removed == 0` assertion in `prepare_a1a2`
  ([fake.rs:1092-1098](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\hierarchy\fake.rs:1092))
  is untouched by the separate A5 fixture.
- **A4 (unchanged, open, independent).** No correction bytes touch the A4
  section; `KillMode=mixed` remains packaging
  ([arxd.service:15](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\packaging\linux\arxd.service:15)).
  A5 shares no A4 machinery (no crash, no two-instance sequence).

No new Spec findings; no minimal fix beyond the correction is required.

## Security

No blocker. The correction preserves every fence and closes the prior gate
line: the fixture carries the identical four disposable-host environment
gates plus real/effective root checks used by A1/A2 (`CI`/`GITHUB_ACTIONS`/
`RUNNER_ENVIRONMENT=github-hosted`/`ARX_DISPOSABLE_EXEC_TEST=1`, `getuid() ==
0`, `geteuid() == 0` per
[disposable.rs:388-397](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-3159a252-374f30b6021abb6e\crates\arxd\src\exec\disposable.rs:388)),
so the root `mkdirat` seeder can never run outside a fresh disposable host.
Pre-mutation checks are read-only port observations (no writes, no enables);
the only new privileged act stays bounded (one `runs/` + one empty leaf by
the service main, removed by the janitor under test), bound to that unit/run
with links/foreign/replaced objects rejected.

## Verdict

Standards: 0 findings (worst: none). Spec: prior S1–S3 closed by the
correction, 0 new findings (worst: none). Security: prior gate line closed,
0 blockers (worst: none). A4 accepted open and independent; A5 source
preparation may proceed on this plan. M2 A4/A5, remaining Stage F/Stage G,
and PA6 stay open, **unverified on real hardware**. No source, PROGRESS,
push, or PR action taken; this recheck commits only itself.
