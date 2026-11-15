# M2 A4 native omitted-S7-kill hosted result — independent Muse assessment

Scope: prepared `6cfb814` (coordinator `docs(a4): record bounded native
omitted-kill result`) on `work/a4-native-negative-result` atop actual main
`1958f9975d1620151ca9a182c5047dd8aeb43e65` (PR #115). Read-only review plus
read-only local Git/blob inspection; no tests, build, helper/native execution,
LinuxFs/probe, dispatch, variable operation, settings, push, PR, subagents, or
source/PROGRESS/ADR edits. Read: AGENTS.md, KICKOFF, ADR0020 D9a (with the
test-only omitted-kill clarification), ADR0019 D7g plus consumed D7f/D7e, the
accepted omitted-kill plan and its Muse plan review, the native-negative source
evidence and source review, the D7g replacement review, the positive baseline
feedback record and baseline assessment, and the new evidence
`docs/evidence/linux-a4-negative-run-43674201597.json` and
`docs/evidence/linux-a4-native-negative-hosted-result.md`. Run/job metadata was
assessed from the committed closed record only; no live API re-query and no log
fetch was needed, and no raw payload is printed or retained here.

## Standards — PASS, no findings

Docs/evidence-only assessment scope: this note adds one review file and commits
only it. Small one-concern conventional commit; LF-only, no BOM, `git diff
--check` clean (verified at commit time). No secrets, owner data, paths, PIDs,
tokens, panic payloads, or raw fixture/helper bytes in the assessment. Smell
baseline: no duplication (single assessment of a single run), no feature envy,
no data clumps, no speculative generality (fixed single-run join, no new
selector/parameter/framework), no message chains, middle man, or refused
bequest. Tooling-enforced items (fmt/clippy) are builder/coordinator-observed,
not re-claimed here.

## Spec — PASS: source+run join closes ONLY D9a A4 omitted-kill scope

The executed source is the reviewed source. Reviewer-verified this session:

- `git diff --name-only 994760c 1958f99` returns exactly five document paths
  (PROGRESS, ADR0019, replacement preparation, skipped-run JSON, D7g review);
  zero executable/non-document delta, no mode changes.
- Fixed execution blobs byte-identical at both refs (reviewer `git ls-tree`
  at `994760c` and `1958f99`): `driver.py` `2180b594`,
  `mutation_check.py` `00740ee6`, `test_driver.py` `2684cc6a`, workflow
  `dbcca227`, `disposable/a4.rs` `b17a9619`. PR114's fixed negative TEST
  literal therefore ran unchanged.
- Reviewed-head binding: reviewer `git rev-parse` shows reviewed
  `28cc16be006fe641834db0efe73ca9d14ee6579^{tree}` =
  `90560c2ea43469ad64b36932fd225c74b33efc26` = squash `1958f99` tree.
  The executed SHA is the independently reviewed tree.

The fixed oracle was verified in the actual executed code, not only in
records. At `1958f99` (`git show`, read-only):

- Outer (driver `2180b594`): `TEST =
  'exec::disposable::a4::disposable_linux_a4_omitted_kill'`; pass requires
  exactly one `test {TEST} ... ok` line plus `test result: ok. 1 passed; 0
  failed;`, returncode 0, harness-pid present, real systemd success/zero exit,
  and independent driver finally cleanup (`feedback_cleanup == 'confirmed'`).
- Inner (`disposable/a4.rs` `b17a9619` + hierarchy oracle): both literal
  entries share one fixed spawn/seed/placement body via `fixture(Case)`;
  the negative arm builds test-only `OmitS7` omitting exactly
  `write(runs, cgroup.kill, 1)` (returns Ok, counts the attempt), runs one
  real setup, catches only the shared positive `assert!(accepted)` rejection,
  and requires exactly `Unavailable(KillTimeout)`, attempts 1 / omitted 1 /
  forwarded 0, zero removals, retained populated literal leaf and live
  unreaped sole helper with original lifetime remaining **before** failure
  cleanup; then exactly one raw-fd `ARX_A4_OMITTED_S7_REJECTED\n` marker,
  checked owned-child SIGKILL/reap and owned-leaf emptiness/removal/absence
  **before** READY/identity-ack (`a5::empty`, `NOTIFY READY`,
  `hold_identity_ack`).

The run record joins that oracle to a real disposable host:

- Run `43674201597`, attempt 1 / job `132690637470`, `workflow_dispatch` on
  `main`, actor and triggering actor `Dytschgo`, exact source SHA `1958f99`,
  conclusion `success`, `2026-11-15T03:51:26Z–03:52:58Z` (92 s). All nine
  steps `success`, including safe guard tests, locked-dependency fetch,
  unprivileged helper compile/compare, and guarded stage/run/finally cleanup.
- Bounded feedback: schema `unix-protection-f0` v2, provenance
  source/workflow `1958f99`, run `43674201597`, attempt `1`; `first`/`final`
  null, `mutation_lower_bound` `cgroup`, `cleanup` `confirmed`, internal
  service/fixture-check/harness `absent`, carried/refusal null.
- Pin discipline: the monitor bound the URL returned by dispatch, held it
  through that exact run's terminal, then finally cleared it; terminal/404
  plus an independent API HTTP 404 confirm absence. D7g is consumed; D7f
  stays consumed by its skipped zero-step run; no further native authority.

Positive baseline `43666413693`/`a328060` is joined as the counterfactual, not
as acceptance: it proves the same fixture/driver path accepts with S7 present,
while `mutation_check.py` restores positive A4 as the inverse anchor. The
fixed source oracle (exact KillTimeout + 1/1/0 + zero removals + retained-leaf
+ marker + owned cleanup + outer pass) proves the join, so no optional
telemetry beyond the retained fixed-schema feedback is demanded, and the green
job alone is not the credit — the driver-enforced oracle entailment is.

Credit: the combined source+run evidence closes **ONLY** D9a A4 —
disposable present-residue mechanism (kill one preseeded bounded populated
leaf, confirm emptiness, remove it before admission) plus the native
omitted-S7-kill negative. It does not close crash lifecycle or systemd
directory removal (A9/U9 stay independently required), M2, A7–A12, full
Stage F, Stage G, real-Runner PA6, or installed/dedicated paths (which remain
**unverified on real hardware**).

## Security — PASS, no findings

No new attack surface in the assessed scope. No workflow, runner, permission,
credential, repository, selector, or provisioning change between baseline and
executed SHA; manual-only/read-only posture, protected staging, fixed-unit
collision refusal, packaged Standard hardening, descriptor-bound helper
delivery, MainPID/supervisor attribution, exact ignored/exact/one-thread argv,
bounds, and owned-only finally cleanup are retained by reference from D7f/D7g.
The closed record retains only SHAs, run/job IDs, and sanitized fixed-schema
outcomes; no raw harness output, helper bytes, identity, token, or private
payload. Bounded local Git reads only; no network, dispatch, or variable
operation was performed for this assessment.

## Remaining limits

- Live run/job API and the HTTP 404 pin check were not re-queried here; the
  committed closed record plus the recorded coordinator monitor chronology and
  independent 404 verification are the evidence. Per-step timestamps are not
  in the record (job-level 92 s only).
- Raw fixture/helper output is not retained by design (1 MiB capture bound);
  inner-oracle firing is entailed by the fixed driver acceptance, not
  re-observed.
- A4 credit is limited to the D9a disposable scope above. M2, crash/A9/U9,
  full F/G, real-Runner PA6, and installed/dedicated hardware acceptance stay
  open. No further native execution is authorized; D7g is consumed.
