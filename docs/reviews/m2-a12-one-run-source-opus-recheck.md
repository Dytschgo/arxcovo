# A12 one-run source/content recheck (Opus)

2026-11-15. Anthropic Claude Opus 5.5 / high. This is a fresh, independent delta
recheck by a review sub-agent (Opus reviewing Sol), with no subagents. It is
read-only apart from this file. No push, PR, comment, status, merge, watch,
dispatch, variable, native, host/root/account/sudo, Runner or LinuxFs action was
taken.

**Verdict: S1 CLOSED. Standards PASS, Spec PASS, Security PASS. No new findings.**
The candidate content is eligible to enter the normal ADR0019 D7l chain. That
chain is unchanged and still gates any one-shot.

## Pinned target

- Prior review: `docs/reviews/m2-a12-one-run-source-opus-review.md` (original
  `e53d60b`, retained as `cadddd4`). It reviewed head `8e42768` and found one
  blocker, S1.
- Correction: `d76acba63901ffed70ee613820b89d480e287029` (Sol, on top of `cadddd4`,
  work/m2-a12-one-run-candidate). It changes:
  - `tools/unix-exec-validation/mutation_check.py`: one line, the `CASES[0]`
    tuple only.
  - `docs/evidence/linux-a12-one-run-preparation.md`: the mutation_check
    inventory row, the candidate digest and a new "Opus S1 correction" note.
  - `docs/PROGRESS.md`: one coordinator entry.
- `cadddd4` adds only the review document. Against `8e42768`, the only non-doc
  path that differs at `d76acba` is `mutation_check.py`. `driver.py`, `a12.rs`,
  `test_driver.py` and `tools/ci/a7-admission-readonly.py` have identical blobs.
  `git diff --check 8e42768 d76acba` passes.

## S1 closure

- The new tuple is `("\nTEST = 'exec::a12::native::disposable_linux_a12'"` →
  `"\nTEST = 'exec::disposable::a4::disposable_linux_a4'"`,
  `test_source_selection_is_a12_candidate`). It matches the minimal fix the prior
  review proposed.
- The leading newline is load-bearing. In the driver bytes, the bare
  `TEST = 'exec::a12::native::disposable_linux_a12'` occurs 2 times, because it
  is a substring of the `A12_TEST = …` line. The newline-anchored form occurs
  exactly once, and `main()` asserts `source.count(before) == 1`. The
  replacement also starts with `\n`, so the line structure is preserved. In
  memory, the mutant source has `TEST = 'exec::disposable::a4::disposable_linux_a4'`
  and an untouched `A12_TEST = 'exec::a12::native::disposable_linux_a12'`.
- This reviewer ran `mutation_check.main()` at `d76acba`:
  - The 91 audited baseline tests pass.
  - All 40 historical driver mutants are KILLED by assertion failures, with no
    errors. Case 1 is killed by `A1A2.test_source_selection_is_a12_candidate`.
  - The driver source is unchanged at sha256
    `24dd0d6a2c8948f2092e54fff40bbb03c4364ab2a07e59c6e12d62517e5fe47f`.

  This agrees with the coordinator's report.

## Inventory (recomputed from committed Git blobs)

Canonical input: non-doc tree entries, sorted by path bytes, each
`mode TAB blob TAB path LF`.

| Rev | Entries | SHA256 |
| --- | --- | --- |
| base `fb23b58` | 223 | `84f8446e059f5e64a1aa7957763a7bb171c43de967410dcd23811ddaba2b9d05` (matches record) |
| prior `8e42768` | 223 | `615c780a5c4ff0f58fc0f4ee4a4c171ca7e9a16bd8cde715a5546513a1f9af5d` (superseded) |
| `d76acba` | 223 | `dbc11a5ad3a0e37d6bf659b1e1bac65c2758565d8a11cf5c500bb415e375c710` (matches record) |

- Every entry is a 100644 blob. There are no symlinks, submodules or mode changes.
- The record's 223 table rows match the `d76acba` non-doc tree exactly:
  - The path sets are equal.
  - Mode, blob, SHA256 of the committed blob bytes and size all match on every
    row. There are 0 mismatches.
- mutation_check row: blob `8e3074cebd3ac8fdb2effa28e334e2eeca99c7fe`, SHA256
  `752a0570f53f4275370318561bc16790684e075f9bba4cffdaa2d8eda0cd5c89`, 58814 bytes.
- driver row: unchanged at blob `48e4fe6e…`, SHA256 `24dd0d6a…5fe47f`, 96767
  bytes. The wrapper's consumed MODULE pins still equal these values, and the
  wrapper blob is unchanged.

## Verdicts

- **Standards: PASS.** S1 is closed. The correction is a one-line test-harness
  fix with a meaningful kill, and the evidence row and digest are accurate. The
  PROGRESS entry is factual and labels A12 **unverified on real hardware**.
  Advisories S2–S4 are unchanged and non-blocking. As instructed, there was no
  advisory churn.
- **Spec: PASS.** No source, selection, gate, test or workflow content changed
  since the prior review. Its Spec analysis carries over unchanged, including
  the four-selection scope, ordinary-test isolation, the counterfactuals and the
  narrow D7l binding. SP1–SP2 remain advisory.
- **Security: PASS.** There is no new host, network, privilege or secret
  surface. All gates stay fail-closed. SEC1 (attempt 1 is enforced by procedure)
  remains advisory: the coordinator must not rerun, and the post-run assessment
  must reject any attempt other than 1.

## Eligibility (unchanged D7l chain)

This recheck satisfies only the fresh-Opus-content-review prerequisite. Before
any one-shot, the normal chain still applies:

1. Exact-head independent review of the PR head.
2. Five green required checks.
3. Coordinator merge as the first eligible normal squash.
4. Verification that the squash tree and the 223-row inventory equal this
   content: digest `dbc11a5a…5c710`, under the strict path/mode predicate.

Further constraints:

- Any later byte change to a non-doc path invalidates this recheck's inventory
  binding.
- The A7/A11 hosted FINAL NO-GO and the consumed D7a–k grants stay closed.
- A12 is unrun and **unverified on real hardware**. It has no acceptance credit.

## Checks

Run by this reviewer at `d76acba` (Windows, TEMP/TMP on D:, pure Python and
mock only, no native calls, no builds):

- `mutation_check.main()`: 91 audited tests OK. 40/40 killed by assertion. Driver
  sha256 unchanged.
- `mutation_check.a12_assertion_mutants()`: 9/9 killed.
- `python -I -B -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`:
  196 OK.
- Full recomputation of the inventory, digests and 223 rows for `fb23b58`,
  `8e42768` and `d76acba`.
- Anchor occurrence counts (bare 2, newline-anchored 1) and in-memory mutant
  line inspection.
- `git diff --check 8e42768 d76acba`.
- Blob identity of driver, wrapper, a12.rs and test_driver between `8e42768` and
  `d76acba`.

Reused from the prior review without rerunning, honestly, because those inputs
are blob-identical:

- Rust `exec::a12::` 7 passed.
- Residue guard and w8_e7.
- `cargo fmt --check`.
- `test-a7-admission-readonly.py` 13 OK.
- Other mutation entry points, per-site gate mutants and source-guard
  simulations.

Builder-reused items remain as stated there. Not run by anyone: the full Rust
suite, the Linux compile and the native A12 fixture.
