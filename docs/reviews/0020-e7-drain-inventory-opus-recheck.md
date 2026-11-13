# ADR 0020 Slice E7 SB1/PB1 correction recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. I was the only
review sub-agent and used no subagents. This is a narrow independent recheck
of the Sol medium correction for the E7 review blockers SB1 (W9 real-time
wait) and PB1 (crate-wide inactive source inventory). The first review was
made at `4728c3d` and is retained as `d6e3f37`
(`docs/reviews/0020-e7-independent-security-review.md`). Advisories from that
review that are not listed below carry forward unchanged; I found no new
material on them.

**Standards: APPROVED.** SB1, SA1 and SA2 are closed. SA3 and SA4 carry
forward. There is one new minor advisory (SA5).

**Spec (including security): CHANGES REQUESTED.** One blocker (PB2): the
new inventory's `cfg(test)` stripper can also strip the production code that
follows certain test-only items. I proved this against the real test with a
real source file. Production code is unchanged and inert. The fix is a
one-line, fail-closed change, which I checked by probe. PA1-PA7 carry
forward.

This recheck grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Publication still needs a PR, a
different-provider review of the exact published head and all required
checks. Native, installed and dedicated paths remain **unverified on real
hardware**. M2 remains incomplete. The historical dispatcher privacy flake,
`E_PROTECTION` and `ExecutionFailed` remain unknown. I claim no fix,
diagnosis or retry for any of them. Stage F and Stage G remain separate
gates.

## Pins and isolation

- Worktree `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-a0b1f358-...`,
  branch `t3/a0b1f358`. The cwd and Git top level are both the worktree,
  never the canonical checkout `D:/Code/Arxcovo`. Status was clean at the
  start and after every probe.
- Target HEAD `fe1ee5d6d142e82967b2be37130c0e447d1cf51c` (evidence) over
  source `09cc061cc5933c0bd8fd8433868bc73fccaa0c24`, parent `d6e3f37`.
  `git diff --stat d6e3f37 HEAD` touches six paths: the four manifest paths,
  `docs/evidence/linux-discovery-e7.md` and one PROGRESS line. There is no
  production behavior, CI, Cargo, lockfile or policy change.
- All four rows of the correction manifest match the HEAD blobs: Git SHA-1s
  and SHA-256 of the committed bytes.
- Read: AGENTS, the review skill, the first E7 review, the T20b/W8/W9 text
  it cites, the source and evidence diffs, `tokens()` in `stage_b_tests.rs`,
  W9, `service::tests::stuck_drain_is_bounded_and_never_claims_stop_audit`
  and `ServiceApplication::drain`.

## Standards

- **SB1 closed.** W9 now asserts only that `app.drain()` does not finish
  within 100 ms. The fake is held, so the assertion cannot be satisfied
  early, and a slow host only makes the timeout more certain. It is not
  flaky. It fails if drain returns early. It keeps the `started` row, the
  unreleased gate, the late `succeeded` row and the cache-present
  assertions. The test-local `unwrap_or(Err(ExitCode::Store))` echo is gone.
  The production 10 s and `Store` mapping stay pinned in paused time by the
  cited test, which I ran. The evidence now calls the old W9 historical and
  withdraws its two overstated claims. My 10 focused tests took 0.35-0.37 s.
- **SA1 closed.** `Activity` has its doc comment back. `ToolCache` has its
  own accurate line.
- **SA2 closed.** The script now uses `write_bytes(...encode("utf-8"))`.
  Controls I ran on `application.rs` (LF in index and worktree): an LF
  identity rewrite passes W8. The old `write_text` identity rewrite still
  fails W8. So the confound was real and the new method removes it. My run
  of the committed script reported 7 of 7 `COMPILED KILLED` and
  `EXACT RESTORE`, with the SHA-256 unchanged afterwards.
- **SA5 (new, minor) Prose spacing.** The new evidence and PROGRESS text has
  glued tokens, such as "uses100ms", "around0.16-0.20s", "Review4728c3d",
  "finalfocused10" and "exit100". Fix when convenient.
- Smell (judgement): in `e7_non_test_tokens`, the `block_item` keyword
  heuristic is the source of PB2. Dropping it makes the function simpler and
  fail-closed.

## Spec (including security)

Credited, and checked by reading or by probe:

- **Reach.** The walk covers every `src/**/*.rs`. It uses `symlink_metadata`
  and rejects symlinks, plus `FILE_ATTRIBUTE_REPARSE_POINT` (0x400) on
  Windows. It caps files at 1024 and each file at 1 MiB before reading, and
  requires more than 20 files. Every `#[path]` and `include!` target in the
  crate today lies inside `src` and is scanned in full.
- **Module guard.** The eight skipped files are tied to exact, count-1,
  token-level `cfg(test)` declarations, and to `mod e6;`/`mod e7;` under the
  `cfg(test)` `tests.rs`. Control: removing `#[cfg(test)]` from
  `mod residue_tests;` compiled and failed the inventory.
- **Allowlist.** Exactly one occurrence is removed in exactly two files. The
  `cfg(test)` `locate` import at `unix_bridge.rs:20-21` is stripped, as
  claimed. Controls: a second, aliased `SetupFailure` import in
  `readiness.rs` compiled and failed the inventory. The allowed import placed
  in a new file is rejected.
- **Fail-closed directions.** `cfg(all(test, ..))`, `#![cfg(test)]`, and
  generics or tuple commas that end stripping early all leave tokens in the
  scan, which errs toward false failures. String and comment contents are
  single tokens, so text inside them cannot start stripping.
