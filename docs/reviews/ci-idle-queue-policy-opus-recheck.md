# ADR 0025 idle queue credit: bounded Opus recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-16. Review sub-agent, one bounded
recheck of the S1/A2 correction round. No subagents or children.
Independence: cross-provider review of Sol's correction only. It claims none for the ADR
contract, which an Opus design sub-agent wrote.

**Verdict: PASS for preparation. S1 is closed and A2 is closed.** Standards: PASS, with
earlier advisories A1 and A3–A5 still open and nonblocking. Spec: PASS. Security: PASS.
This grants nothing beyond preparation. See Activation limits.

## Pins (actual)

- Worktree branch `t3/ci-queue-policy-opus-recheck`, HEAD exactly
  `f5b3361457f57b64be934b11d277ecb26b3bb43f`, clean. Parent merge `8c5a6fb` brings in the
  full review `95ecbce` (of `e7c75ec`, source `874dc9e`).
- `git diff --stat 95ecbce f5b3361` lists exactly 6 paths, +82/−11: `scripts/ci-budget.mjs`,
  `scripts/ci-budget.test.mjs`, `scripts/fixtures/ci-budget-buffer.mjs`,
  `scripts/policy.test.mjs`, ADR 0025 and `docs/evidence/ci-idle-queue-policy.md`.
- `git diff --quiet 874dc9e f5b3361` passes for `scripts/policy.mjs`,
  `scripts/evaluate-budget.mjs`, the credit fixture, `.github/` and
  `docs/ci-duration-history.json`. Queue geometry, validators and byte bounds are unchanged
  since the full review. The only production change is in `ci-budget.mjs`: a code line and
  its comment.
- SHA-256 at HEAD: policy `6e9911bf…2480`, ci-budget `7e532f28…19a6`, buffer fixture
  `f1d67dee…65e5`, ci-budget test `93ae0b07…d6c0`, policy test `4ef27714…c959`,
  ADR `9453f2c8…d90e`. `git diff --check 874dc9e f5b3361` is clean.

## Checks actually run by this reviewer

- `node --test scripts/*.test.mjs`, run once on exact HEAD (Node v24.18.0): **48/48 pass**,
  8058.70ms, exit 0. That covers `incomplete-main`, `fast-incomplete-main`, the unfinished
  record test and the failed-job credit test. Log SHA-256 `1198307b…e8e3`, kept in the
  scratchpad. ci-budget tests use only the synthetic VM harness and its `gh` stub. Nothing
  ran the live `node scripts/ci-budget.mjs` or `--record`. No GitHub API was used. Afterwards
  `git status` stayed clean and the history file was unchanged.
- I ran no new mutants: I had no concrete concern after reading the code (see the trace below).

## Findings closure

**S1 (blocking) → CLOSED (actual reading plus the actual suite; red-before-fix is reused).**
`measure` now calls `duration(run)` on the raw attempt-1 object. `duration` throws
`Invalid completed workflow run` when status ≠ completed or the seconds are non-finite or
negative. That restores the base `d6dc883` authority on main and candidate alike. Traced
semantics:
- Normal path, 1200s unfinished main: `getMainRun` throws. The evaluator posts
  `Main CI measurement unavailable` as failure and rethrows. The rewritten test now requires
  the original exception. Before, it accepted `charged n/a (run-invalid)` with error null.
- `fast-incomplete-main` (800s): the old forced completion would post success, because
  800 ≤ 900. That is the fail-open case my full review reproduced. Now it throws and posts
  failure, and the loop asserts no success post.
- `record-incomplete-main`: the throw comes before `readFileSync`/`writeFileSync`. The test
  asserts `written === null` and `posts` empty, so no history row and no status. The old
  code would have appended an 800s row.
- `queueMeasurement`'s own `status ≠ completed` → `run-invalid` clause can no longer be
  reached on status. It stays as defense in depth, which is harmless.
- Reused, not rerun: the builder's red log (three runtime assertion failures, no compile
  error), green 48/48 at 8165.94ms, and hashes `24365ccf…bc62` and `aeeae4ee…06d3`. My
  full-review VM variant showed the same one-line revert yields base behaviour.

**A2 (advisory) → CLOSED (actual reading plus the actual suite; the mutant result is reused).**
The new pure test builds a success run in which one job failed. Its spans are three jobs
[0,600,1000] with a 1000s wall, so Q = 600 and charged = 400. The test asserts
`credit === 'withheld:not-green'` and that `budget(result, null, false)` throws. Without
`credit === 'applied'`, `creditable` would be true, because conclusion success and 400 is
finite and ≥ 0. Then 400 ≤ 900 returns true and `assert.throws` fails. So the test kills
exactly the mutant that survived as 45/0 in my full review. The test does not lean on wall
alone: 1000 > 900 makes the outcome depend on the credit marker. Reused, not rerun: the
builder's mutant log `c565c725…6eb6` (it passes `node --check`, then fails at runtime;
restoring the source passes).

**ADR 0025 clarification → accurate.** It now separates two cases. An optional credit
metadata defect falls back to wall time. The existing `duration(run)` completion and time
checks still throw first, and a partial baseline is never accepted. Test 3 also states
that unfinished-attempt rejection comes before credit validation, including below 900s and
under `--record`. This matches the code and the original completed-run authority. The
amendment still adds no metadata-validation throw path.

## New advisories (nonblocking)

- **A6, ordering note:** `measure` reads the attempt-1 jobs before `duration(run)` throws.
  So an unfinished run still costs one bounded jobs read, and a jobs transport error would
  surface first. Both stay fail-closed. "Run first" in the ADR means before credit
  validation, not before the read.
- **A7, cosmetic:** some spaces were dropped in the new ADR and evidence text ("below900s",
  "Opus95ecbce", "the1200s", "suite48/48", "in8165.94ms").
- The earlier A1 and A3–A5 were not in this round's scope. They remain open and advisory.

## Activation limits

- This is user-authorized **preparation only**. It does not accept the KICKOFF wall-clock
  deviation and grants no activation, bootstrap, protection, status, label or merge
  exception. The ADR 0022 exception is exhausted.
- ADR 0025 stays Proposed and inactive, and the old wall policy stays active. The 900s
  budget, all three OS jobs (macOS on Apple Silicon) and the required contexts are kept.
- Main `c612152` is RED at 1475s wall, with 738 charged and uncredited. PR #162 passed
  every platform job and review but fails the budget, and it cannot claim `ci-speed`. This
  round changes none of that.
- Platform CI for `f5b3361` was not run, and I posted no `agent-review` status. M2 is open
  and M3 unchecked. Native paths remain **unverified on real hardware**.

Findings: Spec/Security 0 blocking (S1 closed); Standards 0 blocking, A2 closed, two new
advisories (A6 and A7, cosmetic or ordering).
