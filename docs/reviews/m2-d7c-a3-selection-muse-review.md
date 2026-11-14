# Muse independent review: D7c two fixed source-selected A3 cases

2026-11-14; Muse Spark (muse-spark-1.3, high); independent Standards/Spec/security
review sub-agent. No delegation, no native query, no product CLI, no
Win32/process/service/account/owner operation, no native filesystem probe, no
network/GitHub/CI/dispatch, no ignored-test execution, no PROGRESS edit, no
source edit, no push/PR. Git CLI read-only only. Commit of this review file
only is explicitly tasked; no other commit/push/PR.

## Pins (exact)

- Base (actual main): `fba8b3c993b3d173125525c760ee2dc7234e3cfa`
  (`fba8b3c test(exec): prepare closed A3 driver selections (#90)`).
- Head (candidate): `231732cf44b5d3b1a9a4a9fdde5fafa60c3407fa`
  (`231732c docs(adr): retain UTF-8 and corrected selection validation`).
- Branch: `t3/review-d7c-a3-selection`; HEAD == candidate; working tree clean.
- Sol source retained: `0c09bb48c78d984afe5bfcde2e4db171b798cda9`
  (`0c09bb4 docs(exec): propose bounded A3 execution disposition`).
  `git diff 0c09bb4 3a7bebe --stat` is PROGRESS-only (+2); Sol D7c+TEST content
  is retained through the chain.
- Intermediates: `3a7bebe4e667eaf6a57dc2c92765f0fb7d15169e` (Sol D7c proposal),
  `251a38fdddc3ed2a4e8eccebc75d62fe2c880b7b` (coordinator test corrections),
  `231732c` (UTF-8 retention + corrected validation record).
- Scope diff `git diff fba8b3c..231732c --stat` is exactly five files:
  `docs/PROGRESS.md` (+4), `docs/adr/0019-unix-native-execution-bridge.md`
  (+110/−13), `tools/unix-exec-validation/driver.py` (1/1),
  `tools/unix-exec-validation/mutation_check.py` (1/1),
  `tools/unix-exec-validation/test_driver.py` (+8/−3).
- No workflow/package/production/runtime-guard change except the one selected
  TEST line. `git diff --check` on the range is clean.
- Current selected-driver SHA-256 (observed this session):
  `88cc51ddf0b229e45f6c00f79ed03bcca978ee35d3b4e01306c1b210f5b1a028`.
- Spec sources read: accepted plan `docs/reviews/m2-stagef-a3-driver-plan-muse-review.md`,
  source reviews `docs/reviews/m2-stagef-a3-source-muse-review.md` and
  `docs/reviews/m2-stagef-a3-driver-source-muse-review.md`,
  `docs/reviews/m2-d7a-bounded-run-grok-review.md`,
  `docs/reviews/m2-d7b-corrected-parent-grok-review.md`,
  `docs/reviews/m2-a1a2-hosted-evidence-muse-review.md` (cited by D7b),
  `docs/KICKOFF.md` (v1 scope), full candidate `git diff` above.
- Not run (as tasked): builds beyond the audited pure/mock commands below,
  Linux compilation, native query, service/process/account/owner operation,
  host/root/filesystem probe, network/GitHub/CI/dispatch, ignored fixtures,
  product CLI, subagents.

## Callgraph inspection (before rerun)

- `tools/unix-exec-validation/mutation_check.py`: imports only
  `hashlib/io/pathlib/types/subprocess/tempfile/unittest/unittest.mock` plus
  `driver`, `test_driver`, `test_f0_blockers`, `test_f0b_transport`.
  `safe_suite()` runs `test_driver.Guards` (minus CLI wrong-platform),
  `A1A2`, `CgroupScan`, `F0Blockers`, and `test_f0b_transport.suite()` —
  mocked/in-memory. `rust_mutants()` compiles stdin slices of the real pure
  ports (`parse.rs`/`setup.rs`/`fake.rs`/`development/types.rs`) and excludes
  native adapters; no source file is mutated or written. This matches the
  prior source-review audit, so only these documented pure/mock commands were
  rerun.

