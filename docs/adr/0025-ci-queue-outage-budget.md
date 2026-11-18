# 0025: Charge the CI time budget for execution, not idle job queue

Status: **Active on main10aa891f, 2026-11-17, following the owner-authorized pinned PR163 bootstrap. First new exact-main CI and charged-v1 evaluation verified.** Original Opus design on `c612152`; current-main refresh uses Sol6.1.

The user chose to *prepare* a reviewed queue-outage amendment for the PR #162 deadlock. That choice grants no
bootstrap, protection, status, label or merge exception. The ADR 0022 exception is exhausted and cannot be reused.
No label makes this amendment, or its implementation, a `ci-speed` change. KICKOFF requires "15 minutes
wall-clock", so this ADR is a deliberate deviation, already accepted by the owner. It takes effect only after
separate approval of a concrete bootstrap route (see Activation).

## Owner decision and activation status

On 2026-11-16 the owner explicitly answered "Yes, accept the budget-definition change":
exclude verified idle queue gaps only for fully green runs while retaining total wall-time reporting.
This accepts the deliberate deviation from KICKOFF's queue-inclusive wall-clock definition.
The earlier preparation-only decision above is historical. Acceptance does not authorize the
separate one-use protection/bootstrap route, name its executor, merge this PR, or waive red main.
The trusted main implementation remains the old wall policy until authorized activation.

## Historical context (attempt 1, read-only API)

Main push CI 43880904887 (`c612152`) took 1475s wall. Its macOS job queued 1141s from created to started, then
executed for 333s. Windows failed (R5). PR #162 CI 43889323450 (`bf86f4a`) passed on all three OSes in 1311s wall.
There, macOS queued 1158s, and execution took 228s on Ubuntu, 585s on Windows and 151s on macOS.
The gate is `updated_at - run_started_at` of the latest main run. Only a merge can replace that run, and every merge
needs the gate. Disposition `b9d807a` found no in-scope workflow change that bounds hosted queue time.

## Measurement

All inputs come from attempt 1, in integer UTC seconds, with half-open intervals. S is the run's `run_started_at`
and U is its `updated_at`. For each matrix job j: c_j = `created_at`, s_j = `started_at`, e_j = `completed_at`.

```
seconds            = U − S                      (unchanged; still computed by duration())
pending(t)         ⇔ ∃j: c_j ≤ t < s_j
executing(t)       ⇔ ∃k: s_k ≤ t < e_k
Q                  = { t ∈ [S,U) : pending(t) ∧ ¬executing(t) }
queueCreditSeconds = |Q|
chargedSeconds     = seconds − |Q|
longestJobSeconds  = max_j (e_j − s_j)
```

Compute these with a sweep over the sorted points {S, U, c_j, s_j, e_j}, evaluating each segment at its start.
Charged time keeps every second in which any job executes, counted once. It also keeps queue time that overlaps
another job's execution, idle time with no pending job, time before the first `created_at` and after the last
`completed_at`, and runner setup after `started_at`. Per-job queues are never summed, and the longest queue is
never subtracted.
Invariant: Q ∩ executing = ∅, so |∪ execution| ≤ charged ≤ seconds. Sleeps and slow steps are always charged.
The data proves only that a created required job had no started runner while nothing executed. The cause could be
capacity, scheduling or an outage. The credit is therefore called "idle queue credit", not "capacity". GitHub's
capacity annotation is log text and is not used.

## Gate (pure `budget`)

A run is `creditable` when `chargedSeconds !== null`, the run `conclusion === 'success'`, and every job's
`conclusion === 'success'`. Then `gate = creditable ? chargedSeconds : seconds`.
A run with a known failed platform gets no credit and keeps the exact old wall rule. That rule is never weaker than
today's and never stricter for a normal PR. A red main still holds feature merges through the separate
baseline-health rule (ADR 0024 / coordinator). This gate does not enforce that rule.
A PR passes if `gate ≤ 900`. Otherwise, with the label still re-read live, `ci-speed` passes only if all of these
hold: both the baseline and candidate are green and creditable, both charged values are non-null,
`cand.chargedSeconds < latest.chargedSeconds`, and `cand.longestJobSeconds < latest.longestJobSeconds`.
The last condition is needed because charged time keeps queue that overlaps execution. Without it, a Windows job
starting 200s earlier while Ubuntu runs would count as a reduction made of queue alone. The old wall-reduction path
is removed, because it accepted queue variance. Candidate selection is unchanged: same head SHA, same
`head_repository.id`, highest `run_number`, attempt 1.

## Later topology change

[ADR0031](0031-parallel-ci-validation.md) proposes parallel workers with the
existing required checks retained as completion gates. This ADR's three-job
credit validator is unchanged: the split layout receives no queue credit and
uses the original 900-second wall rule. The single-job workflow pin described
below is historical once ADR0031 is activated; it is not widened to grant credit
to dependency gates or additional jobs.

## Trusted inputs and validation

