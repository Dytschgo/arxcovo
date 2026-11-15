# M2 A4 hosted baseline feedback — independent Muse assessment

Scope: prepared head `da236e1e32ed26566cd678d42b6d80c8fb12cede`
(`docs(a4): record consumed hosted baseline feedback`) on local branch
`t3/review-a4-baseline-feedback`, atop `514538a` (progress only) atop main
candidate `a328060000b8cb7bb389d39f8785f96f6bcb0c25` (PR #109). Base for the
candidate is accepted baseline `8e4bf824b56a2c4df141f9ef8cd78616d9b53475`.
No source, variable, settings, dispatch, push, PR, subagent, build, test, or
native operation was performed for this assessment; only committed file reads
and read-only `git` inspection. No raw log, secret, image, or publishable
artifact is retained here.

Evidence read: [feedback](../evidence/linux-a4-hosted-baseline-feedback.md),
[run record](../evidence/linux-a4-baseline-run-43666413693.json), ADR0019 D7e,
ADR0020 D9a, `a4-d7e-execution-baseline.json`, preparation and driver-delivery
notes, retained [baseline review](m2-d7e-a4-baseline-muse-review.md), and the
ten pinned source/fixture/helper/driver/manifest blobs at the candidate.

## Root verification (read-only)

- `a328060000b8cb7bb389d39f8785f96f6bcb0c25`, tree
  `48b92d548fb6ef5373f7f477ac8bda2f0cb04717`, single parent `8e4bf824`.
  Recorded squash tree equals reviewed `bcff99c`; statement reused from the
  retained full normal review and feedback record, not re-proven here.
- Full-review credit reused: PR #109 five gates green and independent Muse
  exact-head PASS `bcff99c` are recorded in the feedback note and PROGRESS.
  This assessment does not re-run gates, tests, builds, or native paths.
- Baseline inventory `a4-d7e-execution-baseline.json` pins 113 blobs at
  `8e4bf824` (tree `d12edb06bfd87374b6f94cc8b0800a1554e4ed5e`). The
  `8e4bf824..a328060` name-only delta is exactly six non-doc execution paths
  plus five supporting doc/review/evidence paths (11 paths total); the six
  are `driver.py`, `unix-execution-experiment.yml`, `mutation_check.py`,
  `test_driver.py`, `test_f0_blockers.py`, `test_f0b_transport.py`.
- Candidate blob pins in the feedback note verified by `git ls-tree -r
  a328060` (all match): `driver.py`
  `eb085b3e416f9bd8ca08c87c95c84fcd27cafd18`, workflow
  `dbcca2276307357301719ed68a09631b8ab7a10a`, `helper.rs`
  `5a02cf43d8abaa29e221be4ac54221e2ae05354f`, `build.py`
  `5412b0cf345d3bd5dd0106e972a6b10e12827b7b`, `linux-image-pin.json`
  `43e23c1b8597eead804e78cdf69c58139a788bc0`, `disposable/a4.rs`
  `dfa36d53f0176a8c88b52303a44502ab7f4c82b3`, `hierarchy/a4.rs`
  `64e1ddb679ed3517e776735933dc59b2442256d6`, `hierarchy/linux.rs`
  `4ad78a672679226fc11c7899a6ef6b033f833a7d`, `arxd.service`
  `0b337773c60e49fe84de76db018d2235ae076254`, `50-arxcovo-standard-rights.conf`
  `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74`.
- `driver.py` at candidate selects only
  `TEST = 'exec::disposable::a4::disposable_linux_a4'`. Workflow at candidate
  preserves the reviewed manual gates and adds the single unprivileged
  `build.py linux-candidate` compile/compare step before the privileged
  driver step. Variable-absent / no-prior-run-before-once-dispatch statements
  are reused from the retained review and feedback record.
- Run record `43666413693`, attempt 1, job `132666674807`,
  `workflow_dispatch` on `main`, actor and triggering actor `Dytschgo`, exact
  source/workflow SHA equals the candidate. Job
  `2026-11-15T02:09:20Z–02:10:33Z` (73s), conclusion `success`; all nine
  listed steps `success`, including guard tests, locked-dependency fetch,
  unprivileged helper compile/compare, and guard/stage/run/finally-clean.
  Feedback `first`/`final` null, `mutation_lower_bound` `cgroup`, `cleanup`
  `confirmed`; `internal` service/fixture-check/harness `absent`,
  carried/refusal null. Recorded reviewed-SHA variable deletion plus
  independent HTTP 404 read-back are stated in the feedback note; the live
  variable/API check was not re-run here. Bounded 2 MiB read-only GH
  metadata/job check was authorized but not needed for this file-scope
  assessment and was not exercised.

## Standards

Source policy for the two assessed commits (`514538a` progress-only,
`da236e` feedback record) is docs/evidence only. No production, fixture,
driver, workflow, package, Cargo, selector, permission, or timeout byte is
touched by either commit. Small conventional commits, one concern each.
`git diff --check` state is left to the final commit verification below.

Smell baseline (fixed set): no Mysterious Name, Duplicated Code, Feature
Envy, Data Clumps, Primitive Obsession, Repeated Switches, Shotgun Surgery,
Divergent Change, Speculative Generality, Message Chains, Middle Man, or
Refused Bequest applies to a feedback JSON plus prose record. Tooling-enforced
items skipped.

Standards: PASS, no findings.

## Spec (D7e join vs D9a scope)

- Source join verified as source assertion (not a hardware claim): candidate
  `TEST` selects only the ignored A4 fixture; `hierarchy/a4.rs` pins the
  fixed Linux image hash
  `cee895412d58718016dd0db9883526ef8e0b13f8ed00cf213c6f77536b206369`
  with size 3680 (matching `linux-image-pin.json` read here); Standard
  profile unit and rights drop-in blobs match the ten pins above; D9a items
  1–5 (fixed single-task helper, S7-only kill, surviving-parent waitpid with
  SIGKILL requirement, trusted exclusive leaf limits, exact
  postconditions/READY hold/exact summary/systemd success/owned cleanup) are
  the source-side contract the driver joins against. The run record's
  first/final-null, cgroup lower bound, and cleanup-confirmed feedback is the
  only hosted observation joined to that source assertion.
- What the run does not prove: no native omitted-kill evidence, no A4
  acceptance, no M2/A7–A12, full Stage F, Stage G, real-Runner PA6, or
  installed/dedicated-path acceptance. Hardware paths remain **unverified on
  real hardware**. This matches the feedback note's explicit limit.
- Docs/ADR consumed-status truth: ADR0019 D7e records itself consumed by
  accepted dispatch `43666413693` on exact `a328060`, with success/cleanup
  recorded and assessment pending; PROGRESS records D7e consumed with the
  same run/job/SHA and the same open items. Both statements agree with the
  run JSON and the local commit chain. No new run authority exists; D7e
  permits no retry. D7a–D7d remain consumed; D9a native omitted-kill stays
  **unrun and NOT granted**.

Spec: PASS, no findings. Credit is at most disposable positive baseline
feedback; it is not A4/M2 acceptance.

## Security

- No workflow, permission, secret, owner-data, network, publication, MCP,
  path, or reparse-point surface is changed by the assessed commits.
- The feedback file retains only the fixed-schema record (provenance,
  nulls, cgroup lower bound, cleanup, absent internals); no raw harness
  output, helper bytes, identity, token, or private payload is present in the
  files read.
- Fences preserved by reference: manual trigger, exact repo/actor/ref/SHA
  equality, read-only credentials, fresh disposable host, fixed unit,
  packaged Standard profile, protected staging, collision refusal, bounded
  output/lifetime, owned-only cleanup, finally-delete with 404 verification.

Security: PASS, no findings.

## Verdict

Independent assessment accepts
`da236e1e32ed26566cd678d42b6d80c8fb12cede` as a truthful record of one
consumed, authorized A4 baseline feedback run. It grants no A4 acceptance,
no native omitted-kill credit, no M2/A7–A12/F/G/PA6 credit, and no further
run authority. Native omitted-kill remains UNRUN and NOT granted; all
hardware paths remain **unverified on real hardware**.
