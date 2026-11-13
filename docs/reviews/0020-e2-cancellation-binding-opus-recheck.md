# ADR 0020 E2 B1/B2 narrow recheck (Opus high)

Reviewer: Anthropic / claude-opus-5-5 / high, sole review sub-agent,
2026-11-13. No delegates. This is the independent narrow recheck of the Sol
medium (OpenAI) tests-only correction for blockers B1 and B2 from review
`dd4dad8` (retained at `3bc285e` in
`docs/reviews/0020-e2-independent-source-review.md`).

## Verdicts

| Axis | Verdict |
| --- | --- |
| Standards | **Pass.** No new finding. Earlier advisories SA1-SA4 stay open and disclosed. They were not reopened. |
| Spec | **Pass. B1 and B2 are closed.** Each new assertion discriminates its target mutant, and production is unchanged. Advisories A1-A8 stay as disclosed. |

No blockers. Nothing in this delta needs another correction.

## Target identity and isolation

- Checkout: a separate T3 worktree
  (`.../.t3/worktrees/Arxcovo/t3-1d69db76-...`), outside canonical
  `D:/Code/Arxcovo`. `pwd` and `git rev-parse --show-toplevel` matched. Status
  was clean. HEAD was `9bc4797266777cf381974a2caec2c10ede34ab12`.
- Correction `70729f6` touches only `crates/arxd/src/exec/hierarchy/fake.rs`
  (+10 lines). Evidence `9bc4797` touches only `docs/PROGRESS.md` and
  `docs/evidence/linux-hierarchy-e2.md`.
- `git diff b40e81a HEAD -- . ':!docs'` contains only that `fake.rs` change.
  The `setup.rs` blob is `b33bed7c` at both `b40e81a` and HEAD, and its
  SHA-256 is `ab04270e...`. This means production is byte-identical to the
  reviewed original.
- Manifest check: the HEAD `fake.rs` blob is
  `0de9e4e95ee4101f195b06e5115203035e7ac078`. Its SHA-256 is
  `69838972dbd426c3c227ba0613a7054a609d0b8dc34c912780f52d60043ed90f` in the
  worktree, in the `git cat-file` output and in the scratch extract. Both
  values match the evidence manifest.
- `064113a` is an ancestor of HEAD, and its tree is `a3bec9bc...`.

Read: AGENTS.md, the review skill's two-axis rule, the ADR 0020 D3 shutdown
rule ("inside every wait"), the brief's H10 and H14 clauses, the prior review,
the new evidence section, the `setup.rs` driver body, the fake helpers
(`Fake::new`, `run`, `fail`, `no_mutation`, the `kill_delay` model in `read`
and the `Manual` clock) and `parse::locate`.

## Spec: blocker closure

### B1 closed: in-wait cancellation is now the deciding check

- Change: `h14_shutdown_boundaries` now sets `kill_delay = None` before the
  kill-wait case, so the subtree kill never completes.
- Why it discriminates: the token is cancelled only by `Manual::sleep`, and
  only the wait loop calls that. After the first poll, `runs` stays populated.
  The only check that can then return `Stopped` is the in-loop
  `stopped(token)?`. The post-loop check is now unreachable while the kill is
  pending.
- If the in-loop check is removed, the loop keeps polling until the 5 s
  logical deadline and returns `KillTimeout` (`Janitor`) instead of
  `Stopped`. The 5 s bound in `Manual` is not hit first. My mutant confirmed
  this: `left: Unavailable(KillTimeout)` / `right: Unavailable(Stopped)` at
  fake.rs:796.
- `no_removal()` still pins "no `rmdir`".
- The prior review also suggested asserting a single clock step. Sol did not
  add it. It is not needed to tell the mutant apart, because the outcome
  already does. I do not raise it as a finding.

### B2 closed: the walked unit is bound to the process's own cgroup

