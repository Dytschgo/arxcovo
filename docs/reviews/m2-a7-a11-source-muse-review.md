# M2 A7/A11 source review - Muse (independent source/security)

Scope: source commit `5244e9727dd39f2f38616928e9c8ee4e9a1c12be`
(`test(m2): prepare dormant A7 A11 sudo NNP acceptance`, 8 files, 993 insertions,
2 deletions) plus evidence commit `0931650afc2520c0c6715140cfab9ea7c4b6d736`
(`docs(m2): record A7 A11 source-only preparation evidence`, 1 file).
Retained parent `c3fafa0a2cd6405318455373954692f8c53bb433`
(`work/m2-after-cancel-join`) atop main `7113a42` (PR117 merge).
Read: AGENTS.md, `docs/KICKOFF.md` M2/routing, accepted plan `8104326`/`f500c12`
(`docs/reviews/m2-a7-a11-sudo-nnp-plan.md`), conditional Muse plan review
`c402332`/`4172891`, evidence
`docs/evidence/linux-a7-a11-source-preparation.md` (blob pins verified below),
and the full `c3fafa0..5244e97` diff. No PROGRESS, source, ADR, workflow,
dispatch, account, sudo, install, network, or push operation performed by this
review. A7/A11, M2, full F/G and real-Runner PA6 are
**unverified on real hardware**.

Method: call-graph audit by reading (controller, rights selection/execute/reap,
Runner/setup, driver entry/selection/cleanup, inventory, test helpers);
source-byte check via `git hash-object` against the evidence blob table;
focused reruns only: `test_driver.A4Delivery.test_closed_mapping_and_default`,
`test_driver.SudoNnp` (9 mocked), `cargo test ... w8_e7_crate_wide_inactive_source_inventory`,
`cargo fmt --check`, `git diff --check`, plus a `-- --list` probe proving the
Linux-only controller/observation tests do not compile on this Windows host.
No LinuxFs, native, helper, ignored, proc, account, sudo, admin, install,
dispatch, variable, push, PR, or subagent operation ran.

## Standards - PASS, no findings

- Exactly one new Rust module `exec/disposable/sudo_nnp.rs` (382 lines) plus one
  `mod sudo_nnp;` line, two inventory lines, `cfg(test)`-only observation
  additions in `unix_shim_rights_linux.rs` (+86/-2) and its tests (+64), and
  dormant driver additions (+152/+140/+168). No CLI, workflow, package, helper,
  Cargo, production activation, ADR, or PROGRESS bytes. Small conventional
  test/docs shape; UTF-8 LF; no secrets, owner data, paths, identities, or
  policy text in source.
- Inactive-source inventory (`stage_b_tests.rs`) registers both the exact file
  and the exact `mod sudo_nnp;` declaration, matching the existing `mod a4;`
  pattern. Reran green.
- No product approval flows, publishing defaults changed, diagnostic framework,
  or policy-parser/host-audit framework added. `visudo` stays a fixed bounded
  syntax refusal, never policy evidence, as the plan requires.
- Baseline smell check: no mysterious names (`prepared_delivery`,
  `sudo_nnp_eligible`, `observation_accepts` say what they do); no duplicated
  logic (Facts/Observation discriminators are per-site predicates, not copies);
  no speculative generality (the `Prepared` struct exists only to be `None`;
  no general parser, no per-run sudo, no admin/macOS/polkit surface).

## Spec - PASS (dormant Slice 1 exactly as planned; claims neither A7 nor A11)

- Closed execution boundary verified by reading: the ignored controller
  `disposable_linux_a7_a11` refuses at its first statement
  (`prepared_delivery().expect(...)` returns `None`) before any host
  observation, LinuxFs, account, or process operation. `sudo_nnp_host_attested()`
  returns `False` and `sudo_nnp_eligible` requires `is True` on both inputs, so
  `prepare_sudo_nnp` raises `E_POLICY_UNATTESTED` before effects; the
  `test_unattested_before_effects` mock asserts `call`/`mkdir` never ran.
  Driver `TEST` remains
  `exec::disposable::a4::disposable_linux_a4_omitted_kill`; the combined
  `sudo_nnp` selector is rejected (`E_TEST_ARGS`, `open` never called); neither
  `run_body` nor any CLI selector calls the dormant functions (comment + code).
  Ordinary CI and native effects are impossible through these paths.
- Production rights and child safety unchanged: the `unix_shim_rights_linux.rs`
  delta is exclusively `#[cfg(test)]` (struct, predicate, thread-local, helper,
  and guarded call sites). With `cfg(test)` off, the selection/execute/reap
  bytes are the pre-existing reviewed logic. The fork child (lines 377-398)
  contains no observation, allocation, or callback: `fchdir`, `dup2`,
  `execveat(AT_EMPTY_PATH)`, single-byte `write`, `_exit(127)` only.
- Scalar observation placement is as specified: C1 selection, alias-skip mark,
  C3-opened mark, `O_PATH`/`FD_CLOEXEC`/`trusted(S_IFREG)`/inode-identity and
  calling UID/GID are captured before `fork`; `exec_eof`/`nonzero` only after
  `waitpid` reap with `count == 0 && WIFEXITED`. The reentry asserts
  `observation_accepts` (C1, alias-skipped, C3-never-opened, flags, regular,
  same-inode, non-root UID/GID match, EOF, nonzero) and prints the fixed latch
  only on pass; the controller's `reentry_records` grammar pins the exact
  libtest path `unix_shim::rights::linux::tests::e4_sudo_nnp_reentry`, the
  latch line, and bounded `filtered out; finished in 0.01s` shape. Module path
  resolves correctly (`unix_shim` -> `rights` -> `linux` -> `tests`).
  `Runner::run_phased`, `LinuxFs::system` setup, `Settings::default()` reentry,
  and the two-account loop are the actual production call-graph, unselected.
