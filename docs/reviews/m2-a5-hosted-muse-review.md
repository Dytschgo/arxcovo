# Muse independent acceptance review: hosted A5 run (D7d consumed)

2026-11-14; Muse Spark (muse-spark-1.3, high); independent Standards/Spec/security
review sub-agent. Read-only except this file: `git` CLI reads, `gh run view`
and one `gh api` variable-absence read (fixed fields only, no log reads, no raw
payload). No native/product CLI, no system/service/account/owner/process/probe,
no build/tests/dispatch/subagents. No PROGRESS/source edit, no push/PR.

## Exact pins

- Main (PR97 merge): `cf3a2510fc45e0cb438ae14cc41115e492bc0f41`
  (`test(exec): select one bounded A5 empty-orphan run (#97)`).
- Prepared head: `21a9074fdd514690d183f1956c17f8f23f4cd53b`
  (`docs(review): retain bounded hosted A5 execution record`); HEAD == prepared
  head at review time.
- Range `cf3a251...21a9074 --stat` is exactly two docs files, +17/−0:
  `docs/PROGRESS.md` (+2, dispatch-tracking line) and new
  `docs/reviews/m2-a5-hosted-record.md` (+15). `git diff --check` clean.
- Authorized dispatch: run `43644661407`, attempt `1`, job `132599634797`,
  `workflow_dispatch` on `refs/heads/main`, exact head above.
- Spec sources read: `docs/adr/0019-unix-native-execution-bridge.md` (D7d,
  lines 451–537), `docs/reviews/m2-stagef-a4a5-janitor-plan.md` (S1–S3),
  `docs/reviews/m2-stagef-a5-seeder-plan-muse-recheck.md` chain via D7d review,
  `docs/reviews/m2-stagef-a5-source-muse-review.md`,
  `docs/reviews/m2-stagef-a5-driver-muse-review.md`,
  `docs/reviews/m2-d7d-a5-selection-muse-review.md`,
  `docs/reviews/m2-a5-evidence-pin-refresh.md` plus its Muse review, and
  `docs/reviews/m2-a5-hosted-record.md` (the record under review).
- Source fixture pins (observed via bounded `git show` on `cf3a251`, no checkout
  execution): `tools/unix-exec-validation/driver.py` (`TEST =
  'exec::disposable::disposable_linux_a5'`; `standard_profile()` closed
  four-literal allowlist, `TEST != a3_none` rule unchanged),
  `tools/unix-exec-validation/test_driver.py`
  (`test_source_selection_is_a5` literal pin), `crates/arxd/src/exec/disposable.rs`
  (`#[ignore]` `disposable_linux_a5`, `READY=1` + `hold_identity_ack`),
  `crates/arxd/src/exec/hierarchy/a5.rs` (`preflight`/`finish`/`empty`, one
  `setup()` call, `leaves_removed != 1` / `runs_created` guards).

## Run verification (fixed fields only, independently checked)

- `gh run view 43644661407`: `headSha == cf3a251...`, `event ==
  workflow_dispatch`, `headBranch == main`, `status == completed`,
  `conclusion == success`, `workflowName == "Unix execution experiment (manual,
  disposable)"`.
- `gh api repos/Dytschgo/arxcovo/actions/runs/43644661407`: `head_sha ==
  cf3a251...`, `event == workflow_dispatch`, `actor == Dytschgo`,
  `triggering_actor == Dytschgo`, `run_attempt == 1`.
- Job `132599634797`: `status == completed`, `conclusion == success`,
  `startedAt == 2026-11-14T21:47:30Z`, `completedAt == 21:48:58Z` (88 s), all
  eight steps `success` (setup, checkout, toolchain, safe guard tests,
  fetch-without-privilege, guard/stage/run/finally-clean, post-checkout,
  complete). Matches the record's time/step claims exactly.
