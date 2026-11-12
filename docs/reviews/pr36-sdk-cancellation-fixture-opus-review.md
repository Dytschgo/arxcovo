# PR36 SDK cancellation fixture — independent Opus review

Reviewer: Anthropic / claude-opus-5-5 / high, single review sub-agent, no
subagents, 2026-11-12. Builders under review: OpenAI / gpt-6.1-sol / high
(fixture) and OpenAI / gpt-6.1-sol / medium (integration).

**Verdict: PASS.** No blockers. Four non-blocking advisories below.

## Pins

- Reviewed HEAD `6ccfce947613b702e47838055a61b2d1efd76bbd`, parents
  `bd99d5c020f1ce40d184f81ba272107fdff35750` (fixture) and
  `3b418cd2254c05b90de460e2d78101b31a76f944` (main after #34).
- Fixture range `7f47d8d..bd99d5c`: only `crates/arxd/src/local_execution_tests.rs`,
  `docs/PROGRESS.md` and `docs/evidence/pr36-sdk-cancellation-fixture.md`.
- `git diff bd99d5c HEAD -- crates` is empty; `git diff 3b418cd HEAD -- .github tools
  docs/evidence/macos-manager-observations.json` is empty. The integration
  changes no Rust and no inherited #34 CI/observer file.
- Public PR36 remains `c044b72` (Windows red). Nothing was published.

## Standards axis

- No production change. The guard, session, executor, store and SDK paths are
  untouched. `fmt --check` and `git diff --check` against both parents pass.
- `docs/PROGRESS.md`, the evidence file and the test file are strict UTF-8 with
  no BOM, no CR, no trailing whitespace, no U+FFFD and no conflict markers. The
  CP1252 0x97 in the builder heading is now U+2014.
- Every line of the `3b418cd` PROGRESS is still present. Every line of the
  `bd99d5c` PROGRESS is still present except the one corrected heading. All
  provider and table rows are retained.
- No private data was added: the only matches are the existing fixture token
  digest pattern and the scope wording "private-state read".

## Spec axis (fixture behavior)

- rmcp 3.9.0 `RequestHandle::await_response` arms `tokio::time::timeout`
  only when polled, and on expiry sends `notifications/cancelled`. Checked in the
  registry source (`service.rs:554–571`). So the test can keep the handle unpolled
  through the held 250 ms preflight and still exercise the real automatic 200 ms
  deadline after backend entry (`local_execution_tests.rs:694–702`).
- The automatic test now asserts `ServiceError::Timeout` exactly (it previously
  asserted only `is_err`). The explicit test drives the real
  `RequestHandle::cancel` (`:704–710`). Both require durable `failed/cancelled`
  before any client, executor or server shutdown (`:715–725`). A shutdown-caused
  stop would record `shutdown`, so this assertion tells the two apart.
- `Notify::notify_one` stores a permit. Entry signals that fire before the test
  awaits them are not lost.
- The fake has no independent run expiry. It returns only on cancel, shutdown or
  sender disconnect (`:605–615`). The 3 s preflight fallback only bounds a hung
  test. Each stage is capped at 20 s, the same as the old `until` budget.
- Cleanup when an assertion panics: unwinding drops the async test future, and
  with it the `release` sender, before the runtime is dropped. Preflight and run
  then return on `Disconnected`, so the runtime does not wait forever on the
  `spawn_blocking` worker.
- The removed end-to-end path (cancellation during preflight, zero backend calls)
  is still covered in the unit tests by
  `executor/tests.rs::cancellation_after_gated_preflight_never_enters_backend`.

## Commands run (Windows, this worktree, pinned toolchain)

| Command | Run ID | Result |
| --- | --- | --- |
| `cargo nextest run --locked -p arxd -E 'test(dedicated_fake_sdk_cancellation)'` | `03000931-165e-4691-a7a7-6452f8283640` | 2/2 pass (0.338 s, 0.542 s) |
| same, `--stress-count 5` | not captured | 5/5 iterations, 2/2 each |
| M1: `CancelOnDrop::drop` body → `let _ = &self.0;`, `--no-fail-fast` | not captured | 0/2. Both fail with `cancellation stage timed out: durable audit completion without shutdown` (20.3 s, 20.5 s). Automatic test got past its Timeout assertion. Total 42 s; no leftover arxd processes |
| M2 (test-side): poll `await_response` eagerly before preflight entry; explicit test ignored | `1ece5cb7-a191-4cc2-b491-e4132c908001` | 0/1. Fails with `cancellation stage timed out: backend entry` (20.3 s); process exited |
| `cargo fmt --all -- --check` | — | pass |
| `cargo nextest run --workspace --locked` on HEAD | `98bc2949-b012-4067-969a-5540ca5bf074` | **221/221 pass, 2 skipped**, 9.944 s |

The mutations ran in a throwaway detached worktree in the scratch directory,
which was then removed. This worktree's code is unchanged. Clippy and the
Python/Node suites were not rerun by this reviewer: crate bytes are identical
to the builder's checked commit, and the CI files are identical to main.

## Advisories (non-blocking)

- **A1, PROGRESS placement.** The merge put main's former Current-state rows
  (`PROGRESS.md:35–39`, "Last merged PR #9", "Main push CI after #38", "PR34
  refresh…") under the heading "PR36 cancellation fixture correction — Sol high"
  with no heading of their own. They now read as if they were Sol's claims.
  Suggested fix before publication: add a heading such as `## Historical
  checkpoint from main before fixture integration (PR34 refresh) — 2026-11-12`
  above line 35. A heading-only edit needs no code re-review.
- **A2.** The evidence file's "Last merged PR remains #9" was true at the
  builder's base and is now superseded by Current state (#34). It is acceptable
  as a historical record.
- **A3.** The explicit test still sets `options.timeout = 200ms`. That timer
  never arms because `await_response` is never polled. This is harmless.
- **A4.** Hosted cause: still UNKNOWN. The fix removes explanations (1) and (2)
  as fixture artifacts. If a real stall (3) happens, the failure message now
  names the stage. Hosted Windows CI on the published head is still required.

Native, installed and dedicated M2 remains **unverified on real hardware**.
