# Conditional hosted A4 baseline source preparation

Sol / gpt-6.1-sol / high, 2026-11-15. Local source only; independent review,
all five hosted gates and coordinator merge remain pending.

Base/last merged PR #106: `40adbe55672293c894abd2d49be4de630e62577e`.
Prepared source: `5dba9dfc840539d8df390eb4bd827c8b1a31a486` (local, never dispatch authority).
PR #108 deterministic privacy fixture merge, actual full main SHA and accepted
execution-blob baseline: **pending coordinator fill**. No remote query or merge
was performed. Coordinator must integrate/verify that reviewed baseline, retain
its complete execution inventory, and compare the allowed delta before candidate
publication. The first qualifying normal gated main squash is the sole candidate;
its actual full SHA/readback/blob comparison is still pending, never this head.

Read AGENTS/KICKOFF, ADR0019 preamble/D7–D7d, all ADR0020 D9a bindings,
helper source/build pins, fixture evidence, fixed-helper/runtime/fixture/delivery
reviews, Linux comparison evidence and existing manual workflow. The existing
Linux pin is 3,680 bytes, full image SHA-256
`cee895412d58718016dd0db9883526ef8e0b13f8ed00cf213c6f77536b206369`.
The older cross-image review's 3,696-byte pin is historical; it was not restored.
No helper, fixture, production, package, Cargo, guard, permission or timeout byte
was changed. Driver delta is one TEST line; workflow delta is one non-root
compile/compare step before privilege. The builder refuses uid/euid 0 before
compiler/source reads, writes ignored output only, never executes the image or
updates a reviewed pin. Fresh manual checkout therefore gains the missing image.
D7e retains exact pins, actor/ref match, set-once/readback/dispatch-once/finally-clear
and verified variable absence. No repository variable or workflow was operated.

## Checks and limits

Callgraph audited before tests: driver imports define functions only; selected
mapping/summary/guard tests are pure; A4 delivery tests mock opens/stat/writes/
cleanup/process operations and read only fixed checkout inputs. Build-tool tests
use synthetic ELF and mocked compiler calls. No waiting helper, fork, native
adapter, proc/cgroup/systemd/sudo, owner, network or publication path was run.

- Initial instrumented `mutation_check.safe_suite()` audit: 89 tests, 24 failure
  events (subtests included), zero errors; stale literal A5 assertions and zero
  matches for the old inverse anchor. A fail-fast `a4_input` poison prevented
  newly reachable admission reads; it also caused one instrumentation-only
  failure in the direct fixed-input test. No baseline/mutation credit.
- Corrected only literal selection assertions, the inverse A5 mutant, and the
  non-A4 refusal case's explicit historical A5 selection (zero opens still
  required). Two edit-script precondition assertions stopped local edits; no
  test result or fix credit was claimed for those script failures.
- Focused 20 tests passed: A4Delivery, literal A4 selection, closed historical
  mapping, profile bytes, exact summaries, workflow guards, executable gate
  mutants and unchanged time/finally structure. Eight in-memory mutants were
  assertion-killed with zero errors: inverse A5, omit closed A4, A4 without
  Standard, source/recipe hash, image size/hash, held hash, short-write fence,
  replaced-directory identity. No native omitted-kill mutant was run.
- `python -B -m unittest discover -s tools/unix-exec-validation/a4-helper -p
  test_build.py`: 10 mocked tests passed; its candidate-written line is mocked,
  not a real helper compile. Driver SHA-256 stayed
  `c44d027c82abce854bef26ebe77554308619f1098d332179e13b48a503f41671`.
- Final 89-test audit with a fail-fast unmocked `a4_delivery` barrier scoped to
  run_body (preserving tests' own mocks): 21 failure events, zero errors, across
  10 tests. Three A1A2 tests (parent ancestor/exclusive creation and runtime
  attribution), four F0Blockers lifecycle tests, and three F0b EntryRun tests
  still assume default A5 and reach admission before their mocked lifecycle.
  An intermediate barrier also overrode one intentional admission-refusal mock
  (22 events); corrected instrumentation restored that test. These are audit
  observations, **not green full-suite evidence**. No tests were removed,
  suppressed or weakened; out-of-scope lifecycle mocks remain untouched.
- `git diff --check` passed. Full mutation_check/F0/F0b runs, native tests,
  hosted helper compile and five hosted gates were not run or credited.

**Integration blocker:** PR #108 must merge first and the full guard suite must
pass on the accepted integrated source. If its reviewed fixture changes do not
resolve the stale lifecycle mocks, a separate bounded source correction/review
is needed; this scope does not authorize it. Do not merge/dispatch this candidate
by waiving a failure or treating the focused checks as the required gates.

One accepted fresh Ubuntu 24.04 baseline dispatch would consume D7e regardless
of outcome; failed/unknown cleanup means stop, exclude host, no retry. No old
environment reuse. Baseline feedback earns no A4 acceptance: D9a's native
omitted-kill negative remains **unrun and NOT granted**, independently required.
A4, A7–A12, full F/G, PA6 and M2 remain open. Hardware paths are
**unverified on real hardware**. No owner-PC/account/admin/payment action,
push, subagent, variable change, dispatch or workflow run occurred.

## Prepared execution pins (Git blobs at source pin)

These are local preparation pins, not the missing PR #108 baseline or candidate.
All unchanged helper/fixture/package blobs equal #106. The coordinator must
re-establish full baseline/candidate pins after integration; no future SHA is
fabricated here.

| Path | Git blob |
| --- | --- |
| `tools/unix-exec-validation/driver.py` | `eb085b3e416f9bd8ca08c87c95c84fcd27cafd18` |
| `.github/workflows/unix-execution-experiment.yml` | `dbcca2276307357301719ed68a09631b8ab7a10a` |
| `tools/unix-exec-validation/test_driver.py` | `be7720b9cab0dc142cbf42ac8622a8bbf51a6a40` |
| `tools/unix-exec-validation/mutation_check.py` | `143bd308d3e4ded15ee8f25b6aa95761bab2bef1` |
| `tools/unix-exec-validation/a4-helper/helper.rs` | `5a02cf43d8abaa29e221be4ac54221e2ae05354f` |
| `tools/unix-exec-validation/a4-helper/build.py` | `5412b0cf345d3bd5dd0106e972a6b10e12827b7b` |
| `tools/unix-exec-validation/a4-helper/recipe.json` | `91ee0a9075a87447bf531d6089c08547e2767fac` |
| `tools/unix-exec-validation/a4-helper/image-pin.json` | `0554592aff7cc97e869f7f8a4dbf25b254aac640` |
| `tools/unix-exec-validation/a4-helper/linux-image-pin.json` | `43e23c1b8597eead804e78cdf69c58139a788bc0` |
| `crates/arxd/src/exec/disposable/a4.rs` | `dfa36d53f0176a8c88b52303a44502ab7f4c82b3` |
| `crates/arxd/src/exec/hierarchy/a4.rs` | `64e1ddb679ed3517e776735933dc59b2442256d6` |
| `packaging/linux/arxd.service` | `0b337773c60e49fe84de76db018d2235ae076254` |
| `packaging/linux/50-arxcovo-standard-rights.conf` | `93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74` |
