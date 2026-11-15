# M2 A7/A11 integration source review - Muse (independent source/security)

Scope: base `a8d5e9f` (PR126 merge) to HEAD `0b21a05` on `t3/muse-source-security-review`.
Diff: 4 source paths + evidence + PROGRESS queue note, 663 insertions / 81 deletions.
Sol source `95a548a` and evidence `f5d2cbd` retained verbatim as `e5fc9ad` / `da595de`
(empty diffs verified); no PASS exists from prior attempt `758d2f8e`.
Read, in order: AGENTS.md, `docs/KICKOFF.md`, ADR0020 (hierarchy/janitor),
[delivery contract](m2-a7-a11-delivery-contract.md) with binding budget amendment,
[budget proposal](m2-a7-a11-integration-budget-proposal.md) and
[budget review](m2-a7-a11-integration-budget-muse-review.md) (pins 1-4 + Spec pins 1-2),
`docs/evidence/linux-a7-a11-integration-source.md`, PROGRESS.md.
Inspected the actual call graph on all three axes. No build, test, Clippy, network,
host, root, helper, sudo, dispatch, variable, push, PR, merge or watch ran.
Builder-reported checks (fmt, Windows Clippy, pure suites, mutants) are reused, not rerun.
Corroborated read-only: driver SHA256 `61222018...bcccd` and controller SHA256
`855ebd43...b1b2` match the evidence; all four source files are LF-only (0 CR bytes);
`TEST` remains `exec::disposable::a4::disposable_linux_a4_omitted_kill`;
no secrets or owner data in the diff (only `Cancellation` token and hunk-header matches).
A7/A11, M2, full F/G and real-Runner PA6 remain **unverified on real hardware**.

## Standards - PASS, no findings

- Scope discipline holds: only the four contracted paths change
  (`crates/arxd/src/exec/disposable/sudo_nnp.rs`,
  `tools/unix-exec-validation/{driver,test_driver,mutation_check}.py`).
  No source, ADR, workflow, packaging, Cargo, manifest, guard or pin relaxation.
- Small conventional commits; UTF-8 LF; Markdown review-doc only from this review.
- Style matches surroundings: `require()` oracles, fixed literals, fail-closed
  `false` returns, no product approval flow, no publishing default change.
- Smell baseline (judgement calls, none hard): `a7_identity_ack` duplicates the
  `hold_identity_ack` shape, but the contract explicitly requires a separate 5s
  fixture envelope (the shared hold waits 120s), so the duplication is pinned, not
  removable. `a7_cases` / `a7_completion` / `a7_envelopes` are the minimal shared
  scope between the ignored controller and pure counterfactuals, not speculative
  generality. No message chains, middle men or shotgun surgery.
- Observation (not a finding): `monitor_a1a2` gains one unconditional line,
  `require(deadline > time.monotonic(), 'E_TEST_TIMEOUT')` at terminal observation.
  It tightens the shared A1A2/A4 path fail-closed in the same direction as the
  amendment; builder suites covering that path report green.

## Spec - PASS, no findings

Against the binding contract plus budget amendment, per clause:

