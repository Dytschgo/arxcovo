# ADR 0025 idle queue credit: independent Opus review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-16. Review sub-agent, single-agent
review on separate Standards, Spec and Security axes. No subagents or children.

Independence: this counts as cross-provider review of Sol's implementation only. An Opus
design sub-agent wrote ADR 0025, so this review claims no independence for that contract.

**Verdict: CHANGES REQUIRED, one blocking finding (S1).** Standards: PASS with advisories.
Spec: FAIL on S1. Security: S1 is a fail-closed regression of the trusted baseline.
This verdict grants nothing beyond preparation. See Activation limits.

## Pins

- Worktree branch `t3/review-ci-queue-policy`, HEAD exactly
  `e7c75ec6ec0d521de4d57e0bd31397a2583d14c3`, clean. Base
  `d6dc883f68c1605e1a52c83ac4201159dcad4a84` (on Rust main `c612152`).
- `git log d6dc883..e7c75ec`: `51ccbb8` feat (implementation), `874dc9e` fix (completion
  correction 2), `e7c75ec` docs (evidence). Source `874dc9e` and `e7c75ec` differ only in
  four docs files.
- `git diff d6dc883...e7c75ec`: 13 files, +600/−31. Source: `policy.mjs` +51,
  `ci-budget.mjs` +20, `evaluate-budget.mjs` +9, plus tests and fixtures. All inside the ADR
  allowlist.
- Blobs recomputed and equal to the evidence: policy `4b52a44d`, evaluate-budget
  `6d312a80`, ci-budget `b315b25d`, credit fixture `70d60463` (SHA256 `9ef25a64…622e`),
  history `23f63dfb`. The diff does not touch `.github/**`, the history file, `package*.json`,
  `policy.yml` or protection. No dependencies or workflows were added.

## Checks actually run by this reviewer

- `node --test scripts/*.test.mjs` on exact `e7c75ec`, run once (Node v24.18.0):
  **45/45 pass**, 8.2s. This includes the 5 unchanged auto-merge tests. ci-budget tests
  use the synthetic VM harness. No `gh`, live entrypoint or network was used.
- Synthetic VM variant (scratch copy of `scripts/fixtures/ci-budget-buffer.mjs`, repo
  sources loaded read-only). It adds one scenario: an attempt 1 main run with
  `status:'in_progress'` and 800s elapsed. Results:
  - Final source: posts **success** `Main run 20: wall 800s, charged n/a (run-invalid); budget satisfied`.
  - Base `d6dc883` sources: posts failure `Main CI measurement unavailable; retry required`
    and throws `Invalid completed workflow run`.
  - Final source with only `duration({ ...run, status: 'completed' })` → `duration(run)`:
    same failure and throw as base. `fast-main` and `large-candidate` are unchanged.
- Runtime mutants on a scratch archive of `e7c75ec` (full suite each; not the repo tree).
  Each line gives the mutant, then pass/fail:
  - Drop the not-executing check: 43/2. Drop the longest-job condition: 44/1.
  - Drop the jobs-green credit condition: 44/1. Drop the label check: 44/1.
  - Drop the S/U bounds: 44/1. Drop the head_repository check: 44/1.
  - Change the append formatting: 44/1. Revert the status forcing: 44/1, because the
    `incomplete-main` test encodes the weakening.
  - **Drop `credit === 'applied'` from `creditable`: 45/0, survives** (A2).
- Reused, not rerun as such: the builder's 44/44 full run (before the corrections), its
  40/40 focused run and its five Python mutants. Fixture provenance was accepted from the
  cached fixture and evidence. No public API read was needed.

## Spec / Security

**S1 (blocking): forced completion fails open on an incomplete trusted baseline.**
`scripts/ci-budget.mjs:15` calls `duration({ ...run, status: 'completed' })`. The old
`duration(run)` (`policy.mjs:55-58`) throws when the attempt 1 status is not `completed`.
For main, that throw makes `evaluatePullRequests` post `Main CI measurement unavailable`
on every PR and fail. Correction 2 now turns that case into `run-invalid` and falls back
to `U − S` of a run that has not finished. That value is only a lower bound on wall time,
so a partial main elapsed time of 900s or less passes every normal PR. The VM repro above
shows success where base fails. `--record` would also append that partial wall time.
Spec conflicts:
- ADR: the fallback is "the exact old wall rule … never weaker than today's". Today's rule
  refuses non-completed attempts.
- ADR: `seconds = U − S (unchanged; still computed by duration())`. Forcing the status
  changes `duration()`'s contract.
- The task requires preserving trusted first-attempt/completed baseline authority.

