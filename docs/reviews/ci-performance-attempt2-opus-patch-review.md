# CI performance attempt 2: independent Opus review of the actual patch

Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-12. This is a
single-agent, different-provider review of the Sol high implementation. Opus
wrote the attempt-2 design proposal, so this review gives **no independence
credit for the design**. It gives different-provider credit only for Sol's
actual patch. No subagents were started.

## Target identity

| Ref | SHA |
| --- | --- |
| Review target (evidence head) | `ed03f6b2400891fa6ab1c08ba5b323b092f99858` |
| Implementation source commit | `c3285a8d6fd76c3a3ebcf311ac2c2529d6cfd732` |
| Prepared base (accepted proposal) | `d3bc3aa0c2d15b8661543f401b100fc853b4079d` |
| Published PR54 source | `48d4916e556cdcd78a0a7e58fa25c733bcc2814b` |
| Actual main | `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0` |

- The working directory and Git top level both resolve to the existing T3
  worktree `<prepared-worktree>`. That is distinct from canonical
  `D:/Code/Arxcovo`. Status was clean at the exact target HEAD before review
  and again after my checks. I created no checkout or worktree and made no
  canonical writes.
- `git log d3bc3aa..ed03f6b` shows two commits: the source commit `c3285a8`
  (3 files) and the docs commit `ed03f6b` (PROGRESS and evidence).
- Diffs reviewed: `48d4916..ed03f6b` for source, `d3bc3aa..ed03f6b` for the
  implementation delta, and `48d4916..ed03f6b` for the whole public delta.

## Scope verification (run by me)

- **Implementation delta `d3bc3aa..ed03f6b`.** Exactly 5 paths:
  `.github/workflows/ci.yml`, `scripts/ci-workflow.test.mjs`,
  `crates/arx-guest/tests/cli.rs`, `docs/PROGRESS.md` and
  `docs/evidence/ci-headless-shell.md`. That is the three permitted source
  paths plus PROGRESS and the named evidence record.
- **Public delta `48d4916..ed03f6b`.** Exactly 7 paths: the 5 above plus the
  retained `ci-performance-attempt2-proposal.md` and
  `ci-performance-attempt2-sol-review.md`. The attempt-1 non-qualifying record
  is in the evidence/PROGRESS paths. There are no other files.
- **ci.yml.** I removed only the two added lines (`    env:` and
  `      CARGO_PROFILE_DEV_DEBUG: '0'`). The result is byte-identical (`cmp`) to
  `48d4916:.github/workflows/ci.yml`. So the matrix (including `macos-latest`),
  the `verify (...)` names, the 12-minute timeout, concurrency, permissions,
  action pins, step order, `--with-deps --only-shell`, Media Foundation, the
  signed mirror, the macOS stock step and both smokes are all unchanged.
  `ci.yml` is the only workflow with any `CARGO_`/`RUSTFLAGS` token. The
  policy and other workflows are unchanged.
- **cli.rs.** The 48d4916 bytes are an exact prefix of the new file. The
  scaffold test is untouched.
- **Untouched files.** `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`,
  `package.json`, `package-lock.json`, `playwright.config.ts` and `tools/` show
  no diff from 48d4916. `.cargo/` does not exist. No crate has a `[profile`
  table.
- **Retained docs.** `docs/reviews/` has no diff `d3bc3aa..ed03f6b`, so the
  proposal and Sol review bytes are retained. The `d3bc3aa` bytes of the
  evidence file are an exact prefix of the new file.
- **PROGRESS.** Lines are only added, apart from one heading-character change
  (see S-N1). There is exactly one `## Current state` heading. The previous
  current block now sits under `## Historical checkpoint before attempt-2
  implementation`.
- **Whitespace.** `git diff --check 48d4916 ed03f6b` exits 0.

## Standards

**PASS. No blocking findings.** I inspected AGENTS, the KICKOFF review and
merge rules, the review skill and the Fowler smell baseline.

- The patch is small, single-concern and conventional-commit. Each behavior
  has a meaningful test: one Node guard and one compiled Rust guard. The
  guard lives in the existing `cli.rs` binary, so no new test binary is linked.
  Clippy's `assertions_on_constants` is satisfied by `black_box`, not by a
  lint `allow`. That differs from the proposal snippet's bare `cfg!` but
  keeps its meaning, and the evidence records why.
