# ADR 0020 Slice E7 PB2 brace-stripping recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. I was the only
review sub-agent and used no subagents. This is a narrow independent recheck
of the Sol medium PB2 correction. The previous recheck was made at `fe1ee5d`,
committed as `59c169b` and retained as `69931fd`
(`docs/reviews/0020-e7-drain-inventory-opus-recheck.md`). Findings from it
that are not listed below carry forward unchanged. PA8 and PA9 are not
reopened.

**Standards: APPROVED.** There are no blockers. SA5 (prose spacing) carries
forward and also applies to the new text. The new code removes the smell I
flagged before.

**Spec (including security): CHANGES REQUESTED.** PB2 as reported is
closed: braced `cfg(test)` items no longer hide the production code after
them. One new blocker (PB3) is in the same class: a `cfg(test)` attribute on
a generic parameter, a lifetime parameter or a single closure parameter
still makes the stripper swallow the production code that follows. I proved
this against the real inventory test with real source files. The fix is one
line and fails closed, and I checked it by probe.

This recheck grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Publication still needs a PR, a
different-provider review of the exact published head and all required
checks. Native, installed and dedicated paths remain **unverified on real
hardware**. M2 remains incomplete. The historical dispatcher privacy flake,
`E_PROTECTION` and `ExecutionFailed` remain unknown. I claim no fix,
diagnosis or retry for any of them. Stage F and Stage G remain separate
gates.

## Pins and isolation

- Worktree `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-33dd1f95-...`,
  branch `t3/33dd1f95`. The cwd and the Git top level are both this
  worktree, not the canonical checkout `D:/Code/Arxcovo`. Status was clean
  at the start and after every probe.
- Target HEAD `44b1e7969ad8bfeb1a488deac1c2ba2b69d19c6b` (evidence) over
  source `a3e27118895627c1571bd49a706a7f5a461324c3`, parent `69931fd`.
  `git diff --stat 69931fd HEAD` touches three paths: `stage_b_tests.rs`
  (+23/-11), the E7 evidence and one PROGRESS line. There is no production,
  CI, Cargo, lockfile or policy change.
- The committed scanner matches the evidence manifest. Blob
  `3a75dd0b980b5d8621c3a4a3cd6198d4f04d3bbb` matches both
  `git rev-parse HEAD:<path>` and `git hash-object`. The SHA-256 is
  `f6482569...6004bc`.
- Read: AGENTS, the review skill, the previous recheck, both target diffs,
  `tokens()`, `e7_non_test_tokens`, `e7_inactive_source_allowed` and both
  inventory tests.

## Standards

- **Smell resolved.** The `begin`/`block_item` keyword heuristic is gone.
  Stripping now stops after any balanced brace group that brings nesting
  back to zero. The function is shorter and easier to reason about.
- **New test.** `w8_e7_test_braces_never_hide_following_production` builds
  all eight combinations of the four shapes and two calls. It goes through
  the real `e7_inactive_source_allowed` path. Its name and message are
  accurate. The existing negative, guarded and production-after-guard
  controls are kept unchanged.
- **SA5 carries forward.** The new evidence and PROGRESS text still has
  glued tokens, such as "recheck59c169b", "retained69931fd", "exit100",
  "focused11/11", "in0.457s", "or7-mutant" and "at09cc061". This is minor.

## Spec (including security)

Closed and credited:

- **PB2 closed for the reported shapes.** In each of these real-file probes,
  an uncompiled `src/zz_review_probe.rs` was removed after the run:
  - A plain `Runner::new(c)` control made the inventory fail, as expected.
  - The previous PB2 proof file (`#[cfg(test)] macro_rules! m { () => {}; }`
    followed by a production `Runner::new(c)`) now makes the inventory
    **fail**. At `fe1ee5d` it passed.
- **Mutant.** I put the pre-correction `e7_non_test_tokens` from
  `a3e2711^` into the current file and kept everything else. The new test
  failed at line 974 and the other 10 passed (exit 101). I restored the
  file byte-exactly, checked by SHA-256.
- **Fail-closed direction kept.** Ending at the first balanced top-level `}`
  can only end stripping earlier. Any trailing test-only tokens are then
  scanned. Examples are `else { .. }` after an `if` block, and a `;` after a
  struct literal. This errs toward false failures, not missed production.
  The current tree passes.

Blocker:

- **PB3 `cfg(test)` attributes inside `<..>` or `|..|` still hide
  production.** The stripper tracks only `()`, `[]` and `{}`. It stops at
  an unmatched closer of those, or at a top-level `;`, `,` or balanced `}`.
  If the attribute sits on the last generic or lifetime parameter, or on the
  only closure parameter, the list ends at `>` or `|`. Neither token stops
  stripping, so the signature and the following body or expression are
  consumed. All three shapes compile under rustc 1.95, edition 2024, both
  with and without `--test`. Each one **passed** the real
  `w8_e7_crate_wide_inactive_source_inventory` as an uncompiled
  `src/zz_review_probe.rs`:
  - `fn g<#[cfg(test)] T>(c: u8) { crate::exec::Runner::new(c); }`
  - `fn l<#[cfg(test)] 'a>(c: u8) { crate::exec::hierarchy::setup(); }`
  - `fn h2(c: u8) { let f = |#[cfg(test)] a: u8| crate::exec::Runner::new(c); let _ = f; }`

  If another closure parameter follows, a `,` ends stripping, and that
  probe failed as expected. The old heuristic stripped these shapes too, so
  this is not a regression from `a3e2711`. It is still the same failure
  class that PB2 named, and the evidence does not list it among the
  PA8/PA9 limits. These forms are rare in real code, much rarer than
  test-only `macro_rules!` or match arms. The current tree has none. I
  still count PB3 as a blocker on the same grounds as PB2: it is proven, it
  is undisclosed, it hides production code and the fix is cheap.

  **Fix (probed):** add `">" | "|"` to the top-level stop set:
  `if nesting.is_empty() && matches!(token, ")" | "]" | "}" | ">" | "|")`.
  With this temporary change, all 11 focused tests passed, which includes
  the existing controls and the current tree. The three shapes above all
  made the inventory **fail**. A guarded file with
  `#[cfg(test)] fn t() -> u8 { 0 }` followed by a `cfg(test)` module still
  passed. Trade-off: this ends stripping early at `->`, `=>`, a generic
  `>` or an or-pattern `|`. The rest of the test-only item is then scanned,
  which can only cause false failures. A future `#[cfg(test)] fn .. -> T`
  in a non-test file whose body names a forbidden pattern would need to
  move into a test module. A narrower variant is acceptable if all three
  shapes fail closed. Add them as negative controls next to the four brace
  shapes. Disclosing the gap in place of a fix is the weaker option.

Carried forward without change: PA1-PA9 and the `pub` cross-crate limit.
I found nothing new on them.

## Credits and limits

Root runs I credit by byte identity, without repeating them: locked
workspace all-target Clippy, the two disconnected macro source probes (exit
100, then removed), and the focused 11/11 run `4b761b01` in 0.457 s. Full
suite 383/3 stays credited at `cea3038` only, and the 7-kill LF mutation
credit stays at `09cc061` only. There is no new full-suite claim.

My runs at HEAD `44b1e79` on Windows x86_64-pc-windows-msvc, with a target
directory in the session scratchpad:

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: clean. This
  is package-scoped, not the workspace.
- Focused selection (8 E7 tests, both inventory tests and the paused
  `stuck_drain_is_bounded_and_never_claims_stop_audit`): 11 passed, twice,
  in 0.42 s each.
- Real-source inventory probes: the runner control failed, the PB2 macro
  file failed, the generic, lifetime and single-closure-parameter files
  passed (PB3), and the two-closure-parameter file failed.
- The old-helper mutant and the candidate-fix probe on `stage_b_tests.rs`,
  with results above. Each ended in a byte-exact restore, checked by
  SHA-256 against blob `3a75dd0`, and a clean `git status`.
- rustc 1.95 validity check of the PB3 shapes, as a lib and with `--test`,
  in the scratchpad.

Limits: a Windows run does not compile `cfg(target_os = "linux")` code. I
ran no full suite and did not repeat the 7 mutants, because no material
concern called for either. I made no hosted CI, GitHub push, PR, watch,
merge, retry or settings change. I used no NativeRunner, owner, account,
session, service, `/proc`, `/sys`, cgroup, root, VM, private state,
experiment, ignored native fixture or installation. I made no product or
PROGRESS edit. This file is my only commit.

## Summary

Standards: 0 blockers. SA5 (prose spacing) carries forward, and the
previous smell is resolved. Spec: 1 blocker. PB2 is closed for braced
items. The worst issue is PB3: `cfg(test)` on a final generic or lifetime
parameter, or on a single closure parameter, still hides the production
code that follows. Real source probes proved it, and a one-line,
fail-closed fix was probed. Production is unchanged and inert.
