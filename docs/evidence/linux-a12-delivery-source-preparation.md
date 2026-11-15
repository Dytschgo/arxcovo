# A12 delivery preparation: cwd contract handback

2026-11-15, OpenAI gpt-6.1-sol / high implementation sub-agent, no subagents.
Parent `a04c4b581f6a67faf28bd10a0aac44cef6d6289c`, atop merged PR136 main
`cbab8b69f7204c81e4490da107dc1bca3282cf0c`.

## Exact conflict and bounded outcome

The assignment explicitly requires setting the actual native-call spec's cwd
to the owned `config.workspace`, and explicitly requires stopping at a
substantive unsettled contract rather than silently adding capabilities.
Those requirements meet an existing production contract conflict:

- `crates/arxd/src/exec/mod.rs:206`: `run_phased` first calls
  `validate_phased_spec`, mapping validation failure to
  `NotStarted(Invalid)`, before `run_core` and `unix::run`.
- `mod.rs:335`: true/sleep arguments take the ordinary `validate_spec` branch.
- `mod.rs:309`: `validate_spec` accepts empty cwd or a clean relative cwd.
  An absolute workspace as cwd returns `ExecError::Invalid` with the literal
  `absolute executable and workspace-relative cwd required`.
- `crates/arxd/src/exec/unix.rs:190`: the native backend separately encodes
  `config.workspace` as its absolute root; empty spec cwd becomes `.`.
- `unix.rs:331` and `unix.rs:565`: after identity drop, `workspace_cwd` opens
  that root without symlinks and resolves the relative cwd beneath it, then
  the child uses `fchdir`. Empty cwd already means the owned workspace root.

An absolute cwd would therefore prevent **both** cases from reaching native
execution and could not earn A12 credit. Changing either production validator
or the path-resolution contract is forbidden by this assignment. No such
change is made, and the requested delivery integration is **not implemented**.

Minimal contract to settle before continuing: allow the existing empty cwd
workspace-root representation in `RunSpec`, and bind/reopen/recheck the absolute
owned `OwnerConfig.workspace` before each actual call. Prove the spec passes
the actual validator and the closure at the Runner boundary receives it. This
is a proposed clarification, not an adopted design or execution grant.

## Regression and audit before tests

Added only `a12_delivery_cwd_contract_conflict` in
`crates/arxd/src/exec/disposable/a12.rs`. It tests both cases against the actual
legacy and phased validators: empty accepted, absolute workspace rejected with
the exact cwd error, empty accepted again. Windows uses a fixed native absolute
executable spelling so executable validation cannot mask the cwd error. No
paths are opened and no Runner is constructed or called.

Before testing, read AGENTS/KICKOFF/PROGRESS, brief0020 A12, ADR0007's relative
cwd and identity contract, ADR0009's dedicated identity and containment clauses,
ADR0017's report boundary, ADR0020's D6 residue semantics, existing A12 evidence
and independent Opus review. Traced `run_phased -> validate_phased_spec ->
validate_spec -> run_core -> unix::run -> workspace_cwd`. The actual native
runner directly executes the target; it does not require an arxd production
image, sudo helper or A7 qualification. No A7 implementation was reused.

Audited the selected tests before execution: portable A12 tests call pure
acceptance/validation/source predicates or `exercise` with injected closures,
atomics and owned mock threads. The Linux-only delivery test calls the literal
`None` reader; the ignored native test is not selected. The residue source test
uses only `include_str!`. W8 scans bounded repository source and checks inactive
declarations and negative controls. S2b's selected applicability test reads
fixed repository files and applies its two mutations in memory; its import has
no native entry. Its unittest setup creates and cleans only its D: temporary
fixture. No actual Runner, LinuxFs or native fixture is exercised.

## Closed boundaries and remaining delivery work

`prepared_delivery` remains literally `None`, the ignored native fixture is
unselected, and driver TEST remains A7's unchanged historical literal. No
source reader/schema, account provisioning, owned A12 driver branch, artifact
handoff, summary markers or cleanup implementation is claimed by this patch.
No source-pin constant changes are needed: neither mod.rs nor any consumed
S2b/D7j pinned source changes. The trailing mod.rs declaration is untouched.

