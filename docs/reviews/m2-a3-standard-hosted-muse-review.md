# Muse independent acceptance review: A3 Standard hosted run

2026-11-14; Muse Spark (muse-spark-1.3, high); independent
Standards/Spec/security acceptance-evidence review sub-agent. No
delegation, no native query, no product CLI, no Win32/process/service/
account/owner operation, no native filesystem probe, no build/tests/
dispatch, no subagents. Git/GitHub CLI read-only only
(`git log/rev-parse/show/diff`, `gh run view`, `gh api` run/jobs read,
one variable-absence read returning HTTP 404). No raw log payload
retained. Commit of this review file only is explicitly tasked; no
PROGRESS/source edit, no push/PR.

## Pins (exact)

- Under review: `5d504c112d52f4e4fcefce339ee41795ba2374e8`
  (`5d504c1 docs(review): retain bounded A3 Standard execution record`),
  atop `9a22c4f` (PROGRESS dispatch track) atop main
  `beb7117b08aad68da790e05a85b335f9655f5236`
  (`beb7117 test(exec): select bounded A3 Standard acceptance run (#91)`).
  `git diff beb7117...5d504c1 --stat` is exactly two files:
  `docs/PROGRESS.md` (+2), `docs/reviews/m2-a3-standard-hosted-record.md`
  (new, +15).
- Source baseline: PR #90 main `fba8b3c993b3d173125525c760ee2dc7234e3cfa`.
- Accepted dispatch: run `43635131545`, attempt 1, `workflow_dispatch`
  on `main`, head SHA `beb7117b08aad68da790e05a85b335f9655f5236`
  (equals candidate; `GITHUB_SHA == ARX_REVIEWED_SHA ==
  ARX_WORKFLOW_SHA` preserved), actor `Dytschgo`, triggering actor
  `Dytschgo` (both verified via run API read this session).
- Job `132570170629` ("experiment"): success, `2026-11-14T20:00:31Z`
  to `20:02:03Z` (92 s). All eight steps success, including "Safe
  guard tests", "Fetch locked dependencies without privilege", and
  "Guard, stage, run and finally clean (fresh hosted Linux only)".
  One accepted dispatch consumed Standard's grant. No retry.
- Fixed-schema feedback only (2 MiB log-read ceiling; no raw output
  payload or host identity retained):
  `schema unix-protection-f0 v2`, provenance source/workflow
  `beb7117...`, run `43635131545` attempt `1`, `first: null`,
  `final: null`, `mutation_lower_bound: cgroup`, `cleanup: confirmed`,
  `internal: {service: absent, fixture-check: absent, harness: absent,
  carried: null, refusal: null}`.
- Grant consumption: `gh api
  .../actions/variables/UNIX_EXECUTION_REVIEWED_SHA` returns HTTP 404
  here (verified this session), confirming the variable is absent.
- Candidate-scope check (verified this session):
  `git diff fba8b3c...beb7117` touches no `packaging/` or `crates/`
  byte; the only runtime change is the one TEST line in
  `tools/unix-exec-validation/driver.py`
  (`disposable_linux_a1_a2` to `disposable_linux_a3_standard`), plus
  the reviewed selection-regression corrections (`test_driver.py`,
  `mutation_check.py`), the D7c ADR text, and supporting
  review/progress documents.
- Spec sources read: `docs/adr/0019-unix-native-execution-bridge.md`
  (D7c), accepted plan
  `docs/reviews/m2-stagef-a3-driver-plan-muse-review.md`, source
  reviews `docs/reviews/m2-stagef-a3-source-muse-review.md` and
  `docs/reviews/m2-stagef-a3-driver-source-muse-review.md`,
  `docs/reviews/m2-d7c-a3-selection-muse-review.md` (D7c predicate),
  `docs/reviews/m2-a1a2-hosted-evidence-muse-review.md` (prior-axis
  shape), `docs/reviews/m2-a3-standard-hosted-record.md` (record
  under review), `docs/KICKOFF.md` (v1 scope).

