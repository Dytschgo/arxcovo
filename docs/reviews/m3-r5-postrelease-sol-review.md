# R5 post-release observation: independent Sol review

2026-11-16; OpenAI gpt-6.1-sol, high. Independent review of the Opus
implementation, performed directly by this reviewer without children. Source
verdict: **PASS** on Standards, Spec and Security. This is diagnostic-source
review, not merge permission or evidence that the Windows failure is fixed.

## Target and scope

- Reviewed head: `f0b941807a6cecfd3149db01e131f00031ae76d4`.
- Prepared base: `c761aed48731b0be40e51e107d335df65efbe8dd` (also merge base).
- Retained main: `c6121523417089bc0afaf307470183dd4a7f47ba`.
- Builder source: `c49f297d89dc5aee03a455db019291622fbf25e5`.
- `git diff c761aed f0b9418` has exactly six paths: `.config/nextest.toml`,
  `consensus.rs`, `consensus/three_node.rs`, the existing quarantine guard,
  `docs/PROGRESS.md` and the observation evidence document. Read the entire
  diff, enclosing read/shutdown/R5/gate implementations and sampler selection.
  Read AGENTS, KICKOFF, CONTRIBUTING, the dytschgo-review skill/profile, and the
  originating observation brief and evidence.
- `git diff c49f297 f0b9418` changes only PROGRESS and the evidence document.
  All four implementation blobs match the builder's recorded pins exactly:
  `e635e61c8ccffa4152b13c87163e7476b0f04718` (consensus),
  `ecc78ce9de34d58108ec9b0946f216fdbd13f12d` (three_node),
  `d97d8b6643f42135b5a6007af7a1c15337724244` (config), and
  `1e6e74a5671f17cc2df6e683f3c825669f2649d7` (guard).
  Commits reviewed: df878ab, 847b352, fbc929b, c49f297, f0b9418.

## Standards

**PASS; zero findings.** Scope is test diagnostics and output visibility.
Production read/shutdown work, Cargo files, workflows, retries, membership,
scheduling and test selection are unchanged. Per-node cfg(test) OnceLock storage
avoids a cross-fixture slot. The first eligible blocking closure claims the
stamps; five Instant OnceLocks preserve first observations. The new handle
contains no store, permit or ticket. The original guard/store/permit/completion
order remains intact. The fixed schema and small typed records have no material
Fowler-baseline smell requiring a change.

The report assertions, five-second shutdown budget, 5.5-second assertion and
combined two-second cleanup predicate/unwrap semantics are preserved. Binding
the wait result permits output before its original failure is surfaced. The
quarantine guard retains exactly eight Windows ignore gates and required
issuance coverage. PROGRESS retains red main, held PR159, incomplete milestones
and hardware limits. No architectural deviation or new approval flow is added.

## Spec

**PASS; zero findings.** The armed R5 iteration records both raw snapshots,
release flags, read permits and read_blocking counts. The observer uses only
Weak strong_count, the semaphore and release flags; it never upgrades stores.
Sampling sleeps one millisecond. Normal finish stops and joins before printing
and unwrapping; Drop stops and joins on unwinding. A sampler panic is emitted as
a fixed observer state, then resumed. The double-panic cleanup case is handled
by discarding the join error during an existing unwind. No raw error is printed
by the new line. Forced sampler-panic/unwind behavior was reviewed in source,
not exercised by this reviewer.

Stage positions truthfully describe closure entry, lock acquisition, the return
of the read/guard scope, return from drop(sm), and return from drop(permit).
`pre`, `none` and `ovf` distinguish before-anchor, missing and overflow values.
The fixed ASCII renderer caps the line at 1024 bytes; its inputs cannot contain
paths, errors, IDs, addresses or free text. Remaining strong counts map to
`unknown`, including log-only ownership. There is no sampled-field subtraction
or detached-engine attribution.

