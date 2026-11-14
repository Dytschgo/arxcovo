# Review: M2 A5 evidence pin refresh (Muse, independent review)

2026-11-14; Muse; independent review of the Sol medium pin-refresh
change. Read-only except this file: `git` CLI reads, `git diff --check`,
`python -I -B tools/ci/e3-native/test_driver.py`,
`python -I -B tools/ci/s2b-native/test_driver.py`, and one read-only
in-memory anchor probe (owned temp Python file, `git cat-file` blob reads
only; no writes to source). No Cargo/native, product CLI, system service,
account/owner probe, network, GitHub, CI, manual dispatch, or subagents.

## Exact pins

- Base: `294542a21320a34786dc08710f8f57c674086a03`
  (`test(exec): prepare bounded A5 empty-orphan acceptance (#94)`, PR94 merge).
- Head: `99426e53ce0ef4775203a20887cb660f9c173066`
  (`fix(ci): refresh reviewed test source pins after A5`).
- `git log 294542a..99426e53 --oneline` is exactly one commit; `git diff
  294542a...99426e53 --name-only` is exactly three files:
  [driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d759b278-0c1c3ba2dbb2a9ba\tools\ci\e3-native\driver.py),
  [driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d759b278-0c1c3ba2dbb2a9ba\tools\ci\s2b-native\driver.py),
  and the new evidence note
  [m2-a5-evidence-pin-refresh.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d759b278-0c1c3ba2dbb2a9ba\docs\reviews\m2-a5-evidence-pin-refresh.md).
- Spec read:
  [m2-a5-evidence-pin-refresh.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d759b278-0c1c3ba2dbb2a9ba\docs\reviews\m2-a5-evidence-pin-refresh.md).
  Prior A5 source review:
  [m2-stagef-a5-source-muse-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-d759b278-0c1c3ba2dbb2a9ba\docs\reviews\m2-stagef-a5-source-muse-review.md)
  (`174a402`).
- Scope: exactly two one-line hash-constant updates plus the note; no
  logic, guard, workflow, manifest, classifier, or restoration change.

## Standards

PASS, no findings. The diff touches two string literals and one new
`docs/reviews/` note, matching the repo's one-concern conventional-commit
shape (`fix(ci)`). No new names, types, branches, or abstractions, so the
smell baseline has nothing to attach to: no Duplicated Code (one literal
per pin site), no Speculative Generality (both constants are consumed by
the existing `source_pin`/`PINS` refusal checks), no Divergent Change or
Shotgun Surgery (one line per driver, zero behavior drift). `git diff
--check` is clean.

## Spec

PASS, no findings. Every clause of the evidence note verified:

- E3 `SOURCE_SHA` for `crates/arxd/src/exec/hierarchy/linux.rs` is now
  `af82315a49d97d090086d2ef1cc0fe7069be8894ad597f70b3238f7c0b6e9479`;
  S2b `PINS` entry for
  `crates/arxd/src/service/unix_bridge/tests/e6.rs` is now
  `05e30be5a57921492bd6289f23cd3618fb56270d89a78947cab18779516c05bb`.
  Both equal the SHA-256 of the committed LF Git blobs at head
  (`git cat-file -p <head>:<path>`, hashed in-process; earlier naive
  piped `git show` disagreed only because the shell altered bytes).
- All other pins, manifests, and guards unchanged: the full six other S2b
  `PINS` entries re-hash to their pinned values, both `MANIFEST_SHA`
  values (`c8248205...` E3, `28f05948...` S2b) match the committed
  `mutations.jsonl` blobs byte-for-byte, and `git diff base...head --
  .github/ tools/ci/e3-native/mutations.jsonl
  tools/ci/s2b-native/mutations.jsonl` is empty.
- A5 trace: `git diff 440e185...294542a` on the two pinned files shows only
  `#[cfg(test)]`-gated additions (`seed_a5` helper in `linux.rs`, one
  `a5_pending_through_janitor_and_s9` tokio test in `e6.rs`), consistent
  with the accepted Muse A5 source review (`174a402`). No altered prod:
  all eight E3 anchors still apply exactly once before the
  `#[cfg(test)]` boundary and both S2b anchors exactly once in memory
  against the head blobs (probe output: 8/8 E3, 2/2 S2b).
- Portable suites rerun in this session: `tools/ci/e3-native/test_driver.py`
  — 25 tests, one skip, exit 0; `tools/ci/s2b-native/test_driver.py` — 11
  tests, exit 0. Callgraph inspected first: owned temp fixtures and
  synthetic Python children only, no native/driver invocation. This is
  applicability only, not compiled mutant credit; no native or manual
  workflow was run.

## Security

PASS, no findings. Pin refusal is preserved, not weakened: `source_pin`
(E3) and `PINS` (S2b) exact-equality guards, `manifest_pin`,
`tracked_guard`/`checkout_guard`, `restore_failed`/`tree_changed`
restoration checks, and the `ubuntu_only`/`hosted_only`/`root_refused`
entry gates are byte-identical to base. The refresh only advances two
constants to the reviewed A5 bytes, so ordinary existing non-required
hosted evidence jobs stay compatible on this head and keep failing closed
on any other bytes. This review grants no manual A5 run and no M2, Stage
F/G, or real-Runner PA6 acceptance. Installed/dedicated paths remain
**unverified on real hardware**. D7c is consumed; M2 F/G and PA6 stay open.

## Verdict

1 finding per axis: none. Worst issue per axis: none. Repair-only change
accepted as specified; merge decision stays with the coordinator and its
required checks.
