# CI performance attempt 2: independent Sol design review

OpenAI / gpt-6.1-sol / high, 2026-11-12. Single-agent, different-provider
review of the Opus proposal; no implementation or performance measurement.

Reviewed head: `2067e234cfdbc33ee77202024e76a20d1a8fced9` (proposal cherry-pick
of `a57abcb`), base: `aef105a78acbff60fb744cef52b664a6afe0c452`.
`git log aef105a..HEAD` contains only the proposal commit; the triple-dot
diff adds only the complete 242-line proposal. Published PR54 source is
`48d4916e556cdcd78a0a7e58fa25c733bcc2814b`; actual main is
`1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`. Neither is substituted for
the design review target. Workflow, scripts, crates, Cargo manifests/lock and
toolchain have no diff from published 48d4916.

The prepared T3 worktree was clean at that exact HEAD. Its working directory
and Git top level both resolve to
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-036d71ee-902e5c315c6e3ed5`,
distinct from canonical `D:/Code/Arxcovo`. No checkout or worktree was created.

## Standards

**CHANGES REQUIRED, documentation only.** The minimal job-level setting is
acceptable in principle. Two precise corrections must be incorporated before
implementation/measurement; the coordinator can make these small doc fixes
directly. This report does not approve an actual patch or satisfy agent-review
for a future implementation head.

1. **S1 — A5 contradicts mandatory progress and retained review records.**
   Proposal lines 62–65 and A5 restrict the patch to four files. AGENTS and
   KICKOFF require current PROGRESS and provider/outcome evidence, and this
   proposal/review must remain retained. Replace the whitelist with exactly
   three implementation paths: `.github/workflows/ci.yml`,
   `scripts/ci-workflow.test.mjs`, `crates/arx-guest/tests/cli.rs`; additionally
   permit the named CI evidence record, `docs/PROGRESS.md`, retained attempt-2
   proposal and review notes. No other product, workflow or policy files.
   Define implementation scope against 48d4916 (or subtract its already
   reviewed attempt-1 delta when comparing with main). Accept this amendment
   before the run; do not excuse an out-of-scope diff after measurement.
2. **S2 — C debug-flag and build-dependency claims are too absolute.**
   In the assumptions, replace “Only `-g`/`/Z7` are omitted.” Pinned cc
   1.11.1 also defaults frame-pointer forcing to its debug setting, affecting
   `-fno-omit-frame-pointer` and supported leaf-frame-pointer flags on GCC/
   Clang. This changes code generation/diagnostics, without changing the
   SQLite assertion macros or Rust safety-check settings. No `NDEBUG` or
   `DEBUG` conditional is present in the pinned SQLite build script, and cc
   does not introduce `NDEBUG`. Also qualify “build scripts and proc macros
   ... unaffected”: Cargo documents `debug = false` **when possible**, with
   shared normal/build dependencies potentially compiled once. These are
   source-based safety conclusions, not observed cross-platform C builds.

Repository standards inspected: AGENTS, full KICKOFF, CONTRIBUTING, review
skill and its active profile. No additional nested AGENTS was found in the
tracked scope. Fowler smell baseline considered; no separate actionable smell
in this one-document design diff. No new trust or runtime activation is
introduced by the recommended setting.

## Spec

**CHANGES REQUIRED, documentation only.** Preserve the proposed direction and
all numerical thresholds; make the following claims accurate before running.

1. **SP1 — Direct caller check does not inspect the caught panic location.**
   `Location::caller().file().ends_with("cli.rs")` checks the source location
   of that direct call, not the earlier overflow panic, its hook output, or
   its file/line. The minimum acceptable correction is to describe it as a
   compiled caller-location smoke check, with that explicit limitation; the
   pure check is adequate for this limited claim. Prefer comparing to
   `file!()` and optionally a captured `line!()` to pin the direct call.
   Retain `cfg!(debug_assertions)` and the black-boxed overflow inside
   `catch_unwind`: these meaningfully check assertions, overflow and test
   unwinding. They do not prove a separate production binary's panic mode;
   Cargo test harnesses require unwinding. Do not install a global panic hook
   in parallel tests. If actual panic diagnostic output becomes required,
   use a separately isolated subprocess rather than shared hook state.
2. **SP2 — A3 is an approximate control, not causal proof.** Rename “new
   causal metric” and “only causal evidence” to a predeclared comparative
   compile-time metric. Clippy normally checks targets rather than codegen
   for them, but still compiles/runs build dependencies and native SQLite;
   its denominator is not guaranteed unaffected by debuginfo or runner noise.
   Keep the exact seven jobs, below-108s and strictly-below-1.12 gates.
   Arithmetic independently checked: sorted compile seconds are
   73, 89, 106, 108, 108, 113, 121; median 108. Exact minimum ratio is
   `73/65 = 1.123076923...`; 1.12 is a rounded representation and the strict
   threshold is conservatively stronger. Do not round candidate ratios
   before comparing. Passing supports an observed phase reduction; it does
   not isolate causality or prove wall-time savings from this change.

The following parts are acceptable and must survive those corrections:

- A1–A6 are frozen before measurement. Only attempt 1 of the first substantive
  new candidate head counts; no rerun, empty commit, label or rebase resets a
  failed result. Bind logs/API evidence to the exact head and attempt.
- A1 requires successful CI/all three jobs below 900s (also below 906s).
  A2 checks `[unoptimized]` test banners for nextest and macOS stock-directory
  tests and the dev banner for clippy. These share the inherited debug setting,
  not necessarily identical Cargo compilation units: normal incremental reuse
  and a `Finished ... in 0.00s` banner are valid. No cache warmup is needed.
- A4 stays Windows 307 passed/2 skipped, macOS 245/1, Ubuntu 244/3, plus
  passing macOS stock observation, UI and desktop smoke 1/1 each per OS and
  21 Node script tests. These are prospective requirements, not reviewer-run
  results. The current source declares 20 Node script tests; the proposed
  guard adds one, and cli.rs gains one Rust test without a new test binary.
- Preserve all three exact `verify (...)` names, Ubuntu/Windows/macOS including
  existing arm64 coverage, 12-minute jobs, 15-minute policy, action pins,
  permissions, concurrency, step order, dependencies and both smokes. No
  Media Foundation skip, runner narrowing, policy change or new cache.
- Keep the exact job-level `CARGO_PROFILE_DEV_DEBUG: '0'` guard, sole Cargo
  env token, no profile-table overrides, and the proposed mutation checks.
  Local checks and mutation work belong to the future implementation task;
  they were not executed in this design review.
- Attempt 1 remains NOT QUALIFYING: 848s total but 200s Windows install
  exceeds its predeclared 141s limit. This new proposal is prospective, not a
  retroactive waiver. PR54 remains unlabelled/budget-red at 48d4916; PR53
  remains unlabelled/blocked. Main's 906s first attempt remains authoritative
  despite recovery on attempt 2. These states are retained supplied/local
  evidence, not a fresh live GitHub status check.
- A6 requires independent measured review of A1–A5 before considering
  `ci-speed`; design approval alone cannot justify the label. Fresh main
  success within budget is still needed before draining the feature queue.

## Source checks, credit and limits

Read the entire proposal, actual workflow, existing workflow guards, Cargo
workspace/toolchain and locked dependency versions, guest CLI test, smoke
configs, package scripts, policy/duration source, current progress and retained
attempt-1 evidence. Used read-only Git comparisons and a small arithmetic
calculation. No application tests, builds, mutants or benchmarks were run.

Primary sources independently fetched with bounded public GETs:

- [Cargo 1.95 profiles](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html)
  and [configuration](https://doc.rust-lang.org/1.95.0/cargo/reference/config.html):
  test inherits dev; debug info, opt-level, debug assertions, overflow, panic
  and strip are separate settings. The sole env change leaves the latter
  settings at their existing defaults; no TEST_DEBUG override is necessary.
- [Rust 1.95 Location](https://doc.rust-lang.org/1.95.0/core/panic/struct.Location.html):
  direct caller semantics and track-caller propagation support SP1.
- [cc 1.11.1 source](https://docs.rs/crate/cc/1.11.1/source/src/lib.rs) and
  [libsqlite3-sys 0.35.0 build.rs](https://docs.rs/crate/libsqlite3-sys/0.35.0/source/build.rs):
  inspected debug/frame-pointer selection and bundled SQLite flags (S2).
  Web rendering failed for several URLs; bounded Python urllib GETs succeeded.

Credit Opus for the proposal and its reported opposite-order cold-build pairs
(53.3/43.0s and 51.8/43.4s). Those are designer-local evidence, not independently
reproduced here and not CI proof. Historical job timings/counts are credited
to the proposal/coordinator records; only their stated arithmetic and source
compatibility were checked here. No fresh Actions log measurement is claimed.

Only this review file is authored in this task. No source/PROGRESS edits,
GitHub writes, push, label, PR, watch, merge, rerun, settings change, toolchain
install, native/owner/admin/account/service/SCM/LSA/VM/Lab Manager operation,
experiment, private-state or ignored-fixture access, or subagent occurred.
ExecutionFailed and E_PROTECTION remain unknown with no fix, waiver or retry.
M2 remains incomplete; native, installed and dedicated paths remain
**unverified on real hardware**.

Disposition: coordinator corrects/accepts S1–S2 and SP1–SP2 before any run,
then Sol implements and Opus independently reviews the actual patch. No CI
wait in this review task; completion returns control to the parent coordinator.

Standards: 2 findings, worst is the A5/progress scope conflict. Spec: 2 findings,
worst is unsupported causal attribution in A3.