Timing limits: the open snapshot is actually taken immediately BEFORE
`gate.open()`, after the anchor; gate.open also joins the existing gate thread.
The end snapshot is taken after the wait; stage marks are read after sampler
join. These are sequential observations, not an atomic snapshot or a proof of
exact release time. A missing first-clear sample can coexist with a clear end
snapshot. `store_holder=none` means zero observed strong counts, not completed
destruction; release flags and stage marks retain that distinction.

The single default-profile nextest override selects exact arxd/lib/R5 and changes
only success-output to immediate. The diagnostic sampler passes its own explicit
config and diag-sample profile, with run-ignored only and its unchanged exact-eight
filter. The new default override does not alter that selection. Byte-exact and
structural guard checks reject all 18 embedded configuration mutants.

## Security

**PASS; zero findings.** Test-only numeric/enum output adds no external input,
filesystem access, raw path/error output or store ownership. Released clones
contain only atomic flags. No MCP contract, owner/admin operation, VM, old Lab
Manager resource or public publishing behavior is changed. Public actions were
outside this review and none were performed.

## Actual reviewer verification

On the exact reviewed source, all commands below passed:

- `python tools/ci/test_windows_cluster_quarantine.py`: 3 tests, including
  whole-config assertions and the 18 rejection mutants.
- `python tools/ci/test_windows_diag_sample.py`: 58 tests.
- With `CARGO_TARGET_DIR=D:/Temp/arx-r5obs-c761aed/target` and
  `CARGO_PROFILE_DEV_DEBUG=0`, `cargo test -p arxd --lib --locked --offline
  cluster::consensus::three_node::r5_ -- --skip r5_shutdown_read_owners`:
  4 pure units passed, 571 filtered out; build 30.14 seconds, tests 0.00 seconds.
- `cargo fmt --all --check` and `git diff --check c761aed f0b9418`.

No natural R5 rerun, controls, broad cluster suite, heavy engine test, new mutant,
Clippy rerun, hosted check, dispatch, publication or native test was performed.

## Reused evidence and limits

Read retained local logs under `D:/Temp/arx-r5obs-c761aed/logs`: natural,
list, units2, cf-store/cf-permit/cf-drop, guard-mutant, Clippy, sampler and
e3/e4/s2b guard summaries. Natural R5 passed once in **6.883 seconds**, with
passing stderr visible and a 281-byte R5 line. The three control logs each show
the unchanged cleanup unwrap failing with Elapsed and distinct store/permit/drop
stage facts. These runs are builder evidence, not reviewer reruns or hosted
reproductions. Seven rendering/stamp mutation runs remain builder-reported
evidence; this reviewer did not repeat them.

The builder reports that natural-run source differed from fbc929b by one later
comment edit. The log alone cannot independently reconstruct that working tree.
The committed fbc929b-to-c49f297 delta was checked: only a Clone/Copy derive and
the log-only holder unit addition. Final source is identical at c49f297/f0b9418;
the four pure units were rerun here against that source. Earlier Clippy and
e3/e4/s2b passes are reused with that provenance limit.

## Merge blockers and reversibility

Main c612152 remains recorded RED (CI43880904887, Windows R5 cleanup Elapsed).
The separate root-retained 1475-second main time-budget/capacity blocker is also
retained as supplied steering evidence; no live remote check was made here.
Neither a local diagnostic pass nor this review makes main green. No ci-speed
label is authorized. Candidate merge requires exact-main green or separately
recorded steering, plus all five exact-head gates and the coordinator's action.
No source finding needs reproduction/fix; these external gate blockers remain.

The change is reversible by reverting the test instrumentation/config/guard
commits; no production state or data migration is involved. It earns no fix,
milestone acceptance or hardware credit. All hardware paths remain
**unverified on real hardware**. Root should keep PROGRESS current when consuming
this report; the reviewer owns only this review file.

Standards: 0 findings; Spec: 0 findings; Security: 0 findings. Merge blocked by
retained main health/time-budget prerequisites, independently of source PASS.
