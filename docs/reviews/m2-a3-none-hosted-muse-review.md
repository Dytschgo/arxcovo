# Muse independent acceptance review: A3 None hosted run

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

- Under review: `c9a992498fbaed4091710ef91e96aec2a7678181`
  (`c9a9924 docs(review): retain bounded A3 None execution record`),
  atop `d76edc7` (PROGRESS dispatch track) atop main
  `4bb15612e30ec5bbf0c019bab347c83fd60a9d0a`
  (`4bb1561 test(exec): select bounded A3 None acceptance run (#92)`).
  `git diff 4bb1561...c9a9924 --stat` is exactly two files:
  `docs/PROGRESS.md` (+2), `docs/reviews/m2-a3-none-hosted-record.md`
  (new, +15).
- Source baseline: PR #90 main `fba8b3c993b3d173125525c760ee2dc7234e3cfa`.
- Predecessor: Standard squash `beb7117b08aad68da790e05a85b335f9655f5236`
  (`beb7117 test(exec): select bounded A3 Standard acceptance run (#91)`),
  already independently accepted for run `43635131545` by the Standard
  review retained in this tree.
- Accepted dispatch: run `43636804545`, attempt 1, `workflow_dispatch`
  on `main`, head SHA `4bb15612e30ec5bbf0c019bab347c83fd60a9d0a`
  (equals candidate; `GITHUB_SHA == ARX_REVIEWED_SHA ==
  ARX_WORKFLOW_SHA` preserved), actor `Dytschgo`, triggering actor
  `Dytschgo` (both verified via run API read this session).
- Job `132575292404` ("experiment"): success, `2026-11-14T20:18:44Z`
  to `20:20:10Z` (86 s). All eight steps success, including "Safe
  guard tests", "Fetch locked dependencies without privilege", and
  "Guard, stage, run and finally clean (fresh hosted Linux only)".
  This accepted dispatch consumed the final D7c grant. No retry.
- Fixed-schema feedback only (2 MiB log-read ceiling; no raw output
  payload or host identity retained):
  `schema unix-protection-f0 v2`, provenance source/workflow
  `4bb1561...`, run `43636804545` attempt `1`, `first: null`,
  `final: null`, `mutation_lower_bound: cgroup`, `cleanup: confirmed`,
  `internal: {service: absent, fixture-check: absent, harness: absent,
  carried: null, refusal: null}`.
- Grant consumption: `gh api
  .../actions/variables/UNIX_EXECUTION_REVIEWED_SHA` returns HTTP 404
  here (verified this session), confirming the variable is absent.
- Candidate-scope check (verified this session):
  `git diff beb7117...4bb1561` touches no `packaging/` or `crates/`
  byte; the only runtime change is the one TEST line in
  `tools/unix-exec-validation/driver.py`
  (`disposable_linux_a3_standard` to `disposable_linux_a3_none`), plus
  the Standard record/review documents and the PROGRESS line. D7c
  permits exactly this: only that TEST line plus supporting
  review/progress documents versus the pinned Standard squash.
- Spec sources read: `docs/adr/0019-unix-native-execution-bridge.md`
  (D7c), accepted plan
  `docs/reviews/m2-stagef-a3-driver-plan-muse-review.md`, source
  reviews `docs/reviews/m2-stagef-a3-source-muse-review.md` and
  `docs/reviews/m2-stagef-a3-driver-source-muse-review.md`,
  `docs/reviews/m2-d7c-a3-selection-muse-review.md` (D7c predicate),
  `docs/reviews/m2-a3-standard-hosted-record.md` and
  `docs/reviews/m2-a3-standard-hosted-muse-review.md`
  (predecessor-axis shape), `docs/reviews/m2-a3-none-hosted-record.md`
  (record under review), `docs/KICKOFF.md` (v1 scope).

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

Pass with bounded scope. The run binding matches the D7c None
grant, and the exact selected TEST joins to the full acceptance
chain already covered by the cited plan/source reviews:

- `tools/unix-exec-validation/driver.py` at `4bb1561`: `TEST` is
  exactly `exec::disposable::disposable_linux_a3_none` with
  `--ignored --exact --test-threads=1`; `gate()` requires Linux,
  `Dytschgo/arxcovo`, both actors `Dytschgo`, `refs/heads/main`, and
  `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`.
- Standard profile entirely omitted: `standard_profile()` returns
  false for the None selection, so `runtime_files()` emits only the
  exclusive `arxd.service` (from unchanged packaged bytes) plus the
  fixed `90-disposable-a1a2.conf` fixture; the whole
  `50-arxcovo-standard-rights.conf` file is absent, never a
  weakening drop-in.
- Fragment byte attribution: `verify_runtime()` requires manager
  `FragmentPath == /run/systemd/system/arxd.service` and the exact
  space-separated `DropInPaths` set-string built from the owned
  runtime files (fixture drop-in only in None mode), with per-file
  byte equality against staged package/fixture bytes; extras,
  aliases, and identical foreign copies refuse as `E_UNIT_COLLISION`.
- Proc-status parser (`crates/arxd/src/exec/hierarchy/parse.rs` at
  `4bb1561`, unchanged from baseline): `hardening()` requires
  `NoNewPrivs:\t1` exactly and `CapBnd:\t00000000000000c1` exactly
  for `Hardening::Standard`; anything else is `Hardening::None`, so
  the None assertion is the exact R4 complement.
- `disposable_linux_a3_none` (ignored) asserts `Hardening::None`
  via the production procfs reader, checks the absent ack, sends
  `READY=1`, then holds on the private `identity-ack`;
  `hold_identity_ack()` is the verbatim A1/A2 loop.
- Exact runtime attribution, READY hold, sole-PID outer checks
  (MainPID = recorded PID, `/proc/PID/exe` = protected artifact,
  supervisor `cgroup.procs` containing exactly that PID),
  exact ignored-libtest argv, one-pass/zero-failure summary
  (`test {TEST} ... ok` exactly once, `1 passed; 0 failed`),
  returncode 0 with `harness-pid` present, real systemd
  success/zero exit (`ActiveState` live then `Result == success` /
  `ExecMainStatus == 0`), bounded output/lifetime, and owned-only
  cleanup are the driver/`validate_record` joins accepted in the
  cited plan/source reviews; the observed run reports their joint
  success (`first: null`, `final: null`, `cleanup: confirmed`) with
  all job steps green and the reviewed-SHA variable cleared.

## Security

Pass. No new privileged path, secret, or trust claim. Fixed absolute
runtime paths and existing fences (exact package/runtime attribution,
sole MainPID/supervisor checks, protected staging, shared
reservation, collisions, bounded output/lifetime, owned-only cleanup)
retained verbatim; profile absence derives from the selected TEST
by file omission, never by a weaker file; no NNP/capability
override; closed codes unchanged; technical fences only, no approval
flow. The 404 variable read confirms no standing dispatch authority
remains.

## Findings and verdict

- Blockers: none.
- Credit: A3 combined only, on these two exact disposable-hosted
  runs: Standard on run `43635131545` (exact `beb7117`, already
  independently accepted) plus None on run `43636804545` (exact
  `4bb1561`, accepted here). No other path receives A3 credit. M2,
  A4-A12, full Stage F, Stage G, real-Runner PA6, installed/dedicated
  acceptance, production constructors and activation receive no
  credit and remain open/inactive; installed/dedicated paths stay
  **unverified on real hardware**. Historical `43091339273` and all
  consumed D7a/D7b environments stay excluded with no cause claim;
  no historical failure is reclassified.
- D7c is fully consumed by these two accepted dispatches; this
  record grants no further execution authority and no new native
  authority follows.
- This review grants no additional execution before its gates and
  does not merge. This documentation checkpoint may publish as a
  review-docs commit only on this PASS.

Standards 0. Spec 0. Security 0. Worst issue: none.
