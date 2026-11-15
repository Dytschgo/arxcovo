# M2 D7f one-run native A4 omitted-S7-kill preparation - independent Muse review

Scope: base `0d003cbdfb02772918690fb2989ce1c938efa637` (PROGRESS only atop
accepted main baseline `d0b4c5530b1c4c084a35fe40499afa96a2678b0b`, PR #113),
candidate source `a28ccbd84104fd3f94178175a449012f527abc64` retained as
`ec7255117a394a5df8bc43e15d7b4276701bf2a3` (identical tree
`ae5130889246c1cc03a9c7662df1de2e6afbbb04`, empty `git diff a28ccbd ec72551`),
candidate evidence `d1f30c62b3f0f95e09fa3ab182ff25be0588f122` retained as
`102f87d7f5142d364a61cec6084926fc0f9eb8f9` (identical tree
`59f3606be972baf8521f68fe5161a67ba939e8bf`, empty `git diff d1f30c6 102f87d`),
on `work/a4-negative-run-preparation` (reviewed here as HEAD `102f87d`).
Baseline `d0b4c55` tree `9cfd66dbdf7ce1afc43c1f5acecf4eb62fd09455` equals
reviewed `15c4d73` tree (verified via `git log --format=%T`; empty
`git diff d0b4c55 15c4d73`). Read-only review plus rerun of the audited
pure/mock suite, blob/encoding/diff checks; no helper/native/LinuxFs/ignored
fixture/probe, no install/network/dispatch/variable/push/PR/subagents, no
PROGRESS/source/ADR edits. Read: AGENTS.md, KICKOFF, ADR0019 D7f/preamble,
ADR0020 D9a, accepted plan, plan review, native-negative source evidence and
source review, hosted baseline feedback, baseline assessment, and the
preparation evidence note with its blob table.

## Standards - PASS, no findings

Small one-concern preparation: `0d003cb..HEAD` touches exactly five paths -
ADR0019 D7f, the new evidence note, and the three execution files. No
fixture/helper/workflow/package/Cargo/prod/pin change. Conventional subjects
(`test(a4): ...`, `docs(a4): ...`). LF-only, no BOM, `git diff --check` clean
(reviewer-observed). No secrets, owner data, paths, PIDs, or panic payloads in
source, commits, or evidence. The test rename is honest
(`test_source_selection_is_a4_omitted_kill` asserts the omitted-kill literal);
the marker/summary delta is confined to `test_exact_summary` plus the two
literal default assertions. Smell baseline: no duplication (single TEST
literal swapped, single marker prefix shared by the summary cases), no feature
envy, no data clumps, no speculative generality (fixed literal, no new
selector/parameter/framework), no message chains, middle man, or refused
bequest. Tooling-enforced items (fmt/clippy) are builder/reviewer-observed,
not re-claimed here.

## Spec - PASS, no findings

The candidate implements the D7f exact-candidate predicate with no more and
no less:

- Exactly three non-document execution blobs differ from baseline, and their
  HEAD blobs equal the evidence "prepared blob" column, which equals the
  accepted-baseline "at baseline" column at `d0b4c55`: `driver.py`
  `7d30c46b` -> `2180b594`, `mutation_check.py` `d997dd93` -> `00740ee6`,
  `test_driver.py` `f9ec1a36` -> `2684cc6a` (reviewer `git ls-tree -r` at
  both refs). `git diff --name-only d0b4c55 HEAD` filtered to executable
  suffixes returns only those three; the full `0d003cb..HEAD` name-status is
  exactly M ADR + A evidence + M 3 execution files. Spot-checked unchanged
  execution blobs are byte-identical at both refs (workflow `dbcca227`,
  `disposable/a4.rs` `b17a9619`, helper `helper.rs` `5a02cf43`).
- Driver delta is the one fixed TEST literal to
  `exec::disposable::a4::disposable_linux_a4_omitted_kill`; test delta is the
  focused renamed selection assertion, the focused default assertion, and the
  exact-summary mock marker correction with all 201 assertion calls preserved
  (reviewer AST count: 201); mutation delta is the focused inverse anchor
  restoring positive A4 as the counterfactual.
- D7f carries every required gate verbatim: D7a-D7e consumed; only the first
  eligible normal main squash containing D7f after exact-head independent
  review, all five green required checks (`verify` x3, `ci-time-budget`,
  `agent-review`) and coordinator merge; whole-tree candidate-vs-baseline
  comparison with the exact allowed doc set; read-back before dispatch;
  mismatch/advance/substitution requires a new disposition.
- Dispatch clause is exact: set `UNIX_EXECUTION_REVIEWED_SHA` once, read back,
  dispatch unchanged `unix-execution-experiment.yml` once on one fresh
  GitHub-hosted Ubuntu 24.04 host, verify repository `Dytschgo/arxcovo`,
  actor/trigger `Dytschgo`, `refs/heads/main`, source/workflow SHA equality;
  finally clear with independent HTTP 404 verification. Accepted dispatch
  consumes D7f regardless of outcome; no retry/rerun/reuse; rejected dispatch
  grants no retry; failure/unknown/uncertain cleanup stops work with bounded
  sanitized feedback and host exclusion.
- Native oracle joins every required element, not a green job alone: shared
  inner positive `assert!(accepted)` rejection caught only at that assertion,
  exact `Unavailable(KillTimeout)`, 1 reached / 1 omitted / 0 forwarded S7
  writes, zero removals, populated retained literal leaf and live unreaped
  sole helper with original lifetime remaining before failure cleanup, fixed
  raw-fd `ARX_A4_OMITTED_S7_REJECTED` LF marker exactly once, checked
  owned-child SIGKILL/reap and owned-leaf cleanup before READY/identity-ack,
  outer exact test-ok one-pass/zero-failure summary, real systemd
  success/zero exit, independent driver finally cleanup. Prior grants stay
  consumed; all previous experiment hosts excluded; no owner-PC/account/admin,
  static-VM, or backup action; delegated disposable-host coordinator ops need
  no further human approval and this preparation performs none.

## Security - PASS, no findings

No new attack surface. No workflow, runner, permission, credential, or
provisioning change; manual-only/read-only posture, protected staging,
fixed-unit collision refusal, packaged Standard hardening, descriptor-bound
delivery, MainPID/supervisor attribution, exact ignored/exact/one-thread argv,
bounds, and owned-only finally cleanup are all retained by reference. The only
runtime selector change is a fixed literal to an already-reviewed test-only
negative case; no user-controlled input reaches it. Evidence retains only blob
IDs, SHAs, run IDs, and sanitized outcomes; no secrets or owner data.

## Real checks observed

- `python -B tools/unix-exec-validation/mutation_check.py`: PASS, 91 tests OK;
  40 Python assertion kills listed; Rust pure baseline 7 passed; 8 Rust
  assertion kills; `source unchanged sha256=4865f8fe...`, Rust fingerprints
  unchanged (reviewer-observed this session).
- Assertion preservation: 201 `assert*` calls in `test_driver.py`
  (reviewer AST count), matching the evidence claim.
- `git diff --check 0d003cb HEAD`: clean. UTF-8/LF/no-BOM: PASS on all five
  touched files. Baseline/reviewed tree equality and candidate retention
  identity verified as above. Hosted five gates and PR #113 Muse PASS are
  reused coordinator evidence, not re-run here.

## Publication eligibility

Eligible for one-concern publication through the normal gates: independent
exact-head review (this note), all five required checks, and coordinator merge
of the first eligible main squash only. This review alone grants no native
authority: the omitted-kill run is **UNRUN and NOT granted** until those gates
pass and D7f becomes effective. No A4 acceptance before the retained
independent native assessment. A4/M2/full F/G/real-Runner PA6 stay open.
Installed/dedicated paths remain **unverified on real hardware**.