## Checks actually rerun (audited pure/mock only)

- `python -B tools/unix-exec-validation/mutation_check.py`: `Ran 72 tests`,
  `OK`; 23 `KILLED` Python assertion lines; `Rust pure baseline: 3 passed`;
  4 `KILLED` compiled Rust lines; `source unchanged
  sha256=88cc51dd...a028`; `Rust source fingerprints unchanged`.
- `python -B tools/unix-exec-validation/test_f0_blockers.py --mutants`:
  23 `KILLED` lines; `source unchanged sha256=88cc51dd...a028`.
- `python -B tools/unix-exec-validation/test_f0b_transport.py --mutants`:
  36 `KILLED` + 4 `EQUIVALENT` (writer_site_unchecked, writer_drops_nofollow,
  reader_ascii_dropped, reader_mode_mismatch, each with same-enforcement
  reason); `source unchanged sha256=88cc51dd...a028`.
- Counts corroborate the ADR corrected-validation paragraph exactly (72 / 23 /
  3 pure / 4 compiled / F0 23 / F0b 36+4). The failed 71/72 preparation run is
  correctly recorded as earning no credit; the corrected checks supersede it
  and grant no execution permission. No native operation or dispatch ran.
- Encoding/scope: current ADR blob decodes as UTF-8, 41434 bytes, CR 0, no
  BOM. The intermediate wrong-Windows-encoding ADR edit is not present in the
  final diff; content was restored from committed UTF-8 before the scoped
  correction. Final diff has no unicode corruption and no out-of-scope file.

## Standards

Pass. One-concern policy+test preparation delta with proportionate fences; no
optional infrastructure.

- `driver.py:13` changes only the selected TEST constant to
  `exec::disposable::disposable_linux_a3_standard`. `standard_profile()`
  still validates TEST against exactly the three closed
  `exec::disposable::` paths before any effect. No profile switch, test/unit/
  drop-in argument, environment selector, new CLI mode, or workflow input.
- `test_driver.py` replaces one hard-coded A1/A2 argv assertion with a loop
  over the three literal accepted selections (`a1_a2`, `a3_standard`,
  `a3_none`), each patched as `driver.TEST` and asserting the exact argv.
  This removes duplication rather than adding it; no production or
  guard-test weakening.
- `mutation_check.py` `wrong test` anchor changes from a dead source-constant
  swap (`TEST = a1_a2` → `TEST = a3_standard`, zero source matches) to an
  argv-level excluded cross-account mutant
  (`return [binary, TEST, ...]` → `return [binary,
  '..._cross_account', ...]`). The anchor now has exactly one source match and
  is assertion-killed by the corrected launch-contract test; the old anchor
  could never kill and correctly earned no credit. Neither correction accepts
  an arbitrary selection.
- Docs: ADR + 2-line PROGRESS status only. No workflow, Cargo, package,
  production, evidence, or hash-byte change. `git diff --check` clean; UTF-8,
  no CR/BOM.
- Smell baseline: no Mysterious Name, Duplicated Code, Feature Envy, Data
  Clumps, Primitive Obsession, Repeated Switches, Shotgun Surgery, Divergent
  Change, Speculative Generality, Message Chains, Middle Man, or Refused
  Bequest worth a change. The loop and argv-mutant are the shared-shape
  extraction the baseline asks for.

## Spec

Pass. The delegated D7c policy is present, internally consistent, and its
Standard candidate predicate actually fits the five-file diff.

- Preamble/D7/D7a/D7b/D7c agree: D7a and D7b are marked consumed; A1/A2
  acceptance is limited to recorded hosted run `43627472312`; D7c proposes
  two fixed fresh-host A3 cases in order with no run permission before this
  independent Muse exact-head review, all five required gates (including
  `agent-review` and `ci-time-budget`), and coordinator merge. The historical
  `E_PROTECTION` diagnosis stays unresolved with its environment excluded.
  D7a header and the checkpoint section both say consumed; only D7c proposes.
