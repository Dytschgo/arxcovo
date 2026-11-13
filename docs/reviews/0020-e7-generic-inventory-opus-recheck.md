# ADR 0020 Slice E7 PB3 generic and closure boundary recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. I was the only
review sub-agent and used no subagents. This is a narrow independent recheck
of Sol medium correction attempt 3 (`f059a53`), which answers PB3 from my
previous recheck. That recheck was made at `44b1e79`, committed as `ef368d0`
and retained as `346c589`
(`docs/reviews/0020-e7-brace-inventory-opus-recheck.md`). Findings from it
that are not listed below carry forward unchanged. PA8 and PA9 are not
reopened.

**Standards: APPROVED.** There are no blockers. SA5 (prose spacing) carries
forward to the new text. One minor naming note (SA6) is below.

**Spec (including security): APPROVED for the narrow PB3 scope.** PB3 is
closed. A `cfg(test)` attribute on a final generic parameter, a final
lifetime parameter or a single closure parameter no longer hides the
production code after it. I proved this with the real crate inventory and
real source files. I found no new blocker, so no other-provider correction
handoff is needed for this attempt.

This recheck grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Publication still needs a PR, a
different-provider review of the exact published head and all required
checks. Native, installed and dedicated paths remain **unverified on real
hardware**. M2 remains incomplete. The historical dispatcher privacy flake,
`E_PROTECTION` and `ExecutionFailed` remain unknown. I claim no fix,
diagnosis or retry for any of them. Stage F and Stage G remain separate
gates.

## Pins and isolation

- Worktree `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-8cdc10a1-...`,
  branch `t3/8cdc10a1`. The cwd and the Git top level are both this
  worktree, not the canonical checkout `D:/Code/Arxcovo`. Status was clean
  at the start and after every probe.
- Target HEAD `b9e0658a1c174fde5c2c25584b504a40cc5f7ba5` (evidence) over
  source `f059a53886d4bc9aae82fc189868921d54e5490d`, parent `346c589`.
  `git diff --stat 346c589 HEAD` touches three paths: `stage_b_tests.rs`
  (+4/-1), the E7 evidence (+22) and one PROGRESS line. There is no
  production, CI, Cargo, lockfile or policy change.
- The committed scanner matches the evidence manifest. Blob
  `1ba51296f1567c96fad348adf2f4763540e76d31` matches both
  `git rev-parse HEAD:<path>` and `git hash-object`. The SHA-256 is
  `07ef5abb...1022eb`.
- `rust-toolchain.toml` pins stable `1.95.0`.
- Read: AGENTS, the review skill, my previous recheck, both target commits,
  `tokens()`, `e7_non_test_tokens`, `e7_inactive_source_allowed` and both
  inventory tests.

## Standards

- **Change matches the probed fix.** The stop set is now
  `")" | "]" | "}" | ">" | "|"`, exactly the one-line fix I probed. Three
  shapes were added to `w8_e7_test_braces_never_hide_following_production`,
  giving 7 shapes times 2 forbidden calls. All go through the real
  `e7_inactive_source_allowed` path.
- **Tokenizer fit.** `tokens()` emits every punctuation byte as its own
  token. So `->`, `=>`, `>>` and `||` split into single `>` and `|` tokens,
  and the new stops see them. `'a>` is not taken as a char literal, because
  the byte after `a` is not `'`. The stop therefore applies to every
  spelling of these delimiters.
