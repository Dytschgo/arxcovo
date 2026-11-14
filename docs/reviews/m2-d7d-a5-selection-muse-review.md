# Muse independent review: D7d one source-selected A5 run + selection correction

2026-11-14; Muse Spark (muse-spark-1.3, high); independent Standards/Spec/security
review sub-agent. No delegation, no native query, no product CLI, no
Win32/process/service/account/owner operation, no native filesystem probe, no
network/GitHub/CI/dispatch, no ignored-test execution, no PROGRESS edit, no
source edit, no push/PR. Git read-only CLI plus the three audited pure/mock
mutation commands below. Commit of this review file only is explicitly tasked.

## Pins (exact)

- Base (reviewed PR96 head): `71c590daabb0d7f9a1ef5d47df0aa233a0121c72`
  (`chore(branch): integrate reviewed evidence pin repair`).
- Prepared head: `1ca7fa169d8ad4879ca3a020250d3110c5a24f63`
  (`test(exec): pin proposed A5 selection and inverse mutant`); working tree
  clean at review time; HEAD == prepared head.
- Sol source: `92224c06c81dc9ab43de3886a434daeb16bef550`
  (`docs(exec): propose bounded D7d A5 run`).
- Retained: `67144539f5451946fb86579b52f7d74b139aba3c` (same message).
  `git diff 92224c0 6714453 --stat` is PROGRESS-only (+2); ADR and driver bytes
  identical, so Sol D7d+TEST content is retained through the chain.
- Coordinator correction: `1ca7fa1` (child of `6714453`).
- Scope `git diff 71c590d..1ca7fa1 --stat` is exactly five files:
  `docs/PROGRESS.md` (+4), `docs/adr/0019-unix-native-execution-bridge.md`
  (+108/−12), [driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\driver.py) (1/1),
  [mutation_check.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\mutation_check.py) (1/1),
  [test_driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\test_driver.py) (+4/−3, one hunk).
  `git diff --check` on the range is clean.
- Current selected-driver SHA-256 (observed this session, all three commands):
  `aa8423e82006ee96a42c03e4267c1bdb777ae39b8e9a4b7f7c5ea84ff59b63de`.
- Spec sources read: D7a review
  [m2-d7a-bounded-run-grok-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-d7a-bounded-run-grok-review.md),
  D7b review
  [m2-d7b-corrected-parent-grok-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-d7b-corrected-parent-grok-review.md),
  D7c review
  [m2-d7c-a3-selection-muse-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-d7c-a3-selection-muse-review.md),
  accepted plan
  [m2-stagef-a4a5-janitor-plan.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-stagef-a4a5-janitor-plan.md),
  seeder recheck
  [m2-stagef-a5-seeder-plan-muse-recheck.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-stagef-a5-seeder-plan-muse-recheck.md),
  source review
  [m2-stagef-a5-source-muse-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-stagef-a5-source-muse-review.md),
  mapping review
  [m2-stagef-a5-driver-muse-review.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\docs\reviews\m2-stagef-a5-driver-muse-review.md),
  full prepared `git diff` above.
- Not run (as tasked): builds, Linux compilation, native query, service/process/
  account/owner operation, host/root/filesystem probe, network/GitHub/CI/
  dispatch, ignored fixtures, product CLI, subagents.

## Callgraph inspection (before rerun)

- [mutation_check.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\mutation_check.py)
  imports only `hashlib/io/pathlib/types/subprocess/tempfile/unittest/
  unittest.mock` plus `driver`, `test_driver`, `test_f0_blockers`,
  `test_f0b_transport`. `safe_suite()` runs `test_driver.Guards` (minus CLI
  wrong-platform), `A1A2`, `CgroupScan`, `F0Blockers`, and `test_f0b_transport`
  suite — mocked/in-memory. `rust_mutants()` compiles stdin slices of the real
  pure ports and excludes native adapters; no source file is mutated or
  written. `test_f0_blockers.py --mutants` and `test_f0b_transport.py --mutants`
  use in-memory source replacement with the same no-write assertion. This
  matches the prior source-review audit, so only these documented pure/mock
  commands were rerun.

## Checks actually rerun (audited pure/mock only)

- `python -B tools/unix-exec-validation/mutation_check.py`: `Ran 76 tests`,
  `OK`; 29 `KILLED` Python assertion lines; `Rust pure baseline: 3 passed`;
  4 `KILLED` compiled Rust lines; `source unchanged
  sha256=aa8423e82006ee96a42c03e4267c1bdb777ae39b8e9a4b7f7c5ea84ff59b63de`;
  `Rust source fingerprints unchanged`.
- `python -B tools/unix-exec-validation/test_f0_blockers.py --mutants`:
  23 `KILLED` lines; `source unchanged sha256=aa8423e8...63de`.
- `python -B tools/unix-exec-validation/test_f0b_transport.py --mutants`:
  36 `KILLED` + 4 `EQUIVALENT` (`writer_site_unchecked`,
  `writer_drops_nofollow`, `reader_ascii_dropped`, `reader_mode_mismatch`,
  each with same-enforcement reason); `source unchanged sha256=aa8423e8...63de`.
