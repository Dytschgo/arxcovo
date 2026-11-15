# Linux A7/A11 source preparation (Slice 1)

OpenAI / gpt-6.1-sol / high, 2026-11-15. Implementation sub-agent only.
Prepared parent: `c3fafa0a2cd6405318455373954692f8c53bb433`, incorporating
main `7113a420140de8a45a17c82a25305f8794c3adb8` and retained plan/review.
Source commit: `5244e9727dd39f2f38616928e9c8ee4e9a1c12be`.
Contract: `docs/reviews/m2-a7-a11-sudo-nnp-plan.md` and retained accepted
`docs/reviews/m2-a7-a11-sudo-nnp-plan-muse-review.md`.

## Delivered source and closed execution boundary

- One ignored controller, `exec::disposable::sudo_nnp::disposable_linux_a7_a11`,
  and literal ignored `unix_shim::rights::linux::tests::e4_sudo_nnp_reentry`.
  Controller uses actual production setup once and actual Runner::run_phased
  for staged production arxd `[tool-probe]` and the reentry, twice each across
  the two prepared account configurations. These are source paths, not runs.
- No host/policy/delivery attestation exists. `prepared_delivery()` returns None
  before any host observation. Dormant driver preparation separately refuses
  through the pure eligibility predicate with actual host proof false and policy
  proof false. Tests inject eligibility only as a mock precondition. Neither
  test nor driver source supplies runtime artifact pins or an installed-sudo proof.
- Test-only scalars capture selected C1, accepted alias/C3 skipped without open,
  O_PATH/CLOEXEC, trusted regular held inode identity, UID/GID before fork, and
  normal nonzero exit with EOF on the existing exec-error pipe after reap.
  The async-signal-safe child is untouched. Reentry uses Settings::default(),
  real anchor/owner/installed candidate selection and unchanged rights operation.
  It asserts NNP and exit_nonzero; private bounded output contains a fixed latch.
- Controller checks pinned Runner identity, exit/status, capture bounds, strict
  ordered tool records and final true/exit_nonzero rights line, bounded exact
  reentry grammar, tree cleanup and cleanup_residue. Two account cases are
  mandatory. Raw native outputs must remain private; no native output was obtained.
- Dormant driver preparation is owned-only, outside the Standard service: two
  exclusive generated account/private workspace cases, one narrow root-owned
  0440 NOPASSWD true rule and one without a rule. Fixed bounded visudo checks
  syntax only. No parser or syntax-to-policy inference exists. Captured identity,
  collision refusal, finally cleanup and uncertain-capture quarantine are covered
  by mocks. Cleanup requires unit/cgroup absence confirmation before deletion,
  checks live fixture UIDs and matching owned state, then rechecks absence.
- Actual driver TEST remains the A4 omitted-kill negative. Closed mappings reject
  the combined selector. No CLI, workflow, package, helper, Cargo, setup, Runner,
  production activation, ADR or PROGRESS change. Inactive inventory adds only the
  exact module/file registrations. Production rights algorithm is unchanged;
  added observations are cfg(test).

## Call-graph audit and checks actually run

Before test commands, inspected the controller, rights selection/execute/reap,
Runner, setup fixture, driver entry/selection/cleanup, inventory and test helpers.
The source-check command extracts only scalar predicates, bounded record parsers,
real RunResult types/predicates and pure tests. It links an existing offline
serde_json rlib; no native adapter is included. Generated Rust tests operate on
values only. Python SudoNnp mocks filesystem/account/process/policy/cgroup effects;
Unix-only APIs/modules are supplied by mocks on Windows. Inventory reads bounded
repository source only. The A4 mapping test is pure with opens mocked.

| Executed check (Windows host) | Result |
| --- | --- |
| `python -c "import mutation_check; mutation_check.sudo_nnp_source_checks()"` in `tools/unix-exec-validation` | PASS: 4 isolated pure Rust tests, 9 mocked tests, 45 assertion-killed in-memory mutants; controller/rights source bytes unchanged |
| `python -m unittest test_driver.A4Delivery.test_closed_mapping_and_default` in the same directory | PASS: 1 test; negative default and closed mappings retained |
| `cargo test --locked --offline -p arxd --lib exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory -- --exact` | PASS: 1 test |
| `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings` | PASS (Windows target only) |
| `cargo fmt --all -- --check` | PASS |
| `git diff --check` and staged diff check | PASS |
| UTF-8 without BOM, LF source bytes | PASS: all eight source paths below |

Mutation coverage: wrong identity, zero/nonzero/unknown/absent rights, selected
candidate, alias/open behavior, flags, regular/inode identity, exec-error EOF,
missing policy case, cleanup, truncation/capture overflow, extra/missing records,
newline truncation, status/boundary, account collision and cleanup-stop gate.
Mocks also cover path collision, short write, syntax failure/finally cleanup,
partial account creation and uncaptured policy quarantine. Initial check-harness
issues (assertion message matching and Windows Unix-API mocks) were corrected;
no native fix attempt or native acceptance credit occurred.

## Limits and next gate

No native/process probe, ignored fixture, LinuxFs, account, sudo, administrator,
helper, Linux installation, network/install, workflow dispatch, variable change,
push or PR operation ran. No sub-agent or independent source review was invoked.
The complete Linux-only controller/observation integration was not compiled or
executed on Linux; the isolated pure checks do not prove that integration.
Production S8-S13 native regressions were not rerun in this slice.

Fresh Muse source/exact-head review is required before publication. A later
separate delivery slice must attest the qualifying host and closed policy image,
connect pinned owned inputs to the controller, settle real artifact/source/workflow
pins and a new narrow native grant, then exercise both cases and owned cleanup.
No future SHA is invented here. CI and review merge gates remain outstanding.
A7/A11, M2, full F/G and real-Runner PA6 are **unverified on real hardware**.
This slice earns source/pure/mock evidence only.

## Exact source blobs

| Path | Git blob |
| --- | --- |
| `crates/arxd/src/exec/disposable.rs` | `252a60449e1217a055129c4aed9d1ed7050912b5` |
| `crates/arxd/src/exec/disposable/sudo_nnp.rs` | `536844c0ce88196e40db422ec74dc7ac9aebe717` |
| `crates/arxd/src/exec/stage_b_tests.rs` | `966bdaa194f5b6c6ad31cf1fec0c50f9072b95d3` |
| `crates/arxd/src/unix_shim_rights_linux.rs` | `daa2144283fea0d2b4bd859c796fb86a827b631d` |
| `crates/arxd/src/unix_shim_rights_linux_tests.rs` | `d02fd13090d0e778100a6fd9a8a8da28b5b60e92` |
| `tools/unix-exec-validation/driver.py` | `2d8a61f9174acf2e7ce3493b851a739c91ff5dd0` |
| `tools/unix-exec-validation/test_driver.py` | `f1ba403bfdd7be4339d7dc18a8006f58088624d9` |
| `tools/unix-exec-validation/mutation_check.py` | `1623185403f481abfb2a838fc4c4590a3143e950` |
