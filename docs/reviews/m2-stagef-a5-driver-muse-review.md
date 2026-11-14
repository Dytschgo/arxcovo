# Review: M2 Stage F A5 closed driver mapping (Muse, independent source review)

2026-11-14; Muse; independent source review of the Sol closed-A5 driver slice.
Read-only except this file: `git` CLI reads, `git diff --check`, audited safe
mock commands and exact pure filters after call-graph inspection. No native /
product CLI, process / system / service / account / owner / probe, network,
GitHub / CI, dispatch, ignored-fixture selection, or subagents.

## Exact pins

- Sol implementation reviewed: `2a4d9d392315f99192fcdc4ca94a6a80e6ac6bf4`
  (`test(exec): prepare closed A5 driver mapping`).
- Retained: `fa541d239758cd9663f23ec724aa446c19cc2e1f` (same message; direct
  `git diff 2a4d9d3 fa541d2 --stat` is empty, Sol bytes identical).
- Base: `294542a21320a34786dc08710f8f57c674086a03`
  (`test(exec): prepare bounded A5 empty-orphan acceptance (#94)`).
- Prepared head at review time: `1f9a453fd615445a6692e1ef89975856f3986190`
  (`docs(progress): queue closed A5 mapping review`); `fa541d2...HEAD --stat`
  is exactly the 2-line `docs/PROGRESS.md` queue entry, so HEAD adds
  PROGRESS only beyond Sol source.
- Scope (`294542a...fa541d2`, 4 files, +147/−5, no other files touched):
  [driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\tools\unix-exec-validation\driver.py) (+2/−1),
  [test_driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\tools\unix-exec-validation\test_driver.py) (+71/−4),
  [mutation_check.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\tools\unix-exec-validation\mutation_check.py) (+11),
  plus the evidence doc (+63)
  [linux-stagef-a5-driver-preparation.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\docs\evidence\linux-stagef-a5-driver-preparation.md).
- Accepted plan: [m2-stagef-a4a5-janitor-plan.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\docs\reviews\m2-stagef-a4a5-janitor-plan.md)
  with Muse plan review and seeder recheck (S1–S3 closed, A4 open/independent).
- A5 source fixture/review read:
  [m2-stagef-a5-source-evidence.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\docs\reviews\m2-stagef-a5-source-evidence.md),
  [m2-stagef-a5-source-muse-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-fc23e76f-3bf1a9c549b1daa8\docs\reviews\m2-stagef-a5-source-muse-review.md).
  Existing pin PR95 parallel does not touch this scope.

Scope: closed A5 driver mapping only. A4 stays open and unimplemented. This
review grants no A5 run authority and no M2 F/G/PA6 acceptance; D7c is
consumed. All hardware paths **unverified on real hardware**.

## Standards

PASS, no findings. Single-concern test-only change: production `driver.py`
gains exactly one closed allowlist literal
(`exec::disposable::disposable_linux_a5`); `standard_profile()` still derives
Standard through the existing `TEST != a3_none` rule, `TEST` default stays
`disposable_linux_a3_none`, and `test_argv()` keeps the fixed
`--ignored --exact --test-threads=1 --color=never` shape. No selector input,
independent profile mode, schema, harness, workflow, packaging, or production
surface change. Tests pin literals independently (never read back from the
driver), so no Faulty oracle. No smell-baseline hit: no duplicated logic
(one literal + existing rule), no speculative generality (closed literal, no
new selector API), no divergent change, no mysterious name. `git diff --check`
passes (observed this session).

## Spec

ACCEPT driver mapping: every scoped clause is implemented, verified against
the pinned source and by rerun.

- **Closed mapping.** `standard_profile()` allowlist holds exactly the four
  literals; `test_source_default_remains_a3_none` pins actual TEST to A3 None;
  `test_closed_mapping` independently expects all four names, profiles
  (`a1_a2`/`a3_standard`/`a5` → Standard, `a3_none` → None) and complete argv.
- **Rejection before effects.** `test_closed_selection_before_effects` refuses
  `''`, cross-account, `a4`, `a5_none`/`a5_standard`/`a5x`/`a5\n`, and
  `TEST+'x'` in `standard_profile`, `test_argv`, `run_body`, and `main`, with
  `getuid`/`cg_preflight` patched to probes — no A4, cross-account, variant,
  suffix/newline, or empty selection reaches effects.
