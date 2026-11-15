# M2 D7g replacement native A4 omitted-S7-kill run - independent Muse review

Scope: root `0ff13465d665bc51de6a0925067a8bdc9a1b3896` (consumed skipped-D7f
record atop actual main baseline `994760c98c2816f37b1acdcaa9d9827693765b17`,
PR #114), Sol candidate `3a00f68a70cd10d52a12850d29475ad5b79e4b25` retained as
`be6b896c1835597d1d78aeadd5d9356511961447` (empty
`git diff 3a00f68 be6b896`, reviewer-observed), reviewed here as HEAD
`dbf0c590719c1f7e698dfc5e1d256e57fc8c9b3e` (coordinator PROGRESS line only
above `be6b896`). Baseline `994760c` tree
`908abcf01c0969cb73d3486e92476f3c90c132f5` equals reviewed
`d60665c81f4e83c10382fea170244ee04db98bf1` tree (reviewer `git rev-parse`
`^{tree}` at both refs). Read-only review plus local Git/blob/encoding/diff
checks; no tests, build, native/helper execution, network, dispatch, variable
operation, push, PR, subagents, or PROGRESS/ADR/source edits. Read:
AGENTS.md, KICKOFF, ADR0019 D7g + consumed D7f/preamble, ADR0020 D9a
disposition by reference, the replacement preparation note with its blob
table, and the closed skipped-run record.

## Standards - PASS, no findings

Small one-concern docs-only preparation: `0ff1346..be6b896` touches exactly
two paths (ADR0019 D7g section, new replacement preparation note), and
`994760c..be6b896` touches exactly four, all inside the D7g allowed document
set (ADR, closed skipped-run JSON, preparation note, coordinator PROGRESS).
Conventional subjects (`docs(a4): ...`, `docs(progress): ...`). LF-only, no
BOM, `git diff --check` clean (reviewer-observed on both ranges). No secrets,
owner data, paths, PIDs, tokens, or panic payloads in source, commits, or
evidence; only SHAs, run/job IDs, and sanitized outcomes. Smell baseline: no
duplication (single D7g section, single blob table), no feature envy, no data
clumps, no speculative generality (fixed single-run grant, no new
selector/parameter/workflow/tool/framework), no message chains, middle man,
or refused bequest. Tooling-enforced items (fmt/clippy) are
builder/coordinator-observed, not re-claimed here.

## Spec - PASS, no findings

The candidate implements the D7g replacement predicate with no more and no
less:

- Zero executable/non-document delta: `git diff --name-only 994760c be6b896`
  returns only the four allowed document paths, and `--raw` shows no mode
  changes (all `100644` preserved, two files added `100644`). The four fixed
  execution blobs are byte-identical at both refs (reviewer `git ls-tree`):
  `driver.py` `2180b594`, `mutation_check.py` `00740ee6`,
  `test_driver.py` `2684cc6a`, workflow `dbcca227` - matching the preparation
  note table. Root `0ff1346` delta above baseline is exactly the three
  consumed-record paths (PROGRESS, ADR D7f header, skipped-run JSON).
- Allowed document set is exactly the five D7g-enumerated paths: ADR0019,
  closed skipped-run JSON, replacement preparation note, this review note
  (`docs/reviews/m2-d7g-a4-negative-replacement-muse-review.md`), and
  coordinator-only PROGRESS. This review completes the set; no fifth-path
  drift.
- Skipped-record facts are consistent across JSON, D7f header, D7g, and
  preparation note with no overclaim: accepted dispatch `43672720190`,
  attempt 1 / job `132686133651`, `workflow_dispatch` on exact main
  `994760c`, actor and triggering actor `Dytschgo`, completed SKIPPED with
  zero steps; the dispatch command returned its run URL, an immediate
  ancillary run-list lookup returned none, and monitor finally cleanup deleted
  the pin before job admission; variable absence independently verified HTTP
  404. The record attributes the miss to eventual consistency and the loss to
  the early finally-clear racing admission, and asserts nothing about the
  runner internals beyond that recorded monitor chronology.
- Monitor correction is concrete and minimal: capture the accepted run URL
  directly from dispatch output, bind its exact run ID, hold the pin until
  that exact run is terminal even if an ancillary run-list lookup fails,
  never redispatch on lookup failure, finally clear only after the terminal
  result (or confirmed dispatch rejection with no accepted run) with
  independent HTTP 404 verification, and bound reconciliation of the accepted
  URL on uncertain identity/result instead of early deletion or a second
  dispatch. It is declared outside the repository; no workflow, tool, or
  runtime infrastructure change is authorized or present in the diff.
- Gates are exact: D7f and all older grants consumed with no retry; only the
  first eligible normal main squash containing D7g after the baseline, after
  independent different-provider exact-head review, all five green required
  checks (three platform `verify`, `ci-time-budget`, `agent-review`), and
  coordinator merge; whole-tree candidate-vs-baseline comparison with zero
  non-document delta; actual reviewed head/tree and squash SHA/tree recorded;
  main read back immediately before dispatch; advance/mismatch/substitution
  requires a new independently reviewed disposition.
- Dispatch clause is exact: set `UNIX_EXECUTION_REVIEWED_SHA` once, read back,
  dispatch unchanged `unix-execution-experiment.yml` once on one fresh
  GitHub-hosted Ubuntu 24.04 host, verify `Dytschgo/arxcovo`, actor/trigger
  `Dytschgo`, `refs/heads/main`, and
  `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA`. One accepted dispatch
  consumes D7g regardless of outcome; rejection grants no retry; failure,
  unknown result, or uncertain cleanup stops work with bounded sanitized
  feedback and host exclusion.
- All D7f native oracles and bounds are retained by reference without
  exception: fixed negative literal, one real setup, shared positive
  assertion rejection with exact `KillTimeout` and 1/1/0 writes, zero
  removals, populated retained leaf and live unreaped sole helper before
  cleanup, exactly one raw-fd LF marker, checked owned SIGKILL/reap and leaf
  cleanup before READY/identity-ack, exact outer pass summary, real systemd
  success/zero exit, independent driver finally cleanup. All staging,
  hardening, descriptor, attribution, argv, lifetime/read/output, and
  owned-only cleanup fences remain. No green-job-only or pure/mock native
  credit. The owner's delegated disposable-host authorization covers the
  bounded coordinator operations without extra approval, and this preparation
  performs none.

## Security - PASS, no findings

No new attack surface. No workflow, runner, permission, credential,
repository, selector, or provisioning change; manual-only/read-only posture,
protected staging, fixed-unit collision refusal, packaged Standard hardening,
descriptor-bound delivery, MainPID/supervisor attribution, exact
ignored/exact/one-thread argv, bounds, and owned-only finally cleanup are all
retained by reference. No owner-PC/account/admin/payment, static-VM, or backup
action is included. Evidence retains only blob IDs, SHAs, run/job IDs, and
sanitized outcomes; no secrets or owner data.

## Real checks observed

- `git diff 3a00f68 be6b896 --stat`: empty (retention identity).
- `git diff 994760c..be6b896 --stat/--name-only/--raw`: exactly four allowed
  document paths, no mode changes; `0ff1346..be6b896`: exactly two (ADR +
  preparation note); `994760c..0ff1346`: exactly three consumed-record paths.
- `git rev-parse` SHAs/trees: baseline `994760c`/tree `908abcf`, reviewed
  `d60665c` tree `908abcf` (equal); `git diff --check` on the candidate
  range: clean.
- `git ls-tree` at both refs: all four execution blobs identical, matching
  the preparation table; UTF-8/LF/no-BOM: PASS on all touched doc paths.
- PR #114 five-green/Muse PASS, PR #113 gates, and hosted run/job outcomes
  are reused coordinator/closed-record evidence, not re-run here.

## Publication eligibility

Eligible for one-concern publication through the normal gates: independent
exact-head review (this note), all five required checks, and coordinator merge
of the first eligible main squash only. This review alone grants no native
authority: the replacement run is **UNRUN and NOT granted** until those gates
pass and D7g becomes effective. No A4 acceptance before the retained
independent native assessment. A4/M2/full F/G/real-Runner PA6 stay open.
Installed/dedicated paths remain **unverified on real hardware**.
