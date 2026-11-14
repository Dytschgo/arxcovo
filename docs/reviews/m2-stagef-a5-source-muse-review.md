# Review: M2 Stage F A5 source preparation (Muse, independent source review)

2026-11-14; Muse; independent source review of the Sol A5 implementation.
Read-only except this file: `cargo test` (15 documented exact filters),
`cargo fmt --all --check`, `cargo clippy -p arxd --all-targets --locked
--offline -- -D warnings`, `git diff --check`, and `git` CLI reads. No native
API, product CLI, process/service/account/owner/root probe, network, GitHub,
CI, dispatch, ignored-fixture selection, or subagents.

## Exact pins

- Sol implementation reviewed:
  `8b347bb5c74b6ceda637aae46db87d6ae452adab`
  (`test(exec): prepare bounded A5 orphan janitor fixture`).
- Retained: `bbe269f28e3111ec5616d49146a1f423716f663a` (same message; working
  head at review time; Sol bytes identical across both).
- Base: `fd7fe94` (`docs(review): narrow recheck of Sol A5 seeder correction`).
  `git diff fd7fe94...HEAD --name-only` is exactly the six Sol files; the only
  other delta in range is the pre-existing `docs/PROGRESS.md` queue line from
  `0889a9d`, not Sol source.
- Accepted plan: `2aec267` (bounded A5 seeder and preconditions) in
  [m2-stagef-a4a5-janitor-plan.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\docs\reviews\m2-stagef-a4a5-janitor-plan.md).
- Prior reviews: `6cbc7e6` (plan review, S1-S3 + host-gate line) and `dbd8447`
  (recheck, all closed).
- Sol evidence read:
  [m2-stagef-a5-source-evidence.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\docs\reviews\m2-stagef-a5-source-evidence.md).
- Scope (622 insertions, 0 deletions, no other files touched):
  [a5.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\a5.rs) (new, 351),
  [disposable.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\disposable.rs) (+51),
  [linux.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\linux.rs) (+26),
  [fake.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\fake.rs) (+2),
  [e6.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\service\unix_bridge\tests\e6.rs) (+104),
  plus the evidence doc (+88).
- Production surface verified unchanged: `setup.rs`, `parse.rs`,
  `unix_bridge.rs`/`readiness.rs`, `tools/unix-exec-validation/`,
  `packaging/`, `.github/`, `Cargo.toml`/`Cargo.lock` have empty
  `fd7fe94...HEAD` diffs. Driver selection remains A3 None.

Scope: A5 source preparation only. A4 stays open and unimplemented (no A4
bytes in this diff). This review grants no A5 run authority and no M2
F/G/PA6 acceptance; D7c is consumed. All hardware paths **unverified on
real hardware**.

## Standards

PASS, no findings. Single-concern test-only change: every new logic item is
gated out of production — `a5.rs` is reachable only through `fake::a5`, and
`fake` is `#[cfg(test)]` ([hierarchy.rs:8-13](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy.rs:8));
`seed_a5` is `#[cfg(test)]` ([linux.rs:101-127](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\linux.rs:101));
the `disposable_linux_a5` fixture is `#[ignore]`d and the whole
`disposable` module is `#[cfg(all(test, target_os = "linux"))]`
(`crates/arxd/src/exec/mod.rs:373-374`). No `CgroupFs` port change, no new
dependency, no new selector API, no optional framework: 622 lines are
proportionate to one seeder, five model tests, one readiness test, and one
dormant fixture. No smell-baseline hit beyond acknowledged intent:
`finish`'s linear evidence checklist and the `finish`/`empty` S9 overlap are
deliberate independent verification, not speculative generality
(`arx-0-0` is a fixed literal, `leaf_name`-checked); the preflight/setup
anchor duplication is the plan-accepted read-only copy whose authoritative
copy setup repeats. `cargo fmt --all --check`, `git diff --check`, and
`cargo clippy -p arxd --all-targets --locked --offline -- -D warnings` all
pass (clippy exit 0, observed this session).

## Spec

ACCEPT source preparation: every plan clause is implemented, verified
against the pinned source.

- **Seeder home/access (S1).** `seed_a5` is one narrow inherent `#[cfg(test)]`
  `LinuxFs` helper returning held runs/leaf `Dir` handles; same-module field
  access needs no new port method and no raw fd is exported
  ([linux.rs:101-127](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\linux.rs:101)).