- The Node guard's regexes tolerate CRLF (`\r?`), as the existing tests do.
  The `existsSync` loop is bounded to three fixed repo-relative paths. No
  unsafe path handling is involved.
- Smell baseline: none applies. The test name says exactly what it checks.
  There is no speculative generality: the profile-table loop covers only the
  three locations Cargo actually reads for profiles.
- Evidence uses a `<prepared-worktree>` placeholder and contains no owner
  paths or secrets. Hardware and unknowns are labelled correctly: native,
  installed and dedicated paths are **unverified on real hardware**, M2 is
  incomplete, and ExecutionFailed/E_PROTECTION remain unknown with no fix,
  waiver or retry.

Non-blocking notes (no change required before push):

- **S-N1 (nit).** The PROGRESS heading changed from `## Current state —
  2026-11-12` (em dash) to `- ` (hyphen). Nothing parses this heading. Only
  prior review prose quotes it. It is cosmetic.
- **S-N2 (advisory, pre-existing practice).** The retained Sol design review
  (line 17) records a full local worktree path that includes the owner's
  Windows account name. The same account name already appears in at least ten
  tracked files on main 1604db1, so pushing it adds no new disclosure. If the
  coordinator wants to align with the placeholder convention the new evidence
  uses, a one-line docs-only substitution is enough and needs no source
  re-review. It is not a gate.

## Spec

**PASS. The patch matches the accepted proposal and coordinator disposition.**

- **Workflow.** One job-level `env:` block with sole
  `CARGO_PROFILE_DEV_DEBUG: '0'`, placed immediately after `timeout-minutes: 12`
  and before `steps:`. No `CARGO_PROFILE_TEST_DEBUG`, cache, runner label,
  safety-profile or Cargo edit. Clippy, the macOS stock `cargo test` and
  nextest all inherit the setting.
- **Node guard** (20 to 21 tests). It asserts:
  - the exact four-line block;
  - exactly one `env:` in `ci.yml`;
  - `CARGO_PROFILE_DEV_DEBUG` as the sole `CARGO_*` token;
  - no `[profile` table in `Cargo.toml`, `.cargo/config` or
    `.cargo/config.toml`.

  The prior headless-shell, matrix, concurrency and timeout assertions are
  unchanged.
- **Rust guard.** It has a black-boxed `cfg!(debug_assertions)`, a black-boxed
  `u8` overflow inside `catch_unwind`, and a direct
  `Location::caller().file() == file!()`. There is no global panic hook. The
  proposal and evidence describe the limits accurately:
  - the location check is a direct compiled-call-site smoke, not caught-panic
    diagnostics;
  - `catch_unwind` checks test-harness unwinding, not a production panic mode;
  - cc's frame-pointer codegen changes as well as symbols; there is no
    `NDEBUG` and no change to Rust assertion semantics;
  - build dependencies get `debug = false` "when possible", not always;
  - A3 is approximate comparative evidence, not causal proof.
- **Ledger.** The evidence ledger records failures honestly: the initial
  vitest-missing `npm test`, Clippy exit 101, and the black_box correction.
  The mutation run IDs and kill messages match the commands described. I did
  not re-run Sol's full-suite commands. I credit Sol for Node 21 / UI 1, npm
  lint/build, workspace all-target Clippy, the 9 Node mutants and the 2
  compiled DEV_* Rust kills.

No missing, partial or unrequested behavior.

### Independent checks I ran (process-scoped env only, functional, no speed claim)

