# M2 D7e A4 baseline candidate — independent review (Muse)

Scope: branch `work/a4-d7e-baseline-preparation`, head
`3903ddf448cef2553f6d9af1b9ac522ea166143` (reviewed; local `t3/d7e-a4-baseline-review`
checkout tracks the same commit), base `origin/main`
`1869175ba31e5c81386f45d293ed50365081fb1f` (PR #108, all five gates green).
`git status` clean; `git diff --check` clean. No push, dispatch, variable,
workflow, native, helper, publication or subagent operation performed.

## Standards

Source policy is preparation only; this review grants no run permission.

- Non-doc delta is exactly six paths, each minimal: `driver.py` one TEST line
  (`disposable_linux_a5` to `a4::disposable_linux_a4`); `mutation_check.py` one
  inverse-anchor line (same selection move); `test_driver.py` +9/−4 (two
  literal A4 selection assertions, one explicit historical-A5 non-A4 refusal
  pin, two legacy-lifecycle A5 mode pins with comments); `test_f0_blockers.py`
  +2 and `test_f0b_transport.py` +2 (one explicit historical-A5 mode pin each,
  comments only otherwise); `unix-execution-experiment.yml` +2 (one
  unprivileged `build.py linux-candidate` compile/compare step before the
  privileged driver step).
- No Rust, helper, recipe, image-pin, package, Cargo, toolchain, workflow
  permission, timeout, selector or fence byte changed. Driver logic outside the
  TEST line is untouched; all assertions, fences, bounds and cleanup order
  retained. A4 admission coverage stays in its dedicated mocks; the four
  legacy lifecycle fixtures keep every assertion and only pin historical A5
  explicitly, so no A4 test was patched out and nothing was weakened to fit.
- Docs delta is additive except a four-line D7 preamble rollover (D7d-proposes
  to D7e-proposes, consumption labels); no execution-semantics or evidence
  content loss. Commits are small and conventional (`test(a4)`, `docs(a4)`,
  `docs(progress)`).
- Smell baseline: the four repeated A5-pin comments are intentional explicit
  scope labels, not extractable logic; no Mysterious Name, Duplicated Code,
  Feature Envy, Data Clumps, Primitive Obsession, Repeated Switches, Shotgun
  Surgery, Divergent Change, Speculative Generality, Message Chain, Middle Man
  or Refused Bequest applies. Tooling-enforced items skipped.

Standards: PASS, no findings.

## Spec (ADR0019 D7e against ADR0020 D9a)

- D7e authorizes exactly one fresh-host Ubuntu 24.04 baseline feedback
  dispatch, only after different-provider exact-head review, all five required
  gates (`verify` on three platforms, `ci-time-budget`, `agent-review`),
  coordinator merge and exact full-candidate squash-SHA verification. The
  accepted baseline (`1869175`, tree
  `a1feee1deeefe47e9a266f805e3e5128f3cc930d`, reviewed head `76c49d2`)
  is pinned as the comparison base and is never the dispatch SHA; no branch
  head or local SHA carries dispatch authority. Verified: `rev-parse
  origin/main` and `origin/main^{tree}` equal the D7e pins, and
  `rev-parse 'origin/main:<path>'` for `driver.py`, the manual workflow and
  `test_driver.py` equals the corresponding `a4-d7e-execution-baseline.json`
  blobs (107 pins). The six current head blobs equal the preparation
  record (`eb085b3`, `dbcca22`, `bb0db6f`, `143bd30`, `4779931`, `294568a`).
- Candidate scope predicate holds on all six non-doc files: only the permitted
  TEST line, the one unprivileged compile/compare workflow step, the focused
  literal/inverse assertions and the four explicit historical-A5 mock pins
  differ, plus ADR/supporting review/progress/evidence documents.
- Manual fresh-checkout helper build is required before privilege; `build.py`
  refuses uid/euid 0 before any compiler or source read, writes ignored
  candidate bytes only, compares against the existing Linux pin without
  repinning or executing. Actor/ref/SHA equality (`Dytschgo/arxcovo`,
  `Dytschgo`, `refs/heads/main`, `GITHUB_SHA == ARX_REVIEWED_SHA ==
  ARX_WORKFLOW_SHA`), read-only credentials, finally-clear with verified
  absence, and unknown/uncertain-cleanup host exclusion are all retained.
- Old D7a–D7d grants are consumed. D9a's native omitted-kill negative case is
  explicitly unrun and NOT granted; pure/model assertion mutants cannot satisfy
  it. Baseline feedback grants no A4, M2, F/G, PA6 or A7–A12 acceptance.
- Independent corroboration run: `python -B -m unittest discover -s
  tools/unix-exec-validation -p 'test_*.py'` → 109 tests, OK. Call-graph
  audited first: the discovery suite is pure/mocked (host effects mocked,
  compiler calls mocked, no helper/native/fork/proc/cgroup/systemd/sudo/owner/
  network/variable/dispatch path executed). Full `mutation_check`/F0/F0b
  mutant counts (89 audited, 36 driver kills, F0 23, F0b 36 + 4 equivalents)
  are retained coordinator/Sol evidence, not re-run here.

Spec: PASS, no findings. Remaining gates are process, not diff defects:
independent different-provider exact-head review, all five hosted gates,
coordinator merge, exact candidate tree/blob check and SHA pin.

## Security

- Workflow trust boundary unchanged: manual trigger only, exact repository,
  owner actor/triggering actor, main ref, triple-SHA equality, read-only
  credentials, fresh disposable host, bounded output/lifetime, owned-only
  cleanup. No permission, secret, owner-data, network or publication surface
  added; no MCP argument, path or reparse-point handling changed.
- Privilege order tightened, not loosened: unprivileged compile/compare lands
  before the privileged driver step; root refusal precedes all reads; the
  helper image is never executed locally. No setuid, sudoers, account,
  payment, owner-PC, static-VM or backup action is authorized.
- Failure handling stays fail-closed: one accepted dispatch consumes D7e
  regardless of outcome; failure/unknown/uncertain cleanup stops, excludes the
  host and never retries. No auto-retry, no reusable permission, no waiver of
  failed tests.

Security: PASS, no findings.

## Verdict

No blockers in the candidate. Exactly one fresh-host A4 baseline feedback
dispatch may proceed only after: different-provider independent exact-head
review, all five required gates green, coordinator merge, exact full main
squash candidate verification against the `1869175`/`a1feee1` baseline with
the six permitted non-doc diffs and no other execution-byte change, variable
set/readback/finally-clear with verified absence. Baseline and branch SHAs
are never the dispatch SHA. Native omitted-kill stays unrun and NOT granted;
A4, M2, F/G, PA6 and A7–A12 stay open. Installed/dedicated paths are
**unverified on real hardware**.