The future slice still owes the requested <=4096-byte root0600/nlink1 strict
JSON with duplicate/unknown-field refusal and validated stage-derived binding;
one exclusively owned standard nologin account/group; held/reopened identity
checks before each call; bounded single-libtest build and artifact pins; fixed
trusted true/sleep targets; collision/partial-capture quarantine; unit/cgroup
and live-UID release before any deletion; exact markers/summary/ACK/runtime
attribution; and the requested counterfactual/mutation tests. No schema is
invented while the call-boundary contract is unsettled.

Existing proof, cancel/join, real residue/ENOENT and shared-runner semantics are
unchanged. Budgets remain 15s preparation, 5s ACK, 15s Runner, 5s proof, 25s
whole call, 90s total, packaged startup30/monitor120/runtime140. A7/A11 remains
FINAL NO-GO. No grant/ADR/PROGRESS, package, helper, Cargo, workflow or production
change. No native host/proc/sys/root/sudo/account/policy/cgroup, network,
dispatch, variables, install, cache deletion, push, PR, merge or watch action.

## Source identities

Git blobs, mode100644:

| Path | Blob |
| --- | --- |
| `crates/arxd/src/exec/disposable/a12.rs` (this patch) | `1ff471b5262f1da800f77a5c87c21cc66296ee0d` |
| `crates/arxd/src/exec/mod.rs` (unchanged) | `b150f543a894d57b25c56bab19c95b4f9bd2beeb` |
| `crates/arxd/src/exec/unix.rs` (unchanged) | `0d087e37f31f3e17db652a87f2a0364eb398479e` |
| `tools/unix-exec-validation/driver.py` (unchanged) | `acfd9c61aaa10bc065f43d80a15be0cc9e733291` |

## Checks and limits

All Cargo outputs and TEMP/TMP use `D:/Code/arxcovo-a12-delivery-source/`.
Actually ran on Windows, Rust1.95.0, locked/offline:

- `cargo fmt --all -- --check`: passed.
- `cargo test --locked --offline -p arxd --lib exec::a12:: -- --nocapture`:
  4 passed, 409 filtered, 0.05s after 54.64s build. Expected injected observation
  and worker panics were caught by the existing mock ownership test.
- `cargo test --locked --offline -p arxd --lib
  exec::residue_tests::cleanup_source_pins_release_sites_and_legacy_errno_order
  -- --exact`: 1 passed, 412 filtered, 0.00s.
- `cargo test --locked --offline -p arxd --lib exec::stage_b_tests::w8_e7 --`:
  2 passed, 411 filtered, 0.11s. Other stage-B tests were not selected because
  this assignment excludes actual Runner calls.
- `python -I -B -m unittest discover -s tools/ci/s2b-native -p test_driver.py
  -k test_pinned_manifest_applicability_in_memory_only`: 1 passed, 0.012s;
  existing pins/workflow and two in-memory mutations verified unchanged.
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`:
  passed, 29.12s.
- `git diff --check`: passed.

No new delivery/schema/cleanup mutation coverage is claimed: those behaviors
were not implemented. The new validator regression has independent positive
and negative controls for both fixed cases, testing actual production
validators without invoking the native backend. No Linux compile was run here.

This is a contract-conflict handback, not completed delivery source or native
acceptance. Fresh independent Opus review and hosted Linux compile gates remain
required before publication/merge. M2 remains incomplete; independent M3 is
outside this assignment. Hardware remains **unverified on real hardware**.

## Coordinator resolution of assignment error

2026-11-15, Sol coordinator. The absolute-cwd instruction was incorrect and is withdrawn. Existing ADR0007/validate_spec/workspace_cwd semantics remain binding: RunSpec.cwd stays empty for the owned workspace root; OwnerConfig.workspace is the absolute trusted root and must be reopened/rechecked immediately before each call. The handback regression proves this actual validator contract. No production validator/backend or architecture change is needed or authorized. Resume the remaining bounded delivery source under this corrected brief; the preceding handback is historical, not an open contract blocker. No native grant or selection permission follows.