- Counts corroborate the ADR corrected-validation paragraph exactly (76 / 29 /
  3 pure / 4 compiled / F0 23 / F0b 36+4). The failed 75/76 preparation run is
  correctly recorded as earning no credit; the corrected checks supersede it
  and grant no execution permission. No native operation or dispatch ran.

## Standards

Pass. One-concern policy+test preparation delta with proportionate fences; no
optional infrastructure.

- [driver.py:13](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\driver.py:13)
  changes only the selected TEST constant to
  `exec::disposable::disposable_linux_a5`. `standard_profile()` still validates
  TEST against exactly the four closed `exec::disposable::` literals before any
  effect. No profile switch, test/unit/drop-in argument, environment selector,
  new CLI mode, or workflow input.
- [test_driver.py:801-802](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\test_driver.py:801)
  renames one assertion to `test_source_selection_is_a5` and pins A5 literally.
  The four-case `test_closed_mapping`, `test_all_four_exact_summaries`,
  `test_exact_summary`, `test_profile_pairings_and_install_bytes`,
  `test_launch_contract`, and `test_closed_selection_before_effects` bodies are
  byte-identical to base, so no profile/argv/summary coverage is weakened; the
  rerun kills above confirm the A5 mapping, summary, and attribution mutants
  still die on the unchanged tests.
- [mutation_check.py:16](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-b253775a-0ec9a99916aca8df\tools\unix-exec-validation\mutation_check.py:16)
  inverts one anchor from a dead select-A5 mutant (zero source matches) to a
  restore-historical-None mutant with exactly one source match, killed by
  assertion in the renamed test. The old anchor could never kill and correctly
  earned no credit.
- Docs: ADR + 2-line PROGRESS status only. No workflow, Cargo, package,
  production, evidence, or hash-byte change. `git diff --check` clean; no
  unicode corruption, no out-of-scope file.
- Smell baseline: no Mysterious Name, Duplicated Code, Feature Envy, Data
  Clumps, Primitive Obsession, Repeated Switches, Shotgun Surgery, Divergent
  Change, Speculative Generality, Message Chains, Middle Man, or Refused
  Bequest worth a change. The rename+inverse-mutant is the shared-shape
  extraction the baseline asks for, not new duplication.

## Spec

Pass. The delegated D7d policy is present, internally consistent, and its
corrected candidate predicate actually fits the five-file diff.

- Preamble/D7/D7a/D7b/D7c agree: D7a, D7b, and D7c are marked consumed with
  their recorded hosted runs; D7d proposes one fresh-host A5 run with no run
  permission before this independent Muse exact-head review, all five required
  gates (three platform `verify`, `ci-time-budget`, `agent-review`), and
  coordinator merge. No A4/M2F/GPA6 credit is taken; all other native execution
  and historical retries stay unauthorized.
- PR96 condition is conditional, not fabricated: the ADR names reviewed head
  `71c590d` and refresh `6884475968` as the execution baseline, requires the
  coordinator to verify the eventual PR96 main squash is tree-equal to it, and
  to pin the actual full candidate main squash SHA before publication. No main
  squash SHA is assumed anywhere in the diff.
- Candidate predicate matches the diff exactly: one TEST line in `driver.py`,
  the focused selection assertion plus its counterfactual in `test_driver.py`
  and `mutation_check.py`, ADR plus supporting review/progress documents. All
  other bytes unchanged; no stale hash restoration or guard weakening. The
  assertion pins A5 literally, the mutant reverts to historical None, and only
  assertion-failure credit counts — all three hold in the code and reruns.
- One accepted dispatch, consumed regardless of outcome, with actor/ref/SHA
  equality and a finally-style variable clear plus HTTP 404 absence check. No
  selector input or framework is added; existing guards stay unchanged.

## Security

No blocker. The only new accepted literal derives Standard through the
existing closed-allowlist rule, so an A5 host still requires the unchanged
packaged Standard drop-in and a None-profile host stays ineligible. Rejection
precedes every effect, so no arbitrary disposable name, A4, cross-account, or
variant input can reach provisioning, cgroup, account, or service paths. Exact
runtime-attribution, sole-supervisor-PID, collision, reservation, and
owned-cleanup fences extend to A5 unchanged. No secret, owner, approval-flow,
or publish change. Reversibility is a two-way door: reverting the commits
restores the A3-None selection wording with no native residue.

## Verdict

- **Blockers:** none. Minimal actionable item: none beyond the gates the ADR
  already requires.
- **Publication:** ready for the ordinary gates only: the three platform
  `verify` checks, `ci-time-budget`, `agent-review`, and coordinator merge.
  This file is the different-provider independent review of the D7d
  preparation. Check results beyond the reruns above are not claimed. This
  review does not merge and grants no run permission.
- **Run permission:** withheld. Set `UNIX_EXECUTION_REVIEWED_SHA` only after
  those gates, and only to the verified full main squash SHA. Dispatch the
  existing workflow once, then clear the variable and verify it is absent.
- **Standards:** 0 findings.
- **Spec:** 0 findings.
- **Security:** 0 blockers.

Standards 0. Spec 0. Security 0. Worst issue: none. M2 A4/A5, remaining
Stage F/G, and real-Runner PA6 stay open, **unverified on real hardware**.
Linux-only fixture compilation/execution remain unverified locally. This commit
adds only this review; no PROGRESS, source, push, or PR action taken.