## Standards

Pass. The prepared commit is docs/evidence only, one concern, with a
conventional message. It adds no code, workflow, permission, secret,
owner operation, product approval flow, or optional infrastructure.
The record defers acceptance to independent review, disclaims
M2/full F/G/PA6, and marks installed/dedicated paths **unverified on
real hardware** per AGENTS.md. No baseline smell applies to this
prose/JSON delta beyond noting the PROGRESS line and the record
restate the same run facts, which is expected for a checkpoint. No
findings.

## Spec

Pass with bounded scope. The run binding matches the D7c Standard
grant, and the exact selected TEST joins to the full acceptance
chain already covered by the cited plan/source reviews:

- `tools/unix-exec-validation/driver.py` at `beb7117`: `TEST` is
  exactly `exec::disposable::disposable_linux_a3_standard` with
  `--ignored --exact --test-threads=1`; `gate()` requires Linux,
  `Dytschgo/arxcovo`, both actors `Dytschgo`, `refs/heads/main`, and
  `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`.
- Proc-status parser (`crates/arxd/src/exec/hierarchy/parse.rs` at
  `beb7117`, unchanged from baseline): `hardening()` requires
  `NoNewPrivs:\t1` exactly and `CapBnd:\t00000000000000c1` exactly
  for `Hardening::Standard`; anything else is not Standard.
- `disposable_linux_a3_standard` (ignored) asserts
  `Hardening::Standard` via the production procfs reader, checks the
  absent ack, sends `READY=1`, then holds on the private
  `identity-ack`; `hold_identity_ack()` is the verbatim A1/A2 loop.
- Unchanged packaged profile: no `packaging/` or `crates/` diff
  versus `fba8b3c`, so Standard's whole-file profile presence is the
  reviewed bytes, never a weakening drop-in; None's file-omission
  shape is untouched here.
- Exact runtime attribution, READY hold, sole-PID outer checks
  (MainPID = recorded PID, `/proc/PID/exe` = protected artifact),
  exact ignored-libtest argv, one-pass/zero-failure summary, real
  systemd success/zero exit, bounded output/lifetime, and owned-only
  cleanup are the driver/`validate_record` joins accepted in the
  cited plan/source reviews; the observed run reports their joint
  success (`first: null`, `final: null`, `cleanup: confirmed`) with
  all job steps green and the reviewed-SHA variable cleared.

## Security

Pass. No new privileged path, secret, or trust claim. Fixed absolute
runtime paths and existing fences (exact package/runtime attribution,
sole MainPID/supervisor checks, protected staging, shared
reservation, collisions, bounded output/lifetime, owned-only cleanup)
retained verbatim; profile presence derives from the selected TEST;
no NNP/capability override; closed codes unchanged; technical fences
only, no approval flow. The 404 variable read confirms no standing
dispatch authority remains.

## Findings and verdict

- Blockers: none.
- Credit: Standard disposable path only, on this exact run. M2, full
  Stage F, Stage G, real-Runner PA6, installed/dedicated acceptance,
  production constructors and activation receive no credit and remain
  open/inactive; installed/dedicated paths stay **unverified on real
  hardware**. Historical `43091339273` and all consumed D7a/D7b
  environments stay excluded with no cause claim; no historical
  failure is reclassified.
- None eligibility under D7c: this review's acceptance of Standard
  (successful result, confirmed owned cleanup, verified variable
  absence) satisfies Standard's side of the None precondition.
  Dispatching None still requires its own new source change of only
  that TEST line to `..._a3_none` plus supporting review/progress
  documents, a new independent review, all five required gates,
  coordinator merge, and the pinned exact new main SHA. This review
  grants no None execution. This documentation checkpoint may ride as
  review docs inside that None candidate per D7c; no separate main
  merge of this review is demanded (which would invalidate that
  candidate).
- This review grants no additional execution before its gates and
  does not merge.

Standards 0. Spec 0. Security 0. Worst issue: none.