The evaluator keeps its existing read, `GET repos/{repo}/actions/runs/{id}/attempts/1`. It adds one read:
`GET repos/{repo}/actions/runs/{id}/attempts/1/jobs?per_page=100`. Both go through the existing `api()` with its
8 MiB bound. Exceeding the bound, or a transport error, throws as today. The evaluator reads one page and never
follows pagination. The fields were confirmed today against live responses for both runs, and `actions: read`
already covers the new read. Logs, annotations, step names and free text are never read.

An optional-credit metadata defect sets `chargedSeconds` to null and falls back to wall time. Existing
`duration(run)` completion and time checks run first and still throw; fallback never accepts a partial baseline.
The amendment adds no metadata-validation throw path and never grants queue credit when its data is in doubt.

**`run-invalid`** is set when any of these holds: `id` is not the listed id; `run_attempt ≠ 1`;
`path ≠ .github/workflows/ci.yml`; `status ≠ completed`; the event is wrong (main must be `push` with `head_branch`
`main`, a candidate must be `pull_request`); `head_sha` is not the listed 40-hex SHA; a candidate's
`head_repository.id` is wrong; S or U is not a strict timestamp; or U − S ≠ `seconds` or lies outside 0..604800.

**`jobs-invalid`** is set when any of these holds: `total_count` or `jobs.length` is not 3; an id is not a safe
integer ≥ 1, or ids repeat; `run_id`, `run_attempt` or `head_sha` does not match the run (its id, 1, its SHA); the
names are not exactly `verify (ubuntu-latest)`, `verify (windows-latest)` and `verify (macos-latest)`, once each;
`labels` is not deep-equal to `[os in name]`; `status ≠ completed`; `conclusion` is empty; c, s or e is null or not
a strict timestamp; or S ≤ c_j ≤ s_j ≤ e_j ≤ U does not hold.
A strict timestamp matches `^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$` and round-trips through `toISOString()`, so
dates like Feb 30 are rejected. Labels are compared only for equality and never used to skip a job. A skipped job,
a cancelled job that never started, and partial data all give `jobs-invalid`. A complete run or job that did not
succeed gives `not-green`: its charged time is computed but not credited.
`ci-workflow.test.mjs` pins trusted main to one job, `verify`, with matrix `os` exactly the three labels. It also
pins: no `needs`, job-level `if`, `environment` or `services`, and one top-level `concurrency`. With these pins,
created→started on main is runner assignment only. A PR-controlled workflow cannot hide execution, because of the
invariant. The longest-job check rejects a queue-only win with unchanged execution times. Runtime noise can
still change that value; a `ci-speed` PR must name and independently review the actual execution-time change.

## Status description (≤140 chars, `Main run {id}:` invalidation marker unchanged)

Format: `Main run {id}: wall {W}s, charged {C}; {verdict}`.
- `{C}` is `{n}s`, `{n}s uncredited (not-green)`, or `n/a (run-invalid|jobs-invalid)`.
- `{verdict}` is `budget satisfied`, `ci-speed reduction {c}s/{x}s` (candidate charged / longest job), or
  `over 900s; ci-speed needs charged+exec reduction`.

The longest form, with an 11-digit id and 7-digit values, is 126 characters. Tests assert ≤140 without truncation.

## History migration (append-only)

The 30 existing `docs/ci-duration-history.json` records (`{runId, sha, seconds, conclusion}`) stay wall-only and
byte-identical. They are never rewritten, backfilled or re-recorded. `--record` appends only for new run ids:

`{runId, sha, seconds, conclusion, metric:"charged-v1", chargedSeconds, queueCreditSeconds, longestJobSeconds,
credit:"applied"|"withheld:not-green"|"unavailable:<code>", jobs:[{id,name,conclusion,created,started,completed}]}`
Jobs are sorted by name and keep their validated ISO timestamps. A defect gives `null` charged fields and
`jobs:[]`. The implementation PR records nothing itself.

## Historical record examples (exact fixtures; offsets from S)

| Run | Credited segments | seconds | Q | charged | longest | credit |
| --- | --- | --- | --- | --- | --- | --- |
| main 43880904887 | [1,38) all pending; [442,1142) macOS pending | 1475 | 737 | 738 | 372 Windows (failed) | withheld:not-green |
| PR 43889323450 | [1,80); [804,1159) | 1311 | 434 | 877 | 585 Windows | applied |

Job ids: main Windows 133346204427, macOS 133346205159, Ubuntu 133346205218. PR Windows 133375899952, Ubuntu
133375900319, macOS 133375900358.
The main gate is 1475 (wall), so normal PRs fail. PR #162 as a `ci-speed` candidate is denied: 877 < 738 and
585 < 372 are both false. Counterfactual: if main's Windows job had been green, the gate would be 738 and would
pass, purely on idle queue. The PR's Windows wait from 1 to 219 overlapped Ubuntu's execution from 80 to 308, so
it stays charged.

## Required tests (synthetic, no network)

