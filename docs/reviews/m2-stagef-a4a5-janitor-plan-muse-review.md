# Review: Stage F A4/A5 startup janitor plan (Muse, independent design review)

2026-11-14; Muse; independent design review of the Sol A4/A5 source plan.
Read-only: no builds, tests, product CLI, native/service/system, implementation
or dispatch authority exercised. `git` CLI reads only.

## Target pins

- Plan reviewed: [m2-stagef-a4a5-janitor-plan.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\docs\reviews\m2-stagef-a4a5-janitor-plan.md)
  at `f08427f` (retained from Sol `376a01f`; plan bytes identical across both).
- Prepared head `6f47ac4` adds only [PROGRESS.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\docs\PROGRESS.md)
  (2 lines); no plan change. Plan states base `c9a9924`.
- All 15 blob pins in the plan's table verified equal at `f08427f`
  (e.g. `setup.rs` `b33bed7`, `linux.rs` `ae26fad`, `fake.rs` `9a8ecf7`,
  `disposable.rs` `e1644a9`, `arxd.service` `0b33773`).
- Governing sources: [ADR 0020](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\docs\adr\0020-linux-execution-hierarchy-and-janitor.md)
  (accepted inactive staged contract), [brief 0020](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\docs\briefs\0020-linux-hierarchy-implementation.md)
  privileged acceptance A4–A6, production setup/janitor
  ([setup.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\setup.rs),
  [linux.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\linux.rs),
  [fake.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\fake.rs)),
  A1/A2/A3 fixtures
  ([disposable.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\disposable.rs)),
  package ([arxd.service](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\packaging\linux\arxd.service)),
  driver ([driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\tools\unix-exec-validation\driver.py)).

Scope: A5 source preparation only. A4 may remain open without blocking A5.
A3 Standard/None consumed their recorded runs; D7c grants no future authority.
M2 A4–A12, Stage G and real-Runner PA6 stay open; all hardware paths
**unverified on real hardware**.

## Standards

PASS, no findings. The plan is a single-concern docs-only change (119 lines,
one file), matching the small-commit rule. It grants itself no implementation,
native run, dispatch, retry, activation, CI change, publish instruction, owner
step, secret, or approval flow; its "grants no ..." clause (plan §“Minimal
reuse”) is explicit. It adds no product approval flow and no optional
framework — it expressly forbids the general restart harness and package
weakening, which is the proportionate-fences behavior the user asked for.
No smell-baseline hit applies: no speculative generality (A5 is one literal
fixture, no new selector API), no divergent change, no mysterious name
(`StartupDomain` resolves to [unix_bridge.rs:368](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\service\unix_bridge.rs:368);
`Restart=no`/`O_EXCL`/120 s/140 s resolve to the driver fixture override and
monitor, [driver.py:578-592,637-638](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\tools\unix-exec-validation\driver.py),
not packaging).

## Spec

A5 preparation: CHANGES REQUIRED (3 minimal plan clarifications, S1–S3).
A4: ACCEPTED as open and independent (no plan change required).

- **S1 (required, A5 seeder reachability).** The plan mandates "a small
  test-local descriptor-relative mkdirat seeder, not a new CgroupFs method,"
  but names no access path, and none exists from the fixture's module:
  `LinuxFs::Dir` is `pub(crate) struct Dir(OwnedFd)` with a private field
  ([linux.rs:19](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\linux.rs:19)),
  and the `CgroupFs` port exposes no mkdir-leaf or fd accessor, while the A5
  ignored fixture will live alongside A1/A2 in `exec/disposable.rs`.
  Minimal fix: name the seeder's home module and access path in the plan —
  e.g. a `#[cfg(test)]` helper inside `hierarchy/linux.rs` (same-file field
  access via `open_root`/`open_child`) called from the disposable fixture —
  or concede one narrow `#[cfg(test)]` port method. Without this, "no new
  CgroupFs method" is unimplementable from `disposable.rs`.