- **SA6 (minor, judgement call).** The test name
  (`..._test_braces_never_hide_...`) and its message ("test-only braced
  forms cannot hide production") now also cover generic, lifetime and
  closure shapes, which are not braced forms. The code comment above the
  stop set still cites only "a closure". A wording such as "test-only
  attributed forms" would fit better. This does not block.
- **SA5 carries forward.** The new evidence and PROGRESS text still has
  glued tokens, such as "recheckef368d0", "retained346c589", "attempt3",
  "and3", "exit100", "focused11/11" and "in0.418s".
- **Exit-code wording (minor).** The evidence reports the mutant failure as
  "exit100". In my runs a failing `cargo test` exited 101. Both are
  failures; the number does not change any credit.

## Spec (including security)

Closed and credited:

- **PB3 closed.** Each probe below was an uncompiled
  `src/zz_review_probe.rs`, run through the real
  `w8_e7_crate_wide_inactive_source_inventory` and removed after the run:
  - plain `Runner::new(c)` control: **fail** (expected);
  - `fn g<#[cfg(test)] T>(c: u8) { crate::exec::Runner::new(c); }`:
    **fail** (passed at `44b1e79`);
  - `fn l<#[cfg(test)] 'a>(c: u8) { crate::exec::hierarchy::setup(); }`:
    **fail** (passed at `44b1e79`);
  - `fn h2(c: u8) { let f = |#[cfg(test)] a: u8| crate::exec::Runner::new(c); let _ = f; }`:
    **fail** (passed at `44b1e79`);
  - extra variants: `<#[cfg(test)] T: Into<Vec<u8>>>` (nested `>>`), a
    closure with `-> u8 { .. }` and `impl<#[cfg(test)] T> S { .. }`: all
    **fail**;
  - guarded positive control (`#[cfg(test)] fn t() -> u8 { 0 }`, then a
    `cfg(test)` module that holds the call): **pass** (expected).
- **Mutants.** Against the committed scanner I applied three one-line
  mutants, one at a time, each restored byte-exactly before the next:
  - the old stop set `")" | "]" | "}"`: the new test failed at line 967 and
    the crate inventory passed (exit 101);
  - only `">"` removed: the new test failed (exit 101);
  - only `"|"` removed: the new test failed (exit 101).

  So each added stop is needed on its own, and the new fixtures kill it.
- **Fail-closed direction kept.** The new stops can only end stripping
  earlier. Any trailing test-only tokens are then scanned, which can cause
  false failures but not missed production. This includes the trade-off the
  evidence discloses: a `cfg(test)` item with `->`, `=>`, a generic `>` or a
  `|` stops early, and a forbidden name in its body would fail the fence.
  The current tree passes, so no such item exists today.
- **Disclosure.** The evidence states the stricter false-positive behavior
  and keeps calling the inventory a lexical fence, not a semantic Rust
  proof. PA8/PA9 stay disclosed as limits. This matches what I asked for.

Considered, not a finding:

- **Attributes on `where` predicates.** `where #[cfg(test)] T: Copy { .. }`
  would also escape the stop set, because the predicate ends at the body
  brace. rustc 1.95 rejects it in both lib and `--test` builds with E0658
  ("attributes in `where` clause are unstable"), and the toolchain is pinned
  stable. So the shape cannot reach the crate today. If that syntax is
  stabilized and the toolchain moves, add `where`-clause controls at that
  point. I list this only for whoever changes the pin.

Carried forward without change: PA1-PA9 and the `pub` cross-crate limit.
I found nothing new on them.

## Credits and limits

Builder runs I credit by byte identity, without repeating them: locked
workspace all-target Clippy, the three real source probes and the focused
11/11 run `7c2a7846` in 0.418 s. Full suite 383/3 stays credited at
`cea3038` only, and the 7-kill LF mutation credit stays at `09cc061` only.
There is no new full-suite claim.

My runs at HEAD `b9e0658` on Windows x86_64-pc-windows-msvc, with a target
directory in the session scratchpad:

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: clean. This
  is package-scoped, not the workspace.
- Focused selection (8 E7 tests, both inventory tests and the paused
  `stuck_drain_is_bounded_and_never_claims_stop_audit`): 11 passed in
  0.36 s.
- The three mutants and eight real source probes above. Each ended in a
  byte-exact restore, checked by SHA-256 against blob `1ba5129`, and a clean
  `git status`.
- rustc 1.95 validity check of the `where`-clause and closure-arrow shapes
  in the scratchpad.

Limits: a Windows run does not compile `cfg(target_os = "linux")` code. I
ran no full suite and did not repeat the 7 mutants, because no material
concern called for either. I made no hosted CI, GitHub push, PR, watch,
merge, retry or settings change. I used no NativeRunner, owner, account,
session, service, `/proc`, `/sys`, cgroup, root, VM, private state,
experiment, ignored native fixture or installation. I made no product or
PROGRESS edit. This file is my only commit.

## Summary

Standards: 0 blockers. SA5 (prose spacing) carries forward, and SA6 is a
minor test-naming note. Spec: 0 blockers. PB3 is closed for generic,
lifetime and single-closure-parameter attributes. Real source probes and
three one-line mutants prove it. The worst remaining item is the disclosed
lexical-fence limit: false positives on `->`, `=>` and `|`, and
`where`-clause attributes that stay out of reach only while they are
unstable. Production is unchanged and inert.