- Owned provisioning/cleanup matches the plan's hard constraints by reading:
  pre-mutation collision refusal (account/group names and both privileged
  paths), `E_FIXTURE_UID` shape checks (uid/gid >= 1000, nologin shell, no
  supplementary members, single-group), captured inodes before mutation,
  short-write/`fchmod`/syntax-failure `finally` close semantics, quarantine on
  uncaptured identity/policy (`E_QUARANTINE_*`), `unit_gone` gate before any
  deletion, live-UID `/proc` scan bounded at 20000, `same_object` rechecks,
  reverse-order owned-only removal, residue rechecks, never adopt/delete
  foreign state. The narrow root-owned `0440 ... NOPASSWD: /usr/bin/true` rule
  applies to exactly one of the two cases.
- No future artifact or policy proof fabricated: `prepared_delivery()` is
  `None`, host proof is literal `False`, policy proof is literal `False` at
  the call site, no artifact/source/workflow SHA is invented, and the evidence
  file states the unattested-host/native-grant blockers explicitly.

## Security - PASS, no new exposure; constraints retained

- Dormant code cannot execute outside an explicitly selected Linux ignored run:
  `None` delivery, `False` host attestation, closed TEST selector, and
  `#[ignore]` on both the controller and the reentry. No ambient authority is
  added; no existing fence is widened.
- Async-signal-safe child untouched (verified above); observation state is a
  parent-side thread-local never touched by the child.
- Bounded-output discipline holds: 4096-byte caps on probe/reentry parsers,
  96-byte rights line via existing `append`, dropped-byte and stderr-emptiness
  refusals in `output()`, privately reduced booleans in the controller.
- Cleanup is fail-closed (quarantine on uncertainty, stop/cgroup confirmation
  before deletion, inode/identity re-verification). `visudo -c -f` is used only
  as a syntax refusal, consistent with "syntax success is never policy proof".

## Actual checks vs reported

| Reported (evidence file) | This review |
| --- | --- |
| `mutation_check.sudo_nnp_source_checks`: 4 isolated pure Rust, 9 mocked, 45 assertion-killed in-memory mutants, source bytes unchanged | Source bytes verified unchanged (`git hash-object` reproduces all 8 evidence blobs exactly). Mocked suite rerun: 9/9 PASS. Pure/mutant harness read in full; shape matches the claim (Facts discriminators, Observation scalars, bounded record parsers against the real offline `serde_json` rlib, RunResult predicates, 3 driver mutants). Full 45-mutant rerun not repeated; unnecessary after byte-equality plus green focused suites. |
| `test_driver.A4Delivery.test_closed_mapping_and_default`: 1 test PASS | Reran: 1 test OK. TEST A4 negative default and closed mapping retained. |
| `cargo test ... w8_e7_crate_wide_inactive_source_inventory -- --exact`: PASS | Reran: 1 passed. |
| `cargo clippy --locked --offline -p arxd --all-targets -- -D warnings` (Windows target only): PASS | Not rerun (53 s compile already paid for the test probe; code adds no new lints by inspection: no new warnings surface, `#[allow(dead_code)]` only inside the harness snippet, not in-tree). Builder claim accepted with this caveat. |
| `cargo fmt --all -- --check`, `git diff --check`, UTF-8 LF: PASS | Both rerun: PASS. |
| Linux-only controller/observation integration not compiled or executed | Corroborated: `cargo test -p arxd --lib -- --list` on this Windows host shows no `sudo_nnp`/`s14`/`e4_sudo_nnp` tests (`mod disposable` is `cfg(all(test, target_os = "linux"))`); a filtered run matched 0 tests. |
| Mutation coverage list (wrong identity, zero/nonzero/unknown/absent rights, candidate, alias/open, flags, regular/inode, exec EOF, missing policy, cleanup, truncation/overflow, extra/missing records, status/boundary, collision, stop-gate) | Each maps to a concrete discriminator or mocked case read in the diff. No native fix attempt or native acceptance credit exists. |

Production E4 source pins needing exact refresh: none. `A4_IMAGE_SHA`, helper/recipe/image pin digests, `a4_delivery` authorization logic, `CANDIDATES`/alias/exec boundary, `TOOLS`, and S8-S13 tests are byte-untouched; the only rights-file delta is additive `cfg(test)`. No pin refresh is required and none was made.

## Eligibility and remaining gates

- Eligible: publication of this dormant source slice through the normal
  exact-head review and gate flow as **source/pure/mock evidence only**.
- Not eligible: any A7/A11, M2, full-F/G, or real-Runner PA6 claim. The
  evidence file already states this; this review concurs.
- Remaining delivery/native gates (unchanged from the conditional plan
  review): attest a qualifying installed-sudo/local-policy host image; obtain
  the new narrow native grant naming both accounts/cases, artifact/source/
  workflow pins, one dispatch, and cleanup/variable lifecycle; then exercise
  both cases plus owned cleanup in a separate delivery slice with fresh
  independent review and green required checks including `ci-time-budget`.
  Coordinator merges only after all gates pass.

Findings: Standards 0; Spec 0; Security 0. Worst issue in any axis: none;
strongest caveat is the un-rerun Windows Clippy gate (accepted on inspection)
and the by-design uncompiled Linux integration, both already declared in the
evidence file. A7/A11/M2/full-F/G/PA6 remain **unverified on real hardware**.