- D7b history is accurate: PR #86 merged as
  `1e9d8818de1e337b1402ca7e2b71ac02456b82f3`, accepted dispatch `43627472312`
  attempt 1 consumed the grant, evidence/cleanup accepted in the cited Muse
  evidence review, SHA variable verified absent. D7c does not renew it or reuse
  its environment.
- Standard predicate fits: sole candidate is the first main squash containing
  D7c and `TEST = '..._a3_standard'`; versus full PR #90 main SHA
  `fba8b3c...`, only that one TEST line, the two reviewed selection-regression
  corrections, this ADR, and supporting review/progress documents may differ.
  The observed diff is exactly that set. No other runtime byte may change;
  packaged standard profile must be present.
- None predicate is correctly deferred: eligible only after Standard success
  is independently verified/accepted with confirmed owned cleanup and verified
  variable absence; sole candidate is the first subsequent reviewed,
  five-gates-green, merged main squash changing only that TEST line to
  `..._a3_none` plus review/progress docs versus the pinned Standard squash.
  None omits the whole profile; no weakening drop-in allowed. D7c covers it
  without a further amendment.
- Fresh-host/disptach/finally/stop semantics all present: unchanged manual
  `unix-execution-experiment.yml` once per case on a separate fresh
  GitHub-hosted Ubuntu 24.04 invocation; at most one accepted dispatch per
  case consumed regardless of outcome; exact `Dytschgo/arxcovo`, actor +
  triggering actor `Dytschgo`, `refs/heads/main`, and
  `GITHUB_SHA == ARX_REVIEWED_SHA == ARX_WORKFLOW_SHA` preserved; read-only
  credentials and workflow fences remain; no workflow/CLI input, selector, or
  framework introduced. Finally-style clear after completed/failed run or
  rejected dispatch with absence verification. Stop both cases on any failure,
  rejected dispatch, unknown result, or uncertain cleanup; exclude the
  environment with no automatic retry; None ineligible on any such Standard
  outcome. Historical `43091339273` and all consumed D7a/D7b environments stay
  excluded.
- Historical evidence/hash bytes unchanged: the diff touches no evidence,
  workflow, Cargo, package, production, or fixture file. Guard-test failure
  cannot be waived to fit scope.
- Acceptance boundaries honest: only the observed A3 case gets credit; M2,
  full Stage F, Stage G, real-Runner PA6 stay open; installed/dedicated paths
  **unverified on real hardware**; no account provisioning, cross-account
  probe (the mutant value is an excluded argv string, never executed),
  owner-PC access, owner admin, production activation, static VM, or backup
  action.

## Security

Pass. No new privileged path, secret, or trust claim.

- Fixed absolute runtime paths, existing fences (exact package/runtime
  attribution, sole MainPID/supervisor checks, protected staging, shared
  reservation, collisions, bounded output/lifetime, owned-only cleanup)
  retained verbatim. Profile presence still derives from the selected TEST;
  absence is by file omission, never a weaker file; no NNP/capability
  override. Closed codes unchanged. Technical fences only; no approval flow.
  Optional diagnostics stay parked.

## Findings and verdict

- Blockers: none.
- Publication: ready for the ordinary gates only (three platform `verify`
  checks, `ci-time-budget`, `agent-review`, coordinator merge) as a
  docs-plus-closed-test selection. Check results are not claimed here. This
  review does not merge.
- Run permission: withheld. Set `UNIX_EXECUTION_REVIEWED_SHA` only after those
  gates and only to the verified full main squash SHA; dispatch the unchanged
  workflow once per eligible case, then clear in the finally path and verify
  absence. This review does not authorize any dispatch.
- Standards: 0 findings. Spec: 0 findings. Security: 0 findings. Worst issue:
  none; the unrun-Linux note is already stated honestly as **unverified on
  real hardware**.

Standards 0. Spec 0. Security 0. Worst issue: none.