- Setup/admission before READY within 15s: `startup` timer starts first,
  `prepared_delivery` refusal, env/root checks, `prepare_a1a2`, per-account
  `recheck_delivery` + `Runner::new`, then `assert!(startup.elapsed() <= 15s)`
  before `READY=1` ([sudo_nnp.rs:1092-1131](../../crates/arxd/src/exec/disposable/sudo_nnp.rs#L1092)).
- Identity ACK within 5s before any Runner child: `a7_identity_ack` polls a
  root-owned 0600 nlink1 `identity-ack` containing `1`, refuses unless the whole
  wait fits 5s, and runs strictly between READY and `a7_cases`.
- Exactly two scoped workers, one owned account/Runner domain each:
  `runners.each_ref()`, `callers[2]` / `shutdowns[2]`, `handles = [0, 1].map(...)`.
- Real prod probe then fixed E4 reentry: step 0 runs `prepared.arxd` with
  `["tool-probe"]` at 55s; step 1 runs `prepared.reentry` with the exact fixed
  `--exact ... e4_sudo_nnp_reentry --ignored --nocapture --test-threads=1
  --color=never` argv at 10s; `output_bytes: 4096` both.
- Per-call reopened checks: `recheck_delivery` (account/artifact/view pins,
  NOFOLLOW+CLOEXEC held fds) runs before `Runner::new` and before each
  `run_phased`; `_held` is retained through the call.
- Envelopes: full synchronous call measured at <=60s/15s including error returns
  and native guard drops; cumulative non-call work <=15s; case total <=90s via
  `a7_envelopes`; refusal (never detach/abort) on any excess.
- Cancellation/join: `catch_unwind` inside each worker cancels all four
  caller/shutdown tokens on failure; the parent receives either completion via
  channel (never blocks on a healthy join while hiding the other failure), then
  joins BOTH workers without short-circuiting (`joined == [true, true]`
  required); final join/completion measured <=10s. Cooperative-cancellation
  limits are stated, refusal-only, matching the amendment.
- Markers/oracles: fixed `ARX_A7_A11_CASE0_OK` / `CASE1_OK` / `COMPLETE_OK`
  records bypass libtest capture via raw stdout (same mechanism as the accepted
  A4 marker); driver requires each exactly once plus exactly one
  `test result: ok. 1 passed; ...` summary, ACK, `Result=success`,
  `ExecMainStatus=0`, terminal observation before 120s, and stop-gated
  owned-only cleanup (policy first, residue/pending refuse).
- Dormancy: `a7_selected()` derives solely from the fixed TEST literal (still
  closed at the A4 omitted-kill); `a7_admission()` always raises
  `E_POLICY_UNATTESTED` on the `None` profile with short-circuit before any
  host/profile/query call, so `compare()` is unreachable; `sudo_nnp_host_attested()`
  stays `False`; the staged build/handoff/cleanup wiring is unreachable until a
  separately reviewed grant. No installed adapter, native grant, qualification or
  host policy query.
- Fences/pins: `mod disposable` stays `#[cfg(all(test, target_os = "linux"))]`;
  the crate-wide inventory pins (`mod sudo_nnp;`, declaration list) are untouched
  and the new code lives inside the already-inventoried test-only file; extraction
  anchors (`fn a7_cases(`, `// Pure discriminators`) are present for the
  source-only checks. New `a7_a11_scheduling_counterfactuals` is pure
  (threads/barriers/atomics only, no I/O).

## Security - PASS WITH CONSTRAINTS, no new exposure

- Fail-closed posture retained: `prepared_delivery()->None`, False/None gates,
  owned-only cleanup with reservation/quarantine on partial capture, changed
  inode or uncertain stop; per-run sudo, in-service `/etc` mutation, installation
  and policy inference all absent.
- Two workers are scoped to the two fixed fixture accounts with per-call
  view/account/artifact checks, pinned 4096/64MiB/swap0/pids32, derived
  names/workspaces, and no caller paths or artifact overrides.
- Parent cancel-and-join-both on failure/panic closes the orphan-worker race;
  the strict both-case oracle closes single-case, early-READY and early-ACK
  acceptance. Marker bytes are fixed and carry no identities.
- Constraints (load-bearing, unchanged from the contract): full Linux cfg
  compilation was impossible locally (no cross gcc) and must pass in hosted CI;
  synchronous OS blocking can exceed envelopes, so the contract guarantees
  refusal of late results, not hard native completion. No new right to raise old
  fixture limits; 55/10 apply only to this fixture.

## Verdict: eligible for publication, no source blockers

Findings: Standards 0; Spec 0; Security 0 new issues (2 standing constraints above).
Worst item per axis: none beyond the noted shared-path oracle tightening
(Standards observation) and the standing hosted-compile requirement (Security
constraint). The slice may proceed through exact-head review and green required
gates; hosted Linux compilation remains required before merge, and any execution
still needs the separate native grant pinning both cases, source, artifacts,
workflow, profile, bounds, one dispatch and cleanup/variable lifecycle on a fresh
host. D7a-h stay consumed; no retries or host reuse. A7/A11, M2, full F/G and
real-Runner PA6 remain **unverified on real hardware**.
