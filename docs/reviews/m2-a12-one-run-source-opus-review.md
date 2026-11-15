# A12 one-run source/content review (Opus)

2026-11-15. Anthropic Claude Opus 5.5 / high, independent review sub-agent (Opus
reviewing Sol), no subagents. Read-only apart from this file. No push, PR, comment,
status, merge, watch, dispatch, variable, native, host/root/proc/sys/account/sudo,
Runner or LinuxFs action.

**Verdict: NOT ELIGIBLE for publication as-is. One blocking finding (S1); security clear.**
After the S1 correction, a new exact inventory and a fresh independent recheck
(reserved `docs/reviews/m2-a12-one-run-source-opus-recheck.md`) are required by
the preparation record before eligibility.

## Pinned target

- Base: `fb23b58ed5a333044af746bdfe15afb7c0bec64f` (origin/main), tree
  `1b861b4a175c00a63ca7e64ec35b017442d4e7a7`. Reviewed PR137 head
  `b80f8881bb928aef034a1843b6ea7c78c5303089` has the identical tree (recomputed).
- Head: `8e42768c048bb2a90095eb624fbaea0cad152b1f` (work/m2-a12-one-run-candidate).
  Commits: `ea4c25b` (coordinator PROGRESS), `06c8641` (source), `d1d0d27`
  (ADR/evidence), `8e42768` (coordinator PROGRESS).
- Retention: builder `0ff89c84…` and retained `06c8641` share tree
  `ea44d7d01efefec8cf09cba517f371f408deffa7`; builder `e1570698…` and retained
  `d1d0d27` share tree `3e820d19abc6e50f3e451f2cdffca00416fc13fd`. `8e42768`
  changes only `docs/PROGRESS.md`.
- Spec: docs/KICKOFF.md, AGENTS.md, ADR0019 D7l (proposed), docs/evidence/
  linux-a12-one-run-preparation.md.

## Scope and inventory (independently recomputed from Git bytes)

- Changed paths vs base: exactly a12.rs, driver.py, test_driver.py,
  mutation_check.py, a7-admission-readonly.py, ADR0019, the evidence record and
  PROGRESS. All allowed, all regular 100644; no deletions/renames/symlinks/
  submodules/mode changes; every tree entry is 100644.
- Non-doc inventory: 223 baseline / 223 candidate. Canonical digests reproduce
  exactly: baseline `84f8446e…2b9d05`, candidate `615c780a…9af5d`. All 223 table
  rows match HEAD mode, blob, SHA256 of blob bytes and size; table paths equal the
  non-doc tree. Non-doc tree of `06c8641` equals HEAD.
- Driver `tools/unix-exec-validation/driver.py`: SHA256
  `24dd0d6a2c8948f2092e54fff40bbb03c4364ab2a07e59c6e12d62517e5fe47f`, 96767 bytes
  (checkout LF bytes match). The wrapper pins equal these; no stale old pin
  (`a8a01bc5…`/96781) remains outside docs.
- Exactly four selection changes: Rust `DELIVERY_ENABLED = true`; driver
  `TEST = 'exec::a12::native::disposable_linux_a12'`, `A12_DELIVERY_ENABLED = True`,
  `A12_EXECUTION_ENABLED = True`. The rest of the Rust diff touches only test fns.
  The rest of the driver is byte-identical. The wrapper diff touches only
  `_MODULE_SHA256`/`_MODULE_SIZE`. Workflows, Cargo, packaging, helper, rights,
  sudo/NNP/A7 logic, runtime contracts, worker/cancel/join, oracles, cleanup and
  budgets are unchanged.

## Standards