- **Controls.** A real uncompiled `src/zz_review_probe.rs` holding
  `crate::exec::Runner::new(c)` failed the actual inventory test. This
  matches the root's runner/hierarchy probes. The in-test controls (negative,
  guarded and production-after-guard) are meaningful. Like the root, I treat
  all of these as lexical evidence, not compiled behavior.

Blocker:

- **PB2 `cfg(test)` stripping swallows following production code.**
  Stripping stops at a top-level `}` only when the item text before it
  contains `fn`, `impl`, `mod`, `struct`, `enum` or `trait`. For any other
  braced item or statement, it keeps consuming until the next top-level `;`,
  `,` or unmatched closer, or until a later block that contains one of those
  keywords. Production code in that span is never scanned. Shapes that
  rustc 1.95 accepts under edition 2024, both with and without `--test`,
  and that the current function accepts while hiding `Runner::new(c)`:
  - `#[cfg(test)] macro_rules! m { .. }` followed by a production `fn`
  - `#[cfg(test)] unsafe extern "C" { .. }` followed by a production `fn`
  - a `#[cfg(test)]` match arm with a block body, followed by another arm
  - `#[cfg(test)] if t { }` followed by a statement

  The `macro_rules` shape also hides `hierarchy::setup()`. Proof against the
  real test: an uncompiled `src/zz_review_probe.rs` containing
  `#[cfg(test)] macro_rules! m { () => {}; }` and then
  `fn p() { crate::exec::Runner::new(c); }` **passed**
  `w8_e7_crate_wide_inactive_source_inventory`. The evidence says the test
  "strips explicit cfg(test) items". This over-stripping goes beyond that
  claim and is not one of the disclosed alias or macro limits. Test-only
  `macro_rules!` helpers and `cfg(test)` match arms are ordinary Rust, so
  this is a realistic way to miss a regression. The tree is clean today.
  **Fix (probed):** end stripping at every top-level `}`, which makes
  `block_item` unnecessary. I replaced the condition with a temporary
  always-true one. The four shapes and the `macro_rules`/`hierarchy` case
  were then rejected, and the real inventory, including its own controls,
  still passed. Add these shapes as negative controls, at least
  `macro_rules`, the match arm and `extern`.

Advisories:

- **PA8 Residual lexical gaps (partly disclosed).** These evade the fence:
  `use crate::exec::hierarchy as h; h::setup()`, `type R = Runner;
  R::new(..)` and `<crate::exec::Runner>::new(..)`. The first two fall under
  the disclosed "aliases". The qualified-self form is not an alias. Either
  name it in the evidence or add `Runner>::new(` to the patterns.
  `start_daemon_discovery` used as a path without a call is not matched, but
  its definition is `cfg(test)`.
- **PA9 Skip-by-path and src-only reach.** Reviewed test files are skipped
  by relative path. A non-test `#[path = "exec/tests.rs"]` module or
  `include!` of one would go unscanned. So would a `#[path]` module outside
  `src`. The crate already uses `#[path]` and `include!`. The declaration
  check pins the expected declarations, but it does not reject extra ones.
  Consider failing on any `#[path]`/`include!` naming a skipped file or
  leaving `src`.
- **Limit.** `exec::Runner` and `Runner::new` are `pub` in a `pub mod exec`.
  The scan covers only `crates/arxd/src`. Today `arx-guest` does not depend
  on `arxd`, and `crates/arxd/tests` has no `Runner::new`. A future
  dependent crate would need its own pin.

## Credits and limits

Root runs I credit by byte identity, without repeating them: locked
workspace all-target Clippy, the two disconnected runner/hierarchy source
probes (exit 100, then removed), and the 10/10 run `aed18f66` in 0.452 s.
The honest record of the initial closure-delimiter failure is accepted. Full
suite 383/3 stays credited at `cea3038` only. There is no new full-suite
claim.

My runs at HEAD `fe1ee5d` on Windows x86_64-pc-windows-msvc, with a target
directory in the session scratchpad:

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: clean. This
  is package-scoped, not the workspace.
- Focused selection (8 E7 tests, the inventory and the paused drain test):
  10 passed, four times, in 0.35-0.37 s.
- Committed mutation script: 7 of 7 compiled and killed, with exact restore.
  LF and CRLF identity controls as described under SA2.
- Inventory controls in real source: runner file failed (expected),
  `macro_rules` over-strip file passed (PB2), ungated `residue_tests`
  failed, extra aliased import failed.
- A temporary in-file probe and a candidate-fix probe on `stage_b_tests.rs`
  (results under PB2 and PA8). Every run ended with a byte-exact restore,
  checked by SHA-256, and a clean `git status`. The scripts live in the
  scratchpad.
- rustc 1.95 validity check of the four PB2 shapes, edition 2024, in the
  scratchpad.

Limits: a Windows run does not compile `cfg(target_os = "linux")` code. I
ran no full suite: no material concern called for one. I made no hosted CI,
GitHub push, PR, watch, merge, retry or settings change. I used no
NativeRunner, owner, account, service, `/proc`, `/sys`, cgroup, root, VM,
private state, experiment, ignored native fixture or installation. I made no
product or PROGRESS edit. This file is my only commit.

## Summary

Standards: 0 blockers, 1 new minor advisory (SA5, prose spacing). SB1, SA1
and SA2 are closed. Spec: 1 blocker. The worst issue is PB2: `cfg(test)`
stripping in the new inventory swallows production code after braced
non-keyword items, and a real source probe proved it. There are 2 new
advisories (PA8, PA9). Production is unchanged and inert.