- New test: `h10_valid_foreign_location_refused_before_mutation`. The fake's
  `proc_cgroup` is set to `0::/other.slice/arxd.service/supervisor` and
  compared with the requested `/arxd.service/supervisor`.
  - It first asserts `parse::locate(bytes).is_some()`. The refusal therefore
    comes from the location comparison and not from parsing.
  - It then requires `Locate` and zero `write`/`mkdir` (`no_mutation`).
- Sol used `other.slice` where the review used `system.slice`. The two are
  equivalent: the parser accepts both, and both differ from the request.
- If the equality is removed, the walk follows the requested unit, which
  exists in the fake, and setup returns `Ready` (writes performed). My mutant
  confirmed this: `left: Ready(..)` / `right: Unavailable(Locate)` at
  fake.rs:184.
- I also tried a weaker probe: compare only the last unit component.
  `arxd.service` matches on both sides, so the check passes. The test killed
  this probe too, because the walk still reaches `Ready`. The case therefore
  covers more than the deleted-equality mutant.

## Command credits

All runs were mine, on Windows x86_64-pc-windows-msvc (cargo 1.95.0, nextest
0.9.151). They used an owned scratch extract (`git archive HEAD`) with a
scratch `CARGO_TARGET_DIR`. The worktree was never mutated by a build.

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed
  (52 s).
- `cargo nextest run --locked -p arxd --no-fail-fast -E 'test(exec::hierarchy::fake)'`:
  - **24/24 passed** before the mutants (run `955d08a6-f255-408d-9041-3cd78ca48c63`);
  - **24/24 passed** after restore (run `ad15eda7-e03b-45f0-b296-4eb7b94d7c6a`).
- Mutants: one `setup.rs` edit per row. After each row the file was restored
  from a saved copy and its SHA-256 was checked as `ab04270e...`. A final
  `cmp` against the original confirmed a byte-exact restore.

| Mutant | Exit | Result | Run ID |
| --- | --- | --- | --- |
| A: in-loop `stopped(token)?` removed | 100 | killed by h14 only (`KillTimeout` vs `Stopped`) | `b7482348-0433-43f3-a830-6b5b8130dcf5` |
| B: location equality replaced by `true` | 100 | killed by the new h10 case only (`Ready` vs `Locate`) | `6f9290b7-20de-48fd-8e49-0a83bf0727ac` |
| B': compare last unit component only (probe) | 100 | killed by the new h10 case only | `148534b0-6117-43f5-aa56-5ce724600bc4` |

- Builder credits accepted as reported and **not re-executed by me**:
  - fmt and Clippy;
  - the three targeted tests (`eaabfd60`);
  - the final 24/24 (`b23eb3e4-1197-43ad-9641-aade301bef27`);
  - the two mutant runs (`640ea4e8-...` and `67492d14-...`, both exit 100).

  My independent runs agree with all of them.
- I ran no full suite, because this delta changes tests only. The single full
  360 passed / 3 skipped run (`c3e5869b-...`) stays credited at `b40e81a`
  only. No full run is claimed at HEAD.

## Limitations

- This rechecks a fake port only. Every Linux/native path, including real
  descriptor behavior, created mode (A1) and hosted Linux/macOS compile and
  runtime, is **unverified on real hardware**. No Linux runtime claim is made.
- What I did not do:
  - I made no native, filesystem-adapter, `/proc`, `/sys`, root, cgroup,
    account, service, owner, VM, private-state, experiment or ignored-fixture
    call.
  - The only child process was the purpose-built H15 self re-execution, run
    by the normal suite.
  - I took no GitHub, push, PR, CI, watch, merge, retry or settings action.
  - I edited no `PROGRESS` or product file.
- M2 is incomplete. `E_PROTECTION`/`ExecutionFailed` remain unknown, with no
  retry.
- This PASS does not authorize publication. Publication still needs an
  exact-head integration review and green hosted required gates on the
  current main (`064113a` lineage).

**Summary:** Standards and Spec both pass. B1 and B2 are closed. Each new test
alone kills its compiled production mutant, and production `setup.rs` is
byte-identical to `b40e81a`.