- **Read-only preflight (S2).** `preflight` performs exactly the plan's
  S0-S4 plus unit/supervisor port reads before either mkdir — `proc_status`
  + Standard hardening, `identity == (0,0)`, `proc_cgroup` + `locate`,
  root-owned cgroup2 walk with no group/other writes, `xattr_delegate`,
  domain type, empty unit procs, memory/pids controllers, supervisor holding
  only `pid()` ([a5.rs:36-67](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\a5.rs:36));
  the 14-case refusal test asserts `no_mutation`/`no_removal` per case.
- **Seed discipline (S3).** `mkdir_runs == Created` is required (never
  adopt), runs is re-trusted and required `0700`, `arx-0-0` passes
  `parse::leaf_name` and is created by exclusive descriptor-relative
  `mkdirat` (EEXIST fails), and leaf + runs must both read empty
  ([linux.rs:106-127](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\linux.rs:106)).
  `empty` rejects populated procs/events, nesting, and non-control entries
  via `classify_entry` Dot/Control-only
  ([a5.rs:21-35](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\a5.rs:21)).
- **One setup call, exact acceptance.** `finish` invokes production `setup`
  exactly once and additionally requires `leaves_removed == 1`,
  `runs_created == false`, both controller writes, fresh S9 (subtree
  control, empty procs/events, control-only listing, three `open_ro`), and
  literal `arx-0-0` absence with no cancellation
  ([a5.rs:69-154](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\hierarchy\a5.rs:69)).
  The ignored fixture holds both descriptors through setup/postconditions,
  drops only after, then sends READY and holds identity-ack — same host
  gates as A1/A2 (four env gates, uid/euid 0, umask, ack absence,
  PROC_MAGIC, NOTIFY_SOCKET, hold)
  ([disposable.rs:454-503](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\exec\disposable.rs:454)).
- **Readiness.** The e6 test observes Pending plus zero runner calls at
  every kill/events/removal/S9 point, admits exactly one fake call only on
  success, and refuses on timeout/shutdown polling
  ([e6.rs:5-107](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-c3ba7a2f-73d6b2b7f5f8bc76\crates\arxd\src\service\unix_bridge\tests\e6.rs:5)).
  Its temp dir is the normal owned 0700 test fixture, not an owner probe.
- **No regressions.** Historical A1/A2 assertions untouched; `prepare_a1a2`
  still requires zero removals. Call-graph inspection before re-running
  confirmed: hierarchy tests use Fake/Manual plus pure parsers/setup only;
  no selected path touches LinuxFs, real Runner, native probes, subprocess
  fixtures, service/account tools, or networking.
- **Executed feedback (this session, Windows, offline).** All 15 documented
  exact filters ran individually with `--exact`, one test each, all passing
  (394-test lib, 393 filtered): five `fake::a5`, `e6::a5`, four
  H4/H6/H18/H19/H20, four A1/A2, `e7::w8`. Mutant/red-green claims verified
  by wording only (not re-run; out of scope): the evidence credits assertion
  sensitivity of six temporary test-only faults, explicitly not production
  mutation coverage or hardware evidence — that wording is accurate, and
  `setup.rs` is byte-identical, so no production mutation exists.
- **Limits (honest).** Linux-only additions (`seed_a5`, dormant fixture) do
  not compile on this host by target gating and were NOT compiled here —
  **UNCOMPILED locally**; the ignored A5 fixture was not selected. A4
  remains open and independent. No cross-compile was attempted (no toolchain
  install).

No new Spec findings; no change beyond Sol's six files is required.

## Security

No blocker. The only new privileged act (root `mkdirat` of `runs/` + one
empty leaf, removed by the janitor under test) is bounded to the owned
unit/run, fixture-only, and unreachable outside a fresh disposable host: the
fixture carries the identical four environment gates plus real/effective
root checks as A1/A2. Pre-mutation checks are read-only port observations
(no writes/enables); exclusive leaf creation, link/foreign/nested refusal,
held-handle lifetime, single removal, and no early notification/READY are
all enforced in code as reviewed above.

## Verdict

Standards: 0 findings (worst: none). Spec: 0 findings (worst: none; S1-S3
all implemented as planned). Security: 0 blockers (worst: none). A5 source
preparation is ACCEPTED on the accepted plan; A4 stays open and independent.
M2 A4/A5, remaining Stage F/G, and real-Runner PA6 stay open, **unverified
on real hardware**. This commit adds only this review; no PROGRESS, source,
push, or PR action taken.