1. **Geometry.** Serial jobs with gaps. Staggered: A queued 0–500 while B executes 100–400 gives Q=200, not 500.
   Three concurrent 400s waits give Q=400, not 1200. Interleaved A·gap·B·gap·C credits both gaps. Prefix, tail,
   and idle time with nothing pending stay charged.
2. **Slow execution.** 1000s of execution plus 600s of queue gives charged >900, so a normal PR fails.
3. **Defects.** Each `run-invalid` and `jobs-invalid` clause gives null charged, its code, the wall gate and a
   denied `ci-speed`. Include `total_count` 2, 4 and 101, and assert that no second page is read. Existing
   unfinished-attempt rejection precedes credit validation at the entrypoint, even below900s and in `--record`.
4. **Failed jobs.** A complete failed job gives `uncredited (not-green)`. A failed 800s run passes as today, and a
   failed 1475s run fails.
5. **`ci-speed`.** A queue-only reduction is denied. A reduction on both metrics passes. An uncreditable candidate
   or baseline is denied. Keep the existing removed-label, head/repo-mismatch and attempt-2 tests.
6. **Actual runs.** Both reproduce the table exactly.
7. **Status and bounds.** Description length, marker and dedupe. Oversized jobs JSON throws.
8. **History.** Old records stay byte-identical, appends work, duplicate run ids are not rewritten, and the
   workflow pins hold.

## Implementation allowlist and invariants

The implementation may change only these files: `scripts/{policy,evaluate-budget,ci-budget}.mjs` and their
`*.test.mjs`; `scripts/ci-workflow.test.mjs` (pins only); `scripts/fixtures/ci-budget-buffer.mjs`; a new
`scripts/fixtures/ci-queue-credit-runs.mjs`; `docs/ci-administration.md`; `docs/PROGRESS.md`; and this ADR's
Status line. It must not change required contexts, app pins, protection, Actions permissions, `ci.yml`, `policy.yml`, runner
labels or architecture (arm64 `macos-latest`), retries, timeouts, quarantines, the 900s threshold,
test/smoke/desktop counts, or history contents.

## Activation plan (prepare does not activate)

1. **Prepare and review.** Owner definition acceptance is settled. Existing PR163
   source7aefa2ca is refreshed onto exact main3022d601 without policy byte changes.
   Root and an independent Sol6.1 reviewer must review the source and exact final head
   under current non-UI routing, then require three successful platform jobs and
   exact-head independent agent-review. Historical Opus reviews remain valid evidence.
2. **Current old gate.** Main44126173935 attempt1 failed in905s wall after a zero-step
   macOS infrastructure cancellation. This replaces the historical1475s/R5 baseline.
   The single final read verified macOS-only attempt2 completed/success on exact
   main3022d601, restoring functional health but giving attempt1 no credit. Trusted main still runs the old wall policy; no ci-speed label applies.
3. **Separate owner decision.** Only root presents the concrete reviewed full head,
   base/main pins and four green non-budget gates for a separately authorized named
   root executor. The PR164 one-use exception and ADR0022/#57 are consumed, not reusable.
   Definition acceptance alone grants no bootstrap, protection, status or merge action.
4. **Bounded route, still unauthorized.** Follow the detailed plan in
   [ci-administration](../ci-administration.md#proposed-adr0025-bootstrap-not-authorized):
   zero armed auto-merges before the window and immediately before merge; exact
   head/base/main and settings pins; remove ONLY ci-time-budget temporarily;
   preserve admin enforcement and restore original strict/all five app bindings in
   finally with exact readback. Abort on drift or restoration failure. No manual
   success status, rerun, admin bypass or label workaround.
5. **After eventual activation.** Verify merge SHA/tree and first trusted evaluation.
   Failed905s attempt1 gets no credit, regardless of attempt2 functional success.
   A new first-attempt fully green exact-main push with a qualifying charged value
   is still needed to restore the normal budget flow. No feature merge is granted.
   This preparation never runs live ci-budget.mjs or --record; historical rows stay
   untouched. Rollback is a reviewed revert through the same gates.

## Rejected

Summing per-job queues was rejected: it double counts and can exceed wall time. Subtracting the longest queue was
rejected: it credits queue that overlaps execution. Also rejected: excluding macOS by label, parsing annotations,
making macOS non-blocking, raising the 900s threshold, and crediting failed runs, whose execution is truncated.

M2 stays open and M3 unchecked. The Windows quarantines are unchanged; the old R5 failure is historical. Native paths remain **unverified on
real hardware**.

## Activation checkpoint - 2026-11-17

Owner-authorized root Sol coordinator merged reviewed head22a09eae as10aa891f.
The budget-only exception is consumed. Original five required checks and all
protection fences were restored and verified. Earlier preparation/approval text
is historical; no further exception is granted. CI44177880021 attempt1 passed:632s wall,629s charged,3s queue credit.
Automatic trusted policy44179754154 succeeded.