- **Install/argv/attribution.** `test_launch_contract` covers all four argv;
  `test_profile_pairings_and_install_bytes` and `test_exact_runtime_attribution`
  (parameterized to `a5`) check exact unit/drop-in bytes, owned sets, sole-PID
  file check, and all eight collision/mismatch refusals for A5 too.
- **Summaries.** `test_all_four_exact_summaries` accepts only the selected
  literal with one pass / zero failures and rejects each of the other three;
  `test_exact_summary` now fails an assertion (not an error) on refusal, so
  the historical-None mutant earns assertion credit.
- **Fence reuse.** `test_a5_reuses_existing_fences` replays hold/ack,
  MainPID/executable/placement, sole-PID, deadline, execve, attribution
  lifecycle, reservation, collision, and owned-cleanup tests under A5 with all
  host effects mocked; A5 adds no lifecycle of its own.
- **Mutation baseline.** Six new Python mutants (default-select, omit-A5,
  A5-without-Standard, A5-argv-selects-A3, arbitrary-names, historical-None)
  are anchored `count == 1` and killed by assertions; Rust stdin harness
  excludes only the separately housed unused `a5` module via the exact
  `'#[path = "a5.rs"]\npub(crate) mod a5;\n'` anchor assertion — no A1/A2
  test/assertion removed or weakened. Initial summary-mutant errors and the
  first stdin-module baseline failure earn no credit per policy; the final
  complete command passed.
- **No F0/F0b change.** No F0/F0b mock edits; bounds/precedence unchanged
  (reruns below confirm 23 F0 kills and 36 F0b kills + 4 equivalents).
- **Executed feedback (this session, Windows, offline).** Call-graphs
  inspected first: `test_driver.py` header is pure/mock-only; `safe_suite`
  excludes CLI/RuntimeGuards/CopyGuards; CgroupScan mocks `os.walk`/`lstat`;
  EntryRun mocks every host effect; Rust filters are Fake/Manual plus pure
  setup/parsers only — no LinuxFs, real Runner, or native adapter. Then:
  `python tools/unix-exec-validation/mutation_check.py` → 76 tests OK, 29
  Python assertion kills, 3 pure-Rust baselines passed + 4 compiled Rust
  assertion kills, driver SHA-256 unchanged
  (`49bc2a5cffaa2712fe4f172efe6a70c477b4747c2496305be2eefedcc44d3709`), Rust
  fingerprints unchanged; `test_f0_blockers.py --mutants` → 23 kills, hash
  unchanged; `test_f0b_transport.py --mutants` → 36 kills + 4 equivalents,
  hash unchanged; six `cargo test -p arxd --lib <filter> --locked --offline
  -- --exact` filters (five `fake::a5`, `parse::a3_packaged_profile_status`)
  → each 1 passed / 393 filtered. `git diff --check` passes.

No new Spec findings; no change beyond Sol's four files is required.

## Security

No blocker. The one new accepted literal derives Standard through the
existing rule, so an A5 host requires the unchanged packaged Standard drop-in;
a None-profile host stays ineligible. Rejection precedes every effect, so no
arbitrary disposable name, A4, cross-account, or variant input can reach
provisioning, cgroup, account, or service paths. Exact runtime-attribution,
sole-supervisor-PID, collision, reservation, and owned-cleanup fences are
extended to A5 unchanged. No secret, owner, approval-flow, or publish change.

## Verdict

Standards: 0 findings (worst: none). Spec: 0 findings (worst: none; mapping,
rejections, summaries, attribution, and reuse all verified by rerun).
Security: 0 blockers (worst: none). Closed A5 driver mapping is ACCEPTED on
the accepted plan; A4 stays open and independent. A5 run remains unauthorized
(D7c consumed); M2 A4/A5/F/G and real-Runner PA6 stay open, **unverified on
real hardware**. Linux-only fixture compilation/execution remain unverified
locally. This commit adds only this review; no PROGRESS, source, push, or PR
action taken.