- **S1 (BLOCKING): the legacy driver mutation harness regressed.**
  `tools/unix-exec-validation/mutation_check.py:17` `CASES[0]` ('restore
  positive A4 selection') still anchors on the removed literal
  `TEST = 'exec::disposable::sudo_nnp::disposable_linux_a7_a11'`, and its
  assertion names the renamed test `test_source_selection_is_combined_a7`.
  `mutation_check.main()` now aborts on its first case
  (`AssertionError: baseline: test_source_selection_is_combined_a7`), so none of
  its 40 historical in-memory driver mutants run. At base `fb23b58`,
  `main()` kills all 40. This contradicts the evidence claim that mutation
  anchors were adapted, and the brief's requirement that historical mutants stay
  meaningful. Minimal fix, verified in memory without editing the repo: anchor
  `"\nTEST = 'exec::a12::native::disposable_linux_a12'"` (the leading newline is
  required because `A12_TEST = …` also contains the bare substring) → after
  `"\nTEST = 'exec::disposable::a4::disposable_linux_a4'"`, test
  `test_source_selection_is_a12_candidate`. With that change, `main()` kills
  40/40 with driver source unchanged at `24dd0d6a…`. The driver hash and wrapper
  pins stay unchanged; only the mutation_check row and candidate digest change.
- S2 (advisory): the native ignore reason, pinned verbatim by the source test, still
  says "unselected". It is now selected. Harmless, but stale wording. Keep it, or change
  it only with the pin.
- S3 (advisory): the driver comment "Neither delivery preparation nor execution
  is authorized by selecting a name" is still true under D7l. No change needed.
- S4 (advisory, smell): `test_candidate_registration_and_guard_without_host_effects`
  poisons entry points under `hasattr`, so a renamed entry point would be
  skipped silently. All nine names exist today (checked).
- Other Python harnesses have the same result as base: `a7_case_mock_checks` 5,
  `admission_path_mutants` 15, `execution_admission_mutants` 10.
  `a7_integration_checks` and `execution_preparation_checks` fail identically at
  base and HEAD in this local setup (pre-existing, not introduced).

## Spec

- Ordinary test isolation holds. `prepared_delivery`/`read_delivery` are private
  to the `#[cfg(target_os = "linux")] mod native`, so portable tests cannot reach
  them. The only non-ignored native test is now
  `a12_delivery_candidate_injected_reader`, which uses injected closures only. The
  sole enabled-reader caller is the still-`#[ignore]`d `disposable_linux_a12`.
  ci.yml and the experiment workflow never pass `--ignored`/`--include-ignored`
  except the driver's fixed exact-name argv.
- Source-gate counterfactuals exercise the real production body, not the test's
  own strings. `wired()` slices source before `fn a12_acceptance_counterfactuals`,
  which excludes all portable tests. The first `replacen` matches of the bypass
  and extra-caller mutants land on production lines 272 and 499. I simulated the
  guard against mutated source. It rejects: an ordinary test calling
  `prepared_delivery()`; an ordinary test calling `read_delivery()`; removal of
  the `#[ignore]`; and the body bypass.
- Historical false-gate driver counterfactuals stay meaningful.
  `test_closed_gates_before_effects` injects both-false. The new
  `test_each_closed_flag_refuses_before_effects` covers FF/FT/TF with
  uid/open/lstat/build/call/cgroup/file poisoned. Per-site in-memory removal
  is killed as a failure (not an error) by at least one test: run_body gate,
  main gate, and prepare_a12 gate (the last by `test_closed_gates_before_effects`).
  A7 historical classes explicitly restore the A7 selection via `setUp`.
- A7 wrapper: only the consumed MODULE hash/size follow the driver. The A7 route is not
  revived. `A7_ADMISSION_REVIEWED_SHA` stays absent under D7l, and A7 selection is false.
- D7l binding is sound and narrow. It requires exact base/review-tree identity,
  the full 223-row inventory, a strict path/mode predicate, and the first eligible
  normal squash only. It needs fresh Opus content review plus exact-head review,
  five green checks, coordinator merge and squash tree/inventory equality before
  it takes effect. It allows one variable set with readback, one dispatch, a direct
  URL monitored to terminal, and one final clear with all-three 404s. Any accepted
  or confirmed-rejected dispatch consumes it, with no retries. A7/A11 hosted
  FINAL NO-GO and consumed D7a-k are preserved, and nothing reopens them.
  Predictions are labelled feasibility, not acceptance. Credit requires a
  mandatory independent post-run assessment. Native work is **unverified on real
  hardware**.
- SP1 (advisory): the in-tree Rust gate's false branch is no longer exercised.
  The `const { assert!(DELIVERY_ENABLED) }` design makes it unreachable, and the
  builder's true/false extraction is not retained in-tree. Deleting the
  `if !DELIVERY_ENABLED` guard entirely passes every in-tree test (simulated).
  This is harmless while enabled. Any post-run re-closure must restore a
  false-state no-read test.
- SP2 (advisory): `prepared_delivery()` caller counting misses a parenless
  reference (`let f = prepared_delivery;`), confirmed by simulation. Counting the bare
  identifier, which is also 2 today, would close this. Optional.

## Security

- No security blocker. No secrets or owner data are present. No new host, network,
  or privilege path is added. Gates stay fail-closed: the workflow keeps its
  repo/actor/ref/sha/pin checks, and the driver keeps its gate and E_TEST_ARGS checks.
- SEC1 (advisory): "attempt 1 / no rerun" is enforced by procedure for A12. The
  driver requires `GITHUB_RUN_ATTEMPT == '1'` only for the A7 selection. The
  attempt is embedded in the stage/run identity, and only the owner actor can
  trigger the run. Adding a source check would exceed D7l's four-selection
  scope. Instead, the coordinator must not rerun, must clear the pin at terminal,
  and the post-run assessment must reject any attempt other than 1.
- The Linux-only native module, including the new injected test, was not
  compiled locally. Hosted Linux compile remains a required gate.

## Checks

Run by this reviewer (Windows; TEMP/TMP and Cargo target on D:; no native calls):

- `cargo test --locked --offline -p arxd --lib exec::a12::`: 7 passed, 409 filtered.
- Residue guard `cleanup_source_pins_release_sites_and_legacy_errno_order` 1
  passed; `stage_b_tests::w8_e7` 2 passed.
- `cargo fmt --all -- --check` passed; `git diff --check fb23b58 HEAD` passed.
- `python -I -B -m unittest discover -s tools/unix-exec-validation -p 'test_*.py'`: 196 OK.
- `python -I -B tools/ci/test-a7-admission-readonly.py`: 13 OK.
- `mutation_check.a12_assertion_mutants()`: 9/9 killed.
- `mutation_check.main()`: FAILS at HEAD, 40/40 at base (S1). It passes 40/40 at
  HEAD with the in-memory corrected anchor.
- Per-site A12 gate mutants (3), source-guard text simulations (7) and other
  mutation_check Python entry points compared against base, all in memory.
- Full tree/inventory/digest/row recomputation, retention tree equality, driver
  SHA256/size.

Reused from builder, not rerun: Windows offline Clippy, S2b applicability test,
extracted Rust closed_read true/false controls. Not run by anyone: full Rust
suite, Linux compile, the native A12 fixture. A12 is unrun, so there is no
acceptance credit.

## Publication eligibility

Not eligible until S1 is corrected in `mutation_check.py`. After that, the
coordinator must refresh the inventory row and candidate digest in the evidence
record and get a fresh independent Opus recheck. Then the D7l chain applies:
exact-head review, five green checks, coordinator merge and squash tree/inventory
verification. The advisories (S2–S4, SP1–SP2, SEC1) do not block.

Summary: Standards 1 blocking (S1, broken legacy mutation harness) + 3 advisory;
Spec 0 blocking + 2 advisory (worst: SP1, unexercised in-tree false branch);
Security 0 blocking + 1 advisory (SEC1, attempt-1 enforced by procedure).
