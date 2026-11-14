# Muse review: first successful hosted A1/A2 evidence (b765536)

Single-agent read-only review. No builds, tests, native operations, dispatches,
retries, owner/service/account actions, subagents, or source edits were used.
Evidence: file reads plus read-only GitHub metadata (`gh run view`, `gh api`
for run 43627472312, PR 86, and the reviewed-SHA variable 404). No raw logs,
payloads, or private principals are retained here.

## Pins (repo-relative, exact SHA)

- Under review: `b765536da2824867cf4aee4b1579d78e176c980c`
  ("docs(evidence): retain successful bounded A1/A2 run"),
  atop main `c23e0b4ae33878cff0d28d84738d11ea03986bf9`.
  Scope verified by `git diff c23e0b4...b765536`: only
  `docs/PROGRESS.md` (+2), `docs/evidence/linux-stagef-a1a2-hosted-success.md`
  (new), `docs/evidence/linux-stagef-a1a2-run-43627472312.json` (new).
- Source baseline: PR 85 `7afea2fdcb17268396fbdb17c36ee8cf991e2ce4`.
- D7b grant: PR 86 merged `1e9d8818de1e337b1402ca7e2b71ac02456b82f3`
  at 2026-11-14 18:35:23Z (PR API: base `7afea2f...`, head
  `4a025ed6510de866c8db4f0e932e34d520605fb2`, merged true).
  Local `git diff 7afea2f..1e9d881 --stat` confirms docs-only:
  `docs/PROGRESS.md`, `docs/adr/0019-unix-native-execution-bridge.md`,
  `docs/reviews/m2-d7b-corrected-parent-grok-review.md`.
  The reviewed branch head `4a025ed...` is not present in this clone, so
  squash-tree equality with the merge is taken from the PR record and the
  retained Grok review, not re-derived locally.
- Accepted dispatch: run `43627472312`, attempt 1, `workflow_dispatch` on
  `main`, head SHA `1e9d8818de1e337b1402ca7e2b71ac02456b82f3`
  (matches source SHA), actor `Dytschgo`, triggering actor `Dytschgo`.
  Job `132546488843` ("experiment") ran 18:35:47–18:37:01Z (74 s),
  conclusion `success`; all reported steps `success`, including "Safe guard
  tests", "Fetch locked dependencies without privilege", and
  "Guard, stage, run and finally clean (fresh hosted Linux only)".
- Feedback record `docs/evidence/linux-stagef-a1a2-run-43627472312.json`:
  `schema unix-protection-f0 v2`, provenance source/workflow
  `1e9d881...`, run `43627472312` attempt `1`, `first: null`,
  `final: null`, `mutation_lower_bound: cgroup`, `cleanup: confirmed`,
  `internal: {service: absent, fixture-check: absent, harness: absent,
  carried: null, refusal: null}`.
- Grant consumption: `gh api .../actions/variables/UNIX_EXECUTION_REVIEWED_SHA`
  returns HTTP 404 here, confirming the variable is absent.

## Standards

Pass. The prepared commit is docs/evidence only, one concern, with a
conventional message. It adds no code, workflow, permission, secret, owner
operation, product approval flow, or optional infrastructure, matching the
request for proportionate fences. `docs/evidence/linux-stagef-a1a2-hosted-success.md`
explicitly defers acceptance to independent review, disclaims M2/full F/G/PA6,
and marks installed/dedicated paths **unverified on real hardware** per
AGENTS.md. No baseline smell applies to this prose/JSON delta beyond noting
the PROGRESS line and the evidence note restate the same run facts, which is
expected for a checkpoint. No findings.

## Spec

Pass with bounded scope. The cited driver and ignored fixture, as read in-tree,
imply what A1/A2 needs, and the run binding matches the D7b grant:

- `tools/unix-exec-validation/driver.py`: `gate()` requires Linux,
  `Dytschgo/arxcovo`, both actors `Dytschgo`, `refs/heads/main`, and
  `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`; `TEST` is exactly
  `exec::disposable::disposable_linux_a1_a2` with `--ignored --exact
  --test-threads=1`; `monitor_a1a2()` compares `MainPID` to the harness PID,
  `/proc/<pid>/exe` to the built artifact, and the cgroup line to
  `0::/system.slice/arxd.service/supervisor` while the test is held on the
  private ack, then requires `test ... ok`, `1 passed; 0 failed`,
  `Result=success`, `ExecMainStatus=0`, the 1 MiB ceiling, and cgroup absence
  before removing only owned files.
- `crates/arxd/src/exec/disposable.rs` (`disposable_linux_a1_a2`, ignored):
  asserts the four disposable gates, root euid/uid, then calls
  `hierarchy::fake::prepare_a1a2(&LinuxFs::system(), ...)` before
  `Notify::send_to(... "READY=1")`, and holds on the private `identity-ack`.
- `crates/arxd/src/exec/hierarchy/fake.rs` `prepare_a1a2()` calls production
  `setup()` and additionally requires `runs_created && unit_written &&
  runs_written && leaves_removed == 0`, re-walks the unit/`runs` path,
  requires both `subtree_control` files to carry `memory`+`pids`, empty
  `cgroup.procs`, unpopulated `cgroup.events`, no non-control entries, and the
  three limit files openable.
- `validate_record()` in the driver accepts exactly this shape:
  `final: null` requires `first: null`, `cleanup: confirmed`,
  `mutation in (service, cgroup, account)`, and all internal `absent`.
  The retained JSON satisfies it. Internal-absent alone is not the claim;
  the completed workflow, step successes, exact SHA/binding, and the
  driver/fixture assertions above jointly support A1/A2 credit.
- Binding matches D7b: single accepted dispatch on the exact merge SHA;
  no repeat dispatch is evidenced (run number 3, attempt 1, success).

## Verdict

- Standards: 0 findings. Spec: 0 findings. Worst issue: none.
- Credited: hosted disposable A1/A2 on exact `1e9d881` via run
  `43627472312`/job `132546488843` — setup-before-children through production
  `setup`, exact packaged unit `MainPID`/exe/`supervisor` identity with
  `READY`, 1 MiB/120 s bounds, `systemd success`/`ExecMainStatus=0`, and
  confirmed cgroup cleanup with the owned-only removal path. The consumed
  grant (variable absent, HTTP 404) and no-retry posture are confirmed.
- Remain open (no credit): M2, full Stage F, Stage G, real-Runner PA6,
  historical run `43091339273` and main-71 causes (unknown, excluded
  environments not retried), and installed/dedicated paths, which stay
  **unverified on real hardware**. Optional diagnostics stay parked.
- This review permits a docs checkpoint publication only. It grants no
  further run, retry, activation, or production authority.