| Check | Result |
| --- | --- |
| `node --test scripts/ci-workflow.test.mjs` in worktree | exit 0, 3/3 |
| Own Node mutants, each in a scratch copy of the 4 inputs (worktree never edited) | |
| A: value `'0'` → `2` | exit 1, assertion kill |
| B: step-level `env:` with `RUSTFLAGS` on the nextest step | exit 1, assertion kill |
| C: add `CARGO_PROFILE_TEST_DEBUG: '2'` to the job env | exit 1, assertion kill |
| D: `.cargo/config.toml` with `[profile.test] debug = 2` | exit 1, assertion kill |
| E: top-level workflow `env:` with `CARGO_PROFILE_TEST_OVERFLOW_CHECKS` | exit 1, assertion kill |
| F: `.cargo/config.toml` `[build] rustflags = ["-C","overflow-checks=off"]` | exit 0, survives the Node guard as expected; caught by the Rust guard below |
| `cargo nextest run -p arx-guest --locked --offline -E test(ci_test_profile…)`, DEV_DEBUG=0, scratch target dir | baseline exit 0, `test` profile `[unoptimized]`, 1 passed |
| + `CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=false` | exit 100, 1 failed: "debug assertions must stay enabled in test builds" |
| + `CARGO_PROFILE_TEST_OVERFLOW_CHECKS=false` | exit 100, 1 failed: "overflow checks and unwinding panics must stay enabled" |
| + `RUSTFLAGS="-C overflow-checks=off"` (mutant F's effect) | exit 100, 1 failed: same overflow assertion |
| Unmutated rerun | exit 0, 1 passed |
| Full `arx-guest` nextest, DEV_DEBUG=0 | exit 0, 2/2 |
| `cargo clippy -p arx-guest --all-targets --locked --offline -- -D warnings`, DEV_DEBUG=0 | exit 0, `dev` profile `[unoptimized]` |
| `cargo fmt --all --check` | exit 0 |

My Rust mutants override the `test` profile and `RUSTFLAGS`, not Sol's `DEV_*`
keys. So they independently show that the guard catches the inheriting-profile
and rustflags routes too. The scratch target dir sat outside the worktree.
Every mutation was a scratch copy or a child-process environment, and there was
nothing to restore in the worktree. I ran no workspace-wide nextest, benchmark,
stress test, native test or toolchain install.

Residual (informational, not a finding):

- `[build] rustflags = ["-C","debuginfo=2"]` in a future `.cargo/config.toml`
  would re-enable debuginfo without tripping either guard. The A2 banners
  would still read `[unoptimized]`, because they reflect the profile, not
  rustflags.
- That affects speed only, not safety, and A3 would expose it. Adding
  rustflags to the Node guard is optional future hardening. It is not needed
  for this attempt.

## Verdict

- **Standards: PASS.** 0 blocking findings. Non-blocking: S-N1 (cosmetic
  PROGRESS heading dash) and S-N2 (pre-existing owner path in the retained Sol
  review).
- **Spec: PASS.** 0 findings.
- **PASS for push.** The coordinator may push this substantive head to PR #54
  for **hosted attempt 2**. That allows only the push. It does not qualify the
  head for `ci-speed`, does not satisfy A6, and does not authorize a merge.

Prospective criteria are unchanged and frozen. Only attempt 1 of the first new
substantive head counts:

- **A1.** All three OS jobs succeed in below 900s (also below 906s).
- **A2.** `[unoptimized]` nextest banners on every OS, plus the macOS stock
  test and the Clippy dev banner.
- **A3.** Windows nextest compile below 108s **and** the unrounded
  compile/Clippy ratio below 1.12. This is comparative evidence, not causal
  proof.
- **A4.** Test counts:
  - Windows 307 passed / 2 skipped;
  - macOS 245 / 1;
  - Ubuntu 244 / 3;
  - Node 21;
  - macOS stock passes;
  - UI smoke 1/1 and desktop smoke 1/1 on each OS.
- **A5.** The three implementation paths plus the named evidence, PROGRESS and
  retained proposal/review notes.
- **A6.** An independent measured review before `ci-speed`.

Attempt 1 stays failed: 848s total, and the Windows install took 200s against
a 141s limit. There is no retroactive waiver. No empty commit, rebase or
rerun resets a result. After three failed performance attempts, hand off to the
other provider. The parent should wait on the tracked run explicitly after the
push, not rely on a PR watch alone. No hosted run or timing exists, and I claim
none.

## Limits and non-actions

I authored only this file and committed it to the target branch. I made:

- no source or PROGRESS edits;
- no GitHub writes, push, label, PR, watch, merge, rerun, dispatch or settings
  change;
- no toolchain install;
- no native, root, owner, account, service, SCM/LSA, VM or Lab Manager
  operation;
- no private-state, ignored-fixture, experiment or babysitter operation;
- no subagent.

M2 remains incomplete. Native, installed and dedicated paths remain
**unverified on real hardware**. ExecutionFailed and E_PROTECTION remain
unknown, with no fix, waiver or retry.

Standards: 0 blocking findings (worst: cosmetic heading dash and pre-existing
owner path, both advisory). Spec: 0 findings.
