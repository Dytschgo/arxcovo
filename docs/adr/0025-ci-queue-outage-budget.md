# 0025: Charge the CI time budget for execution, not idle job queue

Status: **Proposed 2026-11-16 — implemented locally for review, not accepted, not active.** Opus design sub-agent, exact main `c612152`.

The user chose to *prepare* a reviewed queue-outage amendment for the PR #162 deadlock. That choice grants no
bootstrap, protection, status, label or merge exception. The ADR 0022 exception is exhausted and cannot be reused.
No label makes this amendment, or its implementation, a `ci-speed` change. KICKOFF requires "15 minutes
wall-clock", so this ADR is a deliberate deviation. It takes effect only after the user accepts it and separately
approves a concrete bootstrap route (see Activation).

## Context (attempt 1, read-only API)

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

## Trusted inputs and validation

The evaluator keeps its existing read, `GET repos/{repo}/actions/runs/{id}/attempts/1`. It adds one read:
`GET repos/{repo}/actions/runs/{id}/attempts/1/jobs?per_page=100`. Both go through the existing `api()` with its
8 MiB bound. Exceeding the bound, or a transport error, throws as today. The evaluator reads one page and never
follows pagination. The fields were confirmed today against live responses for both runs, and `actions: read`
already covers the new read. Logs, annotations, step names and free text are never read.

A defect sets `chargedSeconds` to null with a reason code, and the gate falls back to wall time. The amendment adds
no throw path, so it cannot create a new deadlock and never grants credit when data is in doubt.

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

## Actual record examples (exact fixtures; offsets from S)

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
   denied `ci-speed`. Include `total_count` 2, 4 and 101, and assert that no second page is read.
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

## Activation plan (prepare ≠ activate)

1. **Prepare** (this commit). The policy is unchanged and has no effect.
2. **Build and review.** Sol reviews this contract and Sol high builds one PR within the allowlist. Opus high
   reviews the exact head, and the `agent-review` comes from the other provider.
3. **Blocked by the old gate.** The old trusted main policy evaluates the PR. With 43880904887 at 1475s,
   `ci-time-budget` correctly fails. The PR carries no `ci-speed` label, so it stays blocked.
4. **Bootstrap.** Once the patch is reviewable, the coordinator asks the user for a concrete bootstrap route.
   That approval also accepts the KICKOFF deviation. No admin step is taken without an approved bounded route,
   and any execution is recorded here and in PROGRESS.
5. **Readback.** After the merge, read back the first trusted evaluation. While 43880904887 is the latest main run
   and not green, normal PRs still fail. Activation authorizes no PR #162 merge and does not fix R5. The next
   green main push with charged ≤900 restores normal flow and is appended with `--record`. Rollback is a
   reviewed revert through the same gates.

## Rejected

Summing per-job queues was rejected: it double counts and can exceed wall time. Subtracting the longest queue was
rejected: it credits queue that overlaps execution. Also rejected: excluding macOS by label, parsing annotations,
making macOS non-blocking, raising the 900s threshold, and crediting failed runs, whose execution is truncated.

M2 stays open and M3 unchecked. The Windows quarantines and R5 are unchanged. Native paths remain **unverified on
real hardware**.
