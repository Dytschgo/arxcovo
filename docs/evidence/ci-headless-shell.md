# CI headless-shell candidate

OpenAI / gpt-6.1-sol / medium coordinator, 2026-11-12. Base is main after
#52, 1604db1. This is a separate CI performance candidate, not a Stage C
feature change and not a claim that hosted macOS capacity is fixed.

Main run 43240797525 attempt 1 failed at 906s. Its macOS job 131260319040
had no runner name or steps; GitHub annotated hosted runner acquisition failure
and macOS arm64 capacity constraints. Ubuntu and Windows passed. One failed-job
retry completed successfully, but policy deliberately measures attempt 1.
That failed over-budget baseline is retained; no status is manually overridden.

The only workflow change adds --only-shell to the existing signed-dependency
Chromium installation step. Matrix, names, arm64 macOS coverage, action pins,
permissions, concurrency, timeouts, dependencies and smoke steps are unchanged.
No new action, cache, trust surface or policy change is introduced.

[Playwright's browser documentation](https://playwright.dev/docs/browsers#chromium-headless-shell)
supports omitting regular Chromium when no channel is selected and tests use
headless shell. The pinned installed 1.65 CLI advertises --only-shell, and a
dry run selects headless shell and existing support binaries without regular
Chromium. This dry run downloaded or installed nothing. Our root config has
no channel or headed override; the UI smoke uses the default page fixture.
Desktop smoke uses _electron.launch and the application's Electron binary.
The config and all smoke test source remain unchanged.

## Baseline measurements

Coordinator read the Actions API for attempt 1 of these six main runs.
All listed Windows install steps passed. These are step durations, not build
execution time alone or claims about runner queueing.

| Main run | Windows job | Install start UTC | Install end UTC | Seconds |
| --- | --- | --- | --- | --- |
| 43240797525 | 131260319775 | 14:52:59 | 14:55:43 | 164 |
| 43233144276 | 131233422108 | 14:08:50 | 14:12:12 | 202 |
| 43231164086 | 131226519288 | 13:56:26 | 13:59:45 | 199 |
| 43221533019 | 131193987500 | 12:54:43 | 12:57:04 | 141 |
| 43213333692 | 131166213791 | 12:02:29 | 12:05:59 | 210 |
| 43204089423 | 131134713331 | 10:57:40 | 11:00:54 | 194 |

All timestamps are 2026-11-12. This explicitly pinned sample has minimum 141s
and median 196.5s; it is not the reviewer's differently selected sample.
Network variability prevents attributing every second to browser selection.

## Validation and acceptance

- Node script tests: 20 passed.
- Four workflow/config mutants were caught by runtime assertions and restored:
  remove --only-shell; remove macOS; add channel; set headless false.
- npm run lint and npm run build passed.
- Local Windows UI smoke 1/1 and Electron smoke 1/1 passed using existing
  installed binaries. No browser install was performed for these checks.
- No Rust source changed; no cargo or native service tests were run here.

Hosted evidence is pending. Before applying ci-speed, the exact candidate's
FIRST attempt must succeed on all three OS jobs, finish below both 900s and
the 906s baseline, retain UI smoke 1/1 and desktop smoke 1/1 on every OS, and
show a Windows install step below this sample's 141s minimum. The independent
reviewer must verify the unchanged coverage and the measured saving. No speed
claim or label is justified by this local run alone. A capacity or functional
failure stays red; no attempt-count reset or empty commit is a measurement.

Only then may the existing ci-speed exception evaluate the separate genuine
performance PR. No policy code changes or manual success statuses. Stage C
remains unlabelled. After this PR merges, main itself must pass a fresh run
inside 15 minutes before the feature queue can drain. If the candidate fails
these criteria, keep it blocked and record the evidence rather than relabel.

Capacity can fail again; this change does not fix GitHub runner allocation.
ExecutionFailed and E_PROTECTION remain unresolved. Native, installed and
dedicated paths remain **unverified on real hardware**; M2 is incomplete.
No owner/admin/account/service, SCM/LSA, VM/Lab Manager, experiment, private
state, ignored fixture or babysitter operation occurred. Rollback is the
one-line workflow revert and its associated guard update; no installed state.


## Attempt 1: does not qualify

Head 48d4916, run 43248592153 attempt 1 succeeded on all OS jobs in 848s
(15:30:50 to 15:44:58 UTC). Windows install took 200s (15:37:51 to 15:41:11),
which fails the predeclared below-141s criterion. No ci-speed label is applied,
and no measurement criterion is relaxed after seeing the result.
The Windows log shows Media Foundation completion at 15:41:04, headless-shell
download beginning at 15:41:04 and support downloads finishing at 15:41:11.
This establishes the step's dominant observed phase, not a capacity fix or a
portable guarantee about dependency timing. Source review passed, but the
performance acceptance did not. This is failed performance attempt 1 of 3;
a materially different proposal needs prior independent review and new
measurement criteria. No empty commit or rerun resets this failure.


## Attempt 2: local implementation, hosted measurement pending

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-12.
Prepared base: `d3bc3aa0c2d15b8661543f401b100fc853b4079d`.
Published PR54 source: `48d4916e556cdcd78a0a7e58fa25c733bcc2814b`.
Actual main reference: `1604db1fbd686d3bf59e81f4d2702eaecc6b8bc0`.
Source commit: `c3285a8d6fd76c3a3ebcf311ac2c2529d6cfd732`.
Evidence head is the subsequent docs-only commit containing this section
(resolve with `git log -1 --format=%H -- docs/evidence/ci-headless-shell.md`);
no self-referential commit hash is claimed.

Initial pwd and Git top level both matched `<prepared-worktree>`, distinct
from canonical `D:/Code/Arxcovo`; status was clean at the exact prepared base.
No checkout/worktree was created and no canonical writes occurred. Commands
used explicit prepared working directories and checked exits. Local evidence
uses the prepared placeholder rather than owner account paths.

Read full AGENTS/KICKOFF, complete proposal including current coordinator
disposition, and independent Sol high design review (6b9af143 retained at
1afa852). S1/S2/SP1/SP2 were accepted at the prepared base before implementation.
The disposition supersedes older causal shorthand. Proposal/review documents
are retained unchanged by this implementation. Fresh independent Opus high
review of the actual patch is still REQUIRED before any push or measurement.
No subagent or review was started here.

The source commit changes exactly `.github/workflows/ci.yml`,
`scripts/ci-workflow.test.mjs`, `crates/arx-guest/tests/cli.rs` against48d4916.
The job-level block immediately follows timeout12 and precedes steps, with
sole `CARGO_PROFILE_DEV_DEBUG: '0'`; no TEST_DEBUG is added (test inherits dev).
One Node guard and one test in the existing guest CLI binary are added.
The existing scaffold test remains benign. The debug-assertion cfg value goes
through black_box to retain a runtime check while satisfying Clippy's
assertions_on_constants lint. No lint override is used.

Direct Location::caller().file()==file!() is a compiled direct-call-site
smoke only. It does not observe caught panic location/output. catch_unwind
checks test-harness unwinding, not production-binary panic behavior. No global
panic hook is installed. cc's DEBUG-dependent frame-pointer default can change
C code generation/diagnostics as well as symbols; this is not a symbols-only
claim. No NDEBUG or Rust safety-setting change is introduced.

### Executed local checks (functional, not speed measurements)

All checks below ran in this prepared Windows worktree. Cargo child processes
used CARGO_PROFILE_DEV_DEBUG=0. No environment setting was persisted to user or
system scope. No cold speed benchmark, stress test or full workspace nextest
was run for this CI-only change.

| Command | Exit and observation |
| --- | --- |
| Initial `npm test` | 1: Node21 passed; UI could not find vitest because worktree dependencies were absent. Lint/build not reached in that sequence. |
| Initial `cargo fmt --all --check` | 0 |
| Initial `cargo clippy --workspace --all-targets --locked -- -D warnings` | 101: new constant cfg assertion triggered assertions_on_constants; nextest not reached. Corrected with black_box as described above. |
| `npm ci` | 0, locked dependencies installed; no lockfile/source change |
| `npm test` after install | 0: Node21/21, UI1/1, no skipped/dummy tests |
| `npm run lint` | 0 |
| `npm run build` | 0 |
| `cargo fmt --all --check` after correction | 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` after correction | 0: Finished dev profile [unoptimized] |
| `cargo nextest run -p arx-guest --locked` baseline | 0: Finished test profile [unoptimized]; 2 passed, 0 skipped; run ed429954-3941-4575-9410-60bec8c804ea |
| Same guest command after all env mutants | 0: Finished test profile [unoptimized]; 2 passed, 0 skipped; run eaaf18df-d122-4f87-b6bb-8021e4cd311c |
| `git diff --check` | 0 before source commit |

No new local Playwright/Electron smoke is claimed; their existing steps and
coverage are unchanged. Local Windows checks do not satisfy hosted Unix,
macOS stock-directory, all-workspace counts or any speed criterion. Build
banner durations are deliberately not used as performance evidence.

### Restored mutations

Node mutants each ran `node --test scripts/ci-workflow.test.mjs`, exited1,
and were killed by runtime assertions (ERR_ASSERTION/AssertionError), not
parse/compile errors. All edited file bytes were saved and restored in finally
blocks, checked byte-for-byte, and checked again for every file at suite end.
The restored focused Node baseline exited0, all three workflow tests passed.

| Mutant | Result |
| --- | --- |
| M1 remove job env block | Node assertion kill |
| M2 add CARGO_PROFILE_DEV_DEBUG_ASSERTIONS: 'false' | Node assertion kill |
| M4 move sole env to nextest step | Node assertion kill |
| M5 append [profile.dev] / debug=0 to Cargo.toml | Node assertion kill; manifest restored, never used for a build |
| M6 change value to 'line-tables-only' | Node assertion kill |
| Old H1 remove --only-shell | Node assertion kill |
| Old H2 remove macOS matrix member | Node assertion kill |
| Old H3 add channel property in Playwright config scratch mutation | Node assertion kill |
| Old H4 add headless:false in Playwright config scratch mutation | Node assertion kill |

M2 local and M3 used the actual compiled test with
`cargo nextest run -p arx-guest --locked -E test(ci_test_profile_keeps_runtime_checks_when_debuginfo_is_off)`.
Each inherited a copied child-process environment with DEV_DEBUG=0, and only
its named additional override=false. Parent environment was compared before
and after in finally and unchanged; no user/system environment mutation.

- M2 local, CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=false: compiled [unoptimized],
  nextest exit100, 0 passed / 1 failed / 1 filtered; run
  cb1b5445-3e1d-4d6c-bd69-69e7d4b1db66. The new guard failed the assertion
  “debug assertions must stay enabled in test builds”.
- M3 local, CARGO_PROFILE_DEV_OVERFLOW_CHECKS=false: compiled [unoptimized],
  nextest exit100, 0 passed / 1 failed / 1 filtered; run
  ca3f6664-8e74-48b0-9d2f-c0ffae9a6185. The new guard failed the assertion
  “overflow checks and unwinding panics must stay enabled”.

Both are compiled assertion kills, not noncompile results, zero-test passes,
skips or mere config-guard failures. The final unmutated guest run passed2/2.
No permanent Cargo/profile/config, dependency or safety-setting edit remains.

### Frozen future hosted acceptance and limits

No attempt-2 hosted run exists from this task; no run ID, duration, measured
review or speed label is claimed. Attempt1 remains failed: run43248592153,
848s overall, Windows install200s >141s. Design corrections/local failures do
not count as new hosted performance attempts. After three failed performance
attempts, hand off to the other provider.

The accepted proposal remains authoritative with all numerical criteria fixed:
A1 exact new substantive head FIRST attempt, all OS success <900s (also <906s);
A2 unoptimized nextest banners on all OS, macOS stock test and Clippy dev;
A3 Windows nextest compile <108s AND unrounded compile/Clippy ratio <1.12,
approximate comparative evidence, not causal proof;
A4 Windows307/2, macOS245/1, Ubuntu244/3 passed/skipped, macOS stock pass,
UI smoke1/1 and desktop smoke1/1 on every OS, Node21;
A5 exactly three source paths plus permitted docs;
A6 independent Opus measured review of A1–A5 before any label.
No empty commit, rebase, rerun reset, cache, runner architecture change,
dependency drop, timeout/policy exception or ci-speed label is authorized here.
All existing steps/order, action pins, names, permissions, concurrency,
three OS including macOS ARM, 12-minute job/15-minute policy, signed mirror,
Media Foundation --with-deps and UI/Electron checks remain unchanged.

ExecutionFailed/E_PROTECTION remain unknown with no fix, waiver or retry.
M2 is incomplete; native, installed and dedicated paths are **unverified on
real hardware**. No production NativeOps/create/Guard/session/account/service,
fixed-path pipe, root runner, cgroup/SCM/LSA, VM/Lab Manager, experiment,
private-state, ignored-fixture, toolchain installation, GitHub/push/PR/watch/
merge/rerun/dispatch/settings/label or babysitter operation occurred.

Final source-scope invariant check exited0: removing only the two added env
lines reproduces48d4916 workflow bytes; existing guest scaffold is a byte
prefix; full diff has exactly seven permitted paths including retained docs.
Proposal/review bytes equal prepared base, evidence original bytes are an
unchanged prefix, complete progress history is retained with exactly one
current heading and one current queue. Final focused Node3/3, fmt and
whitespace checks exited0 after source commit. No functional source edits
followed the passing Clippy/guest/npm checks; only a surplus blank line was
removed before the source commit.