- Cleanup: `gh api .../actions/variables/UNIX_EXECUTION_REVIEWED_SHA` returns
  HTTP 404 independently observed this session. The accepted dispatch is
  consumed; D7d grants no further authority.
- Retained feedback is only the fixed-schema JSON in the record
  (`schema == unix-protection-f0`, `version == 2`, provenance `source_sha ==
  workflow_sha == cf3a251...`, `run == 43644661407`, `attempt == 1`,
  `first == null`, `final == null`, `mutation_lower_bound == cgroup`,
  `cleanup == confirmed`, `internal.service/fixture-check/harness == absent`,
  `carried/refusal == null`), with a stated 2 MiB log-read ceiling. No raw
  output payload or host identity is retained in the tree.

## Standards

PASS, no findings. The prepared delta is docs-only (one 2-line PROGRESS status
line plus the 15-line hosted record), matching the repo's one-concern
conventional-commit shape. No source, workflow, Cargo, package, guard, pin, or
hash-byte change; no new names, types, branches, or abstractions, so the smell
baseline has nothing to attach to (no Duplicated Code, Speculative Generality,
Divergent Change, Shotgun Surgery, Mysterious Name, or Message Chains).
`git diff --check` clean. No secret, owner datum, approval flow, or publish
change.

## Spec

PASS. The record satisfies the D7d acceptance join, not merely a green workflow:

- Candidate identity: the run's exact head equals the D7d sole-candidate SHA
  (`cf3a251...`), whose TEST delta to the verified `c7f44ec...` baseline is the
  one reviewed runtime TEST line to A5 plus the reviewed literal selection
  assertion and historical-None inverse mutant.
- Selection/profile: exact `TEST == exec::disposable::disposable_linux_a5`
  pinned literally in source and test; unchanged closed allowlist derives
  Standard through the existing rule, so the host required the unchanged
  packaged Standard drop-in and a None-profile host stayed ineligible.
- Fixture discipline: guarded test-only empty-orphan seeder with read-only
  preflight before either mkdir, exclusive `arx-0-0` creation, held descriptors
  through exactly one production `setup()` call, one-leaf removal / runs-reused
  / controller / S9 postconditions before READY and identity hold, exact
  one-pass/zero-failure summary, real systemd success/zero exit, and owned-only
  cleanup — the reviewed A5 source/mapping content joined to this run's
  `final == null`, `cleanup == confirmed` fixed-schema result.
- Bounds honored: one accepted dispatch, consumed regardless of outcome, with
  actor/ref/SHA equality, finally-style variable clear, and HTTP 404 absence
  check; no selector input, retry, or second run. A green workflow or internal
  `absent` statuses alone are correctly disclaimed by the record as
  insufficient; credit here rests on the joined selection/fixture/result above.
- Credit fenced: only this disposable-hosted empty-orphan path. A4
  crash-survivor behavior is unproven by this run. M2, A4, A6–A12, full Stage F,
  Stage G, and real-Runner PA6 remain open. Installed/dedicated paths are
  **unverified on real hardware**. No historical failure is reclassified and no
  further native execution is authorized.

## Security

No blocker. Bounded `git`/`gh` reads only, with fixed JSON fields; no raw logs,
no host identity, no secret or owner data in the delta. The fixed-schema
envelope preserves the allowlist/attribution/PID/collision/reservation/cleanup
fences by joining to the unchanged reviewed source rather than weakening any
guard. D7d is consumed; the 404-verified variable clear leaves no standing run
permission. Reversibility is a two-way door: this docs-only record reverts
cleanly with no native residue.

## Verdict

- **Standards:** 0 findings.
- **Spec:** 0 findings; A5 credit ACCEPTED only for this disposable-hosted
  empty-orphan path on exact `cf3a251...` via run `43644661407` attempt 1.
- **Security:** 0 blockers.
- Worst issue per axis: none.
- Publication: docs checkpoint publication only (this file). No source edit,
  push, PR, retry, or further dispatch.