"Adds no throw path" forbids new throws. It does not license removing an existing
fail-closed throw. Reachability is narrow: the listing is filtered `status=completed`,
so this needs inconsistent API data or a race. The fix costs one token, though, and the
regression is real. For a candidate, the change only alters the failure mode (it cannot be
creditable, so ci-speed is denied), but the same revert applies.
**Fix:** use `duration(run)` again. Change the `incomplete-main` VM test to expect the base
behavior: final post `Main CI measurement unavailable; retry required` and error
`Invalid completed workflow run`. Optionally add an 800s incomplete-main case. Inside the
pure function, `run-invalid` for `status ≠ completed` stays a defensive clause. It is
unreachable through the entrypoint, and the ADR allows that.

Verified as matching the ADR (no finding):
- **Sweep** (`policy.mjs:37-41`): deduplicated sorted points, half-open segments evaluated
  at their start, Q = pending ∧ ¬executing. Overlapping queue is never credited, per-job
  queues are never summed, and prefix, tail and idle time stay charged. Both actual
  fixtures reproduce 1475/737/738/372 withheld and 1311/434/877/585 applied.
- **Run validation:** id equals the listed id and is a safe integer; attempt 1; path;
  status; event and branch by kind; 40-hex `head_sha` equal to the listed one; a positive
  safe candidate repository id equal to `head_repository.id`; strict round-tripping
  timestamps; `U − S === wall.seconds` within 0..604800.
- **Job validation:** `total_count === 3`, `length === 3`, unique safe ids, the exact
  three-name inventory consumed with `Set.delete`, run_id/attempt/sha bound to the run,
  `labels` exactly `[os]`, completed, a non-empty conclusion other than `skipped`, and
  `S ≤ c ≤ s ≤ e ≤ U`.
- **Bounds:** one jobs page, `per_page=100`, never paginated, through the existing 8 MiB
  `api()`. Overflow and transport errors throw, and the VM proves it.
- **Credit:** green only. `withheld:not-green` computes metrics without crediting them.
  Defects null the metrics and keep the wall gate, so no credit can be manufactured from
  malformed data.
- **ci-speed:** both sides creditable, plus strict charged and strict longest-job
  reductions. The wall-reduction path is removed.
- **Status:** the `Main run {id}:` marker is kept, the worst case stays at 140 characters or
  fewer without truncation, and dedupe is unchanged.
- **History append:** keeps the bytes of the 30 old records and does not rewrite a
  duplicate id. No new broad catch exists, and no raw error text reaches a status.
- **Workflow pins:** structural assertions only. The default-main shape matches the ADR.

## Standards (advisory, judgement calls)

- **A1, liveness/fail-closed:** `evaluate-budget.mjs` still fetches the candidate when
  `latest.seconds > 900` (the wall value), not when the gate exceeds 900. A ci-speed PR then
  makes an extra jobs read even though a creditable charged value of 900 or less would
  already pass. A transport error on that read then fails the PR needlessly. This is
  fail-closed only. Consider gating on the same expression `budget` uses.
- **A2, test gap:** no test separates `credit === 'applied'` from `run.conclusion ===
  'success'`. The code is correct, because it requires every job to be green, but no
  test proves it. The case is malformed data, or possibly a PR workflow with a job-level
  `continue-on-error` (unverified). The mutant survives.
  Add a pure case: run success, one job failure, wall above 900 and charged at or below
  900, so `budget` must throw.
- **A3, cosmetic:** the appended record is serialized at indent 0 inside a 2-space array
  (`},\n{\n  "runId"`). It is valid JSON and keeps old bytes. Indenting it would keep the
  file uniform.
- **A4, smell, possible Data Clumps:** `queueMeasurement(wall, run, payload, listed,
  kind, repositoryId)` takes six positional arguments. Acceptable at this size.
- **A5, trust note, not a defect:** a candidate run executes the PR's own `ci.yml`, so the
  structural pins constrain only trusted main. The ADR invariant (Q ∩ executing = ∅),
  the exact three-job inventory and the strict longest-job condition bound the abuse. A
  ci-speed PR must still name and independently review its execution change.

## Activation limits

- This is user-authorized **preparation only**. It does not accept the KICKOFF wall-clock
  deviation and grants no bootstrap, protection, status, label or merge exception.
- ADR 0025 stays Proposed and inactive. The trusted main old wall policy stays active, and
  no `ci-speed` label may be used for this policy repair. The ADR 0022 historical exception
  is exhausted.
- After activation, main 43880904887 is RED and uncreditable, so normal PRs still fail
  at 1475s wall. PR #162 cannot claim ci-speed (877 ≥ 738 and 585 ≥ 372). The amendment
  does not credit failed main, unblock #162 automatically or fix R5.
- Platform CI for this source is unrun, and no `agent-review` status was posted. M2 is open
  and M3 unchecked. Native paths remain **unverified on real hardware**.

Findings: Spec/Security 1 blocking (S1, forced completion fails open on an incomplete main
baseline); Standards 0 blocking, 5 advisory (worst: A2, the surviving `credit` mutant).