- **S2 (required, S0–S4 pre-mutation check).** "Verify the same S0–S4 anchors
  before mutation" does not say which calls. Production `setup()` performs
  anchors internally and fails safe before any write ([setup.rs:205-257](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\setup.rs:205),
  H10 zero-writes), so a test-local pre-check duplicates logic that can drift.
  Minimal fix: define it as read-only port observations with the same parsers
  (`proc_status`+`hardening`, `identity`, `proc_cgroup`+`locate`, `open_root`
  + component walk, `xattr_delegate`, unit-state reads; no writes), or state
  explicitly that the plan relies on setup's own anchor failures instead.
- **S3 (required, A5 host/name preconditions).** S0 requires `Hardening::Standard`,
  so the A5 host needs the unchanged Standard drop-in, but the plan never
  states it (A3 needed explicit Standard/None variants). Minimal fix: one line
  requiring the exact packaged unit + Standard drop-in (same host precondition
  as A1/A2), and that the leaf name is a fixed canonical literal checked by
  `parse::leaf_name` (stable provenance; any valid name is removed, cf. H16).
  The rest of the A5 contract checks out against source: seeder-created `runs/`
  → `mkdir_runs` returns `Exists` ([linux.rs:277-287](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\linux.rs:277),
  so `runs_created: false` ("runs reused") is consistent; `leaves_removed: 1`
  fits the `u8` evidence ([setup.rs:308-313](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\setup.rs:308));
  S7 kill/write/wait/check-all-before-first-removal/verify
  ([setup.rs:259-294](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\setup.rs:259))
  handles an already-unpopulated `runs/`; S9 postconditions match the plan;
  A1/A2 `leaves_removed == 0` is untouched in `prepare_a1a2`
  ([fake.rs:1092-1098](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\hierarchy\fake.rs:1092)),
  so no historical assertion is relaxed by the separate A5 fixture.
- **A4 (accepted open, advisory only).** `KillMode=mixed` confirmed in
  packaging ([arxd.service:15](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\packaging\linux\arxd.service:15));
  `Restart=no` is the driver's fixture override
  ([driver.py:578-582](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\tools\unix-exec-validation\driver.py:578))
  — suggest citing that pin so it is not misread as packaging
  (`Restart=on-failure`). Whether systemd removes delegated subtrees at stop
  stays unresolved per ADR D9/U9, and prior-SIGKILL-survivor provenance is
  unestablished, so A4 has no implementable acceptance yet. The plan's
  forbids-list (no package weakening, no forced residue, no general restart
  harness) matches the ADR's rejected alternatives. A5 shares no A4 machinery
  (no crash, no two-instance sequence, separate fixture), so A4 stays open
  without blocking A5. No change required.

## Security

No blocker. The plan preserves every fence: S0-before-T1 order, exact
hardening, descriptor-relative `O_NOFOLLOW` opens, all-children-checked-before-
first-removal, verify-after-removal, and the 4096/512/64/1 MiB bounds are
carried over unchanged. Required: the A5 ignored fixture must carry the
identical disposable-host gate as A1/A2 (CI + `GITHUB_ACTIONS` +
`RUNNER_ENVIRONMENT=github-hosted` + `ARX_DISPOSABLE_EXEC_TEST=1`, euid 0;
cf. [disposable.rs:388-397](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-e3770786-e77e03ef9efa25a3\crates\arxd\src\exec\disposable.rs:388)) —
"fresh host" and "ignored fixture" imply it, but the plan should state it so a
root-mutating seeder can never run elsewhere. The only new privileged act (a
root `mkdirat` of `runs/` + one empty leaf by the service main) is bounded,
fixture-only, and removed by the janitor under test.

## Verdict

Standards: 0 findings (worst: none). Spec: 3 required plan clarifications, all
A5-only (worst: S1 seeder access path). Security: 1 required gate line (worst:
disposable-host gate). A4 accepted open; A5 may proceed independently once
S1–S3 are written into the plan. No source, PROGRESS, push, or PR action taken.
