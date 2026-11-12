# CI performance attempt 2: proposal (Rust debuginfo off in CI)

Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-12. Base aef105a
(local docs record on PR #54 source 48d4916; actual main 1604db1). Role:
architecture author. **This note is the author's own proposal, not an
independent review.** Before anything is implemented or measured, a reviewer
from a different provider (Sol, or grok-4.7 high as fallback) must review it.
Nothing was implemented, pushed, labelled or merged. I made no GitHub writes
and started no subagents.

Attempt 1 (head 48d4916, run 43248592153) stays failed under its predeclared
criteria. Its 848s success is not proof for any new head.

## Where the time goes (attempt 1 of each run, read from the public API and job logs)

The policy duration is `updated_at - run_started_at` of attempt 1, so the
slowest job's queue time plus its run time sets the total.

| Run | Windows job | macOS queue | macOS job | Total |
| --- | --- | --- | --- | --- |
| 43248592153 (candidate 48d4916) | 634s | **612s** (15:30:51 to 15:41:03) | 234s | 848s |
| 43240797525 (main baseline) | 526s | never got a runner, cancelled | none | 906s |
| 43233144276 | 583s | 3s | 204s | 596s |
| 43231164086 | 572s | 4s | 246s | 577s |
| 43221533019 | 433s | 3s | 210s | 439s |
| 43213333692 | 633s | 3s | 217s | 637s |
| 43204089423 | 559s | 3s | 126s | 564s |

- When macOS gets a runner promptly, Windows is the critical path. Its main
  costs are Playwright install (141 to 210s), `cargo nextest` (157 to 244s) and
  clippy (66 to 97s).
- In attempt 1, Windows nextest split into a 121s compile (`Finished test
  profile ... in 2m 01s`) and a 121.5s test run. The run phase is bounded by
  one 88.9s test, `activity_v2::read_flood_preserves_actions_and_lifecycle`.
  Shortening it would change coverage, so it is out of scope.
- In attempt 1, macOS queueing (612s) was the critical path. No workflow change
  can shorten a hosted-runner queue.

## Options assessed

1. **Media Foundation skip on Windows: rejected.** The `install_media_pack.ps1`
   pinned at
   [v1.65.0](https://github.com/microsoft/playwright/blob/v1.65.0/packages/playwright-core/bin/install_media_pack.ps1)
   runs `Install-WindowsFeature Server-Media-Foundation` on Windows Server.
   The pinned
   [`dependencies.ts`](https://github.com/microsoft/playwright/blob/v1.65.0/packages/playwright-core/src/server/registry/dependencies.ts)
   runs it whenever the `chromium` dependency group is installed. Upstream
   declares it a Chromium dependency on our runner OS, and I found no primary
   source saying headless shell does not need it. Dropping or conditionalizing
   it would remove a declared dependency without support, so it is not
   proposed. It saves the most time (about 193s), so revisiting it needs a
   reviewed ADR with primary-source evidence, not a CI trial-and-error run.
2. **Running the Windows Playwright install in the background, overlapped with
   cargo: deferred.** It keeps `--with-deps` and could save up to about 190s
   on the Windows path. However, it reorders steps, adds background-process
   exit-code propagation in pwsh, and DISM servicing may contend with
   compilation. That is a larger, riskier change. It is a possible attempt-3
   proposal and needs its own prior review.
3. **Warming the cache: rejected.** No main cache exists, so it cannot
   produce a cold-run saving, and it adds trust surface.
4. **Skipping or narrowing the macOS arm64 or Intel coverage: forbidden** by
   the KICKOFF and the task.
5. **Turning Rust debuginfo off for CI dev/test builds: recommended (below).**

## Recommended minimal patch

The patch touches only `.github/workflows/ci.yml`, `scripts/ci-workflow.test.mjs`,
`crates/arx-guest/tests/cli.rs` and the evidence record. No other workflow,
action, permission, matrix, check name, timeout, concurrency setting, policy
script, cache or step order changes.

```yaml
    runs-on: ${{ matrix.os }}
    timeout-minutes: 12
    env:
      CARGO_PROFILE_DEV_DEBUG: '0'
    steps:
```

Why this patch is safe, from the pinned Cargo 1.95.0 official docs:

- [Profiles](https://doc.rust-lang.org/1.95.0/cargo/reference/profiles.html):
  `0`/`false`/`"none"` means no debug info. The dev defaults (opt-level 0,
  debug-assertions true, overflow-checks true, panic unwind, strip "none") are
  separate keys and stay unchanged. "The test profile inherits the settings
  from the dev profile."
- The `build-override` for build scripts and proc macros already defaults to
  `debug = false`, so they are unaffected.
- [Config](https://doc.rust-lang.org/1.95.0/cargo/reference/config.html):
  `profile.<name>.debug` maps to the environment variable `CARGO_PROFILE_<name>_DEBUG`.

What I verified locally (read-only, scratch target dirs, since deleted, no
toolchain install, `--offline`):

- The env var reaches the `test` profile. Cold `cargo test --workspace --locked
  --no-run` printed `test profile [unoptimized]` with the env var set and
  `[unoptimized + debuginfo]` without it.
- Two cold pairs, run in opposite orders: 53.3s vs 43.0s and 51.8s vs 43.4s
  (about 17 to 19% less), with target size going from 1.8G to 935M.
- One dev machine, non-qualifying. **This is not a CI speed claim.**

Assumptions I checked:

- No source, test or script in the repo references debuginfo, PDB files,
  backtraces or `CARGO_PROFILE` settings.
- `libsqlite3-sys` 0.35.0 `build.rs` does not use `DEBUG` or `NDEBUG`.
- cc 1.11.1 reads Cargo's `DEBUG` only for debug flags and never defines
  `NDEBUG`, so SQLite C semantics stay the same. Only `-g`/`/Z7` are omitted.
- Panic messages keep file:line, because `core::panic::Location` is compiled
  in and does not come from DWARF/PDB.

Trade-off: full `RUST_BACKTRACE` backtraces in CI lose line info. That is a
diagnostic cost only. Coverage, assertions and runtime semantics do not
change. Rollback is a one-block revert plus its guard.

Setting the variable at job level means clippy, the macOS stock-directory
`cargo test`, and nextest all share one profile, so no extra rebuilds happen.
Do not add `CARGO_PROFILE_TEST_DEBUG`: local evidence shows `test` inherits.

## Tests, guards and mutants

1. Add a guard in `scripts/ci-workflow.test.mjs` (Node tests go from 20 to
   21). It asserts:
   - the exact block `timeout-minutes: 12` / `env:` / `CARGO_PROFILE_DEV_DEBUG: '0'` / `steps:`
     at job indentation;
   - that `ci.yml` has exactly one `env:` and its only `CARGO_*` token is
     `CARGO_PROFILE_DEV_DEBUG`;
   - that neither `Cargo.toml` nor `.cargo/config(.toml)` (if one is ever
     created) has a `[profile` table.

   The existing headless-shell, matrix, concurrency and timeout assertions
   stay.
2. Add a Rust runtime guard to the existing `crates/arx-guest/tests/cli.rs`.
   It adds no new test binary, so no extra link time.

   ```rust
   #[test]
   fn ci_test_profile_keeps_runtime_checks_when_debuginfo_is_off() {
       assert!(cfg!(debug_assertions), "debug assertions must stay enabled in test builds");
       let overflowed = std::panic::catch_unwind(|| std::hint::black_box(u8::MAX) + std::hint::black_box(1u8));
       assert!(overflowed.is_err(), "overflow checks and unwinding panics must stay enabled");
       assert!(std::panic::Location::caller().file().ends_with("cli.rs"), "panic locations do not need debuginfo");
   }
   ```

   It runs on all three OSes in CI with the new env var.
3. Run each mutant locally and restore it afterwards. Each must fail:

   | # | Mutant | Fails in |
   | --- | --- | --- |
   | M1 | Delete the env block | Node guard |
   | M2 | Add `CARGO_PROFILE_DEV_DEBUG_ASSERTIONS: 'false'` to the env block | Node guard |
   | M2 (local) | Run `cargo nextest run -p arx-guest` with `CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=false` | Rust guard |
   | M3 | Run locally with `CARGO_PROFILE_DEV_OVERFLOW_CHECKS=false` | Rust guard |
   | M4 | Move the env var to the nextest step only | Node guard |
   | M5 | Add `[profile.dev]` with `debug = 0` to `Cargo.toml` | Node guard |
   | M6 | Change the value to `'line-tables-only'` | Node guard |

   The four attempt-1 mutants must still be caught.
4. Local checks before pushing:
   - `npm test`, `npm run lint`, `npm run build`
   - `cargo fmt --all --check` and `cargo clippy --workspace --all-targets --locked -- -D warnings`
   - `CARGO_PROFILE_DEV_DEBUG=0 cargo nextest run -p arx-guest --locked`, which
     must show the `[unoptimized]` banner.

   Do not claim speed from local runs.

## Prospective acceptance criteria for attempt 2 (fixed before any measurement)

The exact candidate head is one substantive commit on PR #54, with no rebase,
so the Rust source equals 48d4916 plus the guard. Measure attempt 1 of its CI
run only. Do not rerun or push an empty commit.

- **A1.** The run succeeds and all three `verify (...)` jobs succeed. Policy
  duration must be below 900s, which also keeps it below 906s.
- **A2.** On every OS, the nextest banner and the macOS stock-directory banner
  read ``Finished `test` profile [unoptimized]``. Clippy reads
  ``Finished `dev` profile [unoptimized]``.
- **A3 (new causal metric).** The Windows nextest compile phase (its `Finished
  test profile ... in` value) must meet both conditions:
  - strictly below **108s**, the median of the seven-sample baseline (89, 113,
    108, 73, 106, 108, 121 from jobs 131260319775, 131233422108, 131226519288,
    131193987500, 131166213791, 131134713331, 131287906938);
  - strictly below **1.12**, the sample minimum, when divided by the same
    job's clippy `Finished dev profile` time. The baseline ratios are
    89/76, 113/91, 108/88, 73/65, 106/86, 108/82 and 121/95, so 1.17, 1.24,
    1.23, 1.12, 1.23, 1.32 and 1.27.

  The ratio controls for runner speed. Clippy runs no codegen, so debuginfo
  does not affect it. If SQLite's build script gets slightly faster, the ratio
  gets harder to meet, not easier.
- **A4.** Per-OS test counts equal attempt 1 plus the one new guard:
  - Windows 307 passed, 2 skipped;
  - macOS 245 passed, 1 skipped;
  - Ubuntu 244 passed, 3 skipped.

  The macOS stock-directory test passes. UI smoke is 1/1 and desktop smoke is
  1/1 on every OS. CI runs 21 Node tests.
- **A5.** The diff is limited to the four files above. Check names, matrix,
  `macos-latest`, timeouts, concurrency, permissions, action pins and policy
  are byte-identical.
- **A6.** A different-provider independent reviewer verifies A1 to A5 from the
  API and logs before anyone considers `ci-speed`.

Failure handling:

- Any miss, including a capacity failure, a queue that pushes the total past
  900s, or A3 missing by noise, is failed attempt 2 of 3. It stays red and gets
  recorded. Do not relax a criterion or reuse an old run.
- A3 is the only causal evidence. A passing total wall time is not attributed
  to this change beyond the Windows phase.

## Measurement plan

1. After the push, the coordinator records the run id for `head_sha` in the
   `CI` workflow.
2. The coordinator waits on that tracked run's completion explicitly (the
   watcher missed the first completion), with no polling loop.
3. The coordinator then reads `actions/runs/<id>/attempts/1`, the jobs with
   step timestamps, and the three job logs. From the logs, grep the `Finished`
   banners, the nextest `Summary`/`Starting` lines and the Playwright pass
   lines.
4. Record the results as an "Attempt 2" section in
   `docs/evidence/ci-headless-shell.md`, or in a sibling evidence file.

## Honest residual risk

- The expected Windows saving is roughly 20 to 25s (CI compile 121s times
  about 0.8), plus a few seconds on Ubuntu and macOS. That is a genuine,
  coverage-neutral reduction, but small.
- It does not fix GitHub runner allocation. Attempt 1 needed a 612s macOS
  queue to land at 848s. A queue above about 660s, or a failure to get a
  runner (as in 906s main run 43240797525), fails A1 regardless of code.
- If capacity keeps failing first attempts, no legitimate in-repo route keeps
  first-attempt CI under 900s while keeping macOS arm64 and the current
  policy. Options like larger or self-hosted runners, or a policy change, are
  owner decisions. This note records that as a blocker for the coordinator to
  report and does not ask the owner through PR comments.
- Stage C stays unlabelled and budget-blocked. ExecutionFailed and
  E_PROTECTION remain unknown, with no fix, waiver or retry. Native, installed
  and dedicated paths are **unverified on real hardware**, and M2 is
  incomplete.
