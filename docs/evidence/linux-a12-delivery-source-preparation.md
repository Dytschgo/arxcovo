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


## Corrected-brief implementation (closed source only)

2026-11-15, OpenAI gpt-6.1-sol / high, implementation sub-agent, no
subagents. Parent `32f685f`, retaining the historical stop and coordinator
resolution above. Source commit `3f319741b04837b177eb5371734774e1eb094803`.
The resolved cwd issue is not a blocker. Empty RunSpec.cwd remains binding;
no production validator/backend, ADR, PROGRESS, Cargo, package, workflow,
rights or A7 admission logic was changed.

### Delivered paths and authority

- `crates/arxd/src/exec/disposable/a12.rs`: strict bounded record reader,
  derived layout, fixed config, real Linux revalidation, call-boundary refusal,
  markers and portable counterfactuals. The historical cwd regression is renamed
  `a12_delivery_workspace_relative_cwd_contract`; its actual positive empty and
  negative absolute validator controls are retained.
- `tools/unix-exec-validation/driver.py`: separate A12 build/account/handoff/
  cleanup branch and fixed mapping. Actual TEST stays the historical A7 literal.
- `tools/unix-exec-validation/test_driver.py` and `mutation_check.py`: injected
  descriptor/account/process tests and finite in-memory A12 assertion mutants.
- `tools/ci/a7-admission-readonly.py`: only its consumed shared-driver SHA256
  and size constants refreshed. No wrapper logic, policy, manifest or other pin
  changes. Driver is normalized LF: 96720 bytes, SHA256
  `138c0a6487511a9064c4627be1785acc01b424fb33842d1e0489c9d721824d75`.

Rust DELIVERY_ENABLED, Python A12_DELIVERY_ENABLED and A12_EXECUTION_ENABLED
are all literal false. The Rust closed gate returns before current_exe, file,
account or host reads. Python run/main refuse an A12 selection before host
probes; prepare_a12 refuses before imports/reads/build/mutation. The ignored
native test remains unselected and cannot run through its closed reader.
Preparing this source grants no candidate, dispatch, account or execution
permission. A7/A11 hosted FINAL NO-GO remains unchanged.

### Schema, paths and binding

Fixed `a12-delivery.json`: root-owned regular mode0600, nlink1, exact advertised
length and at most4096 bytes (4097-byte read ceiling). Typed serde structs reject
unknown and duplicate fields, including nested pins, trailing data and invalid
numeric types. Schema keys are exactly:

```
version, run, source_sha, workflow_sha, uid, gid, root, workspace, harness
pin: dev, ino, mode, uid, gid, nlink, size, sha256
```

Version is1. There are no serialized paths, argv, profile, account names, limits,
CLI or environment overrides. Derive stage/run/attempt/account from exact
`/var/lib/arx-ci-<id>-<attempt>/source/target/debug/deps/arxd-<hex>` current_exe
layout; numeric leading-zero, account-length, attempt-length and hex controls
refuse invalid layouts. The fixed root0600 `a12-source-sha` is written only from
the existing validated driver environment/source-checkout binding; both record
SHAs must equal it. Harness full hash, size<=128MiB, dev/ino, root owner/group,
regular0700 and nlink1 must equal reopened bytes/metadata. Every ancestry walk
uses no-follow descriptors and refuses writable or non-root trusted ancestors.
Stage itself is root/group0 mode0700 at initial admission.

One exclusive standard account/group is named `arxa12<id>x<attempt>`, uid/gid
>=1000, primary group only, no supplementary memberships, nologin shell and
fixed home. Its root is `/var/lib/<account>` and workspace is its literal
`workspace` child, both owned by that account and mode0700 with captured dev/ino.
The root is outside the protected stage so the existing post-drop workspace
open can traverse it. This uses the existing cwd/backend contract, not a new
path capability. Root and workspace pins are both checked; the workspace is
opened relative to the reopened, checked root with O_DIRECTORY/O_NOFOLLOW.

OwnerConfig uses that absolute workspace, empty spec cwd, admin=false,
64MiB memory/pids32, PATH=/usr/bin:/bin, protected_directories=[stage], fixed
`/sys/fs/cgroup/system.slice/arxd.service/runs`. Existing backend sets swap0;
RunSpec keeps output1024 and timeout15s. Targets remain /usr/bin/true with no
args and /usr/bin/sleep with literal15. No arxd production image, helper,
sudo/NNP/package/C2/policy qualification or A7 admission adapter is needed.

Immediately before EACH actual Runner call, in its owned worker, checked_call
rechecks the real NSS identity, bounded local passwd/group uniqueness and
membership, owned root/workspace, harness bytes and fixed trusted system target.
It reopens runs, requires root/nonwritable directory metadata and compares
its dev/ino to the held runs descriptor. A failed check returns NotStarted
Invalid without calling Runner. Local account records are bounded1MiB each;
NSS buffers are16KiB and getgrouplist has two slots with exactly one required.
No future recheck is a stub. Native implementations have not been executed here.

### Build, proof, output and cleanup

Build A12's one libtest harness with the existing offline root-private copy,
360s process wait and a4MiB streaming capture; kill/wait owned child on timeout,
join drain, refuse overflow/failure/late or backward clock observations, require
one fixed-layout test artifact. There is no second production build.

Retain existing15s startup-to-READY,5s ACK,15s Runner,5s populated-leaf proof,
25s whole call and90s fixture bounds; packaged startup30/monitor120/runtime140
remain unchanged. ACK attribution retains exact runtime files/MainPID/executable,
supervisor cgroup and sole PID. Per-case raw writes bypass libtest capture:
ARX_A12_NORMAL_OK, ARX_A12_CANCEL_OK, ARX_A12_COMPLETE_OK, each exactly once.
Verifier also requires exact one-test libtest summary and successful owned unit
exit. Existing proof-before-cancel, explicit owned join, real cleanup_residue,
empty tasks/leaf listing and held-leaf literal ENOENT checks remain. No marker,
premature cancellation, pre-spawn error or missing join can substitute for proof.

Account/group/path collisions are refused without adoption. After an exclusive
root creation, pending state is recorded before provisioning; any failed capture
or uncertain partial provisioning quarantines rather than guesses ownership.
Normal cleanup verifies/stops the exact owned unit and confirms cgroup absence,
then checks account identity and all four Uid fields of bounded proc status reads
(max4096bytes, max20000processes), before any account/root/stage/runtime deletion.
Live UID, changed inode, pending state or failed runtime attribution refuses
cleanup and retains reservation/stage. No static VM or backup deletion exists.

### Audit and checks actually run

Before execution, traced the portable A12 tests to pure schema/layout/metadata
and actual validators, injected Read ports and scoped mock workers. The ignored
native fixture, LinuxFs and Runner were not selected. Audited A12 Python cases
use fake pwd/grp modules, patched process/descriptor/stat/read/clock/file ports;
real repository source reads are only for AST/source fences. The shared ten
monitor/cleanup tests use patched systemd/proc/file/process ports. The loader's
13 tests use its existing FakeOS ports and bounded repository bytes. S2b's exact
applicability case uses repository reads/in-memory mutations and its D: temporary
fixture. No selected test executes host/account/cgroup/policy adapters.

All CARGO_TARGET_DIR/TEMP/TMP are under
`D:/Code/arxcovo-a12-delivery-source/`; no installs or cache deletion.

Actually passed on Windows, locked/offline Rust1.95.0:

- `cargo test --locked --offline -p arxd --lib exec::a12:: -- --nocapture`:
  7 passed,416 total library tests/409 filtered,0.04s. Expected injected panics
  caught by owned-call tests. Includes real reader short/error/mode/link/size
  controls; duplicate/unknown/missing-source/UID/mode/inode/hash/layout controls;
  root/workspace/artifact/target and runs dev/ino/mode counterfactuals; empty/
  absolute cwd controls; per-call failed recheck prevents effects.
- Exact residue source guard:1 passed; `exec::stage_b_tests::w8_e7`:2 passed.
  Trailing mod.rs A12 declaration and exact inactive inventory unchanged.
- Python A12Delivery:14 passed,0.100s. Covers closed gates, markers/strict summary,
  names/collision, partial-capture quarantine, full mocked preparation/handoff,
  cleanup-before-deletion, bounded descriptor identity/hash, one-build output/
  clock, all four UID fields, local UID/GID aliases/supplementary groups,
  ACK clocks/attribution and source callgraph separation.
- Ten selected A1A2 monitor/cleanup/runtime/closed-selection mock regressions:
  10 passed,0.018s.
- `mutation_check.a12_assertion_mutants()`:7/7 killed (delivery gate, execution
  gate, account-name bounds, pending capture, UID release, build clock, duplicate
  artifact). All mutations in memory; no native calls or file mutations.
- `python -I -B tools/ci/test-a7-admission-readonly.py`:13 passed,1.743s, against
  normalized-LF final driver and the two refreshed wrapper constants.
- Exact S2b test_pinned_manifest_applicability_in_memory_only:1 passed,0.011s.
- `cargo fmt --all -- --check`, Windows workspace/all-targets locked/offline
  Clippy -Dwarnings and `git diff --check`:passed.

Development-only failures were corrected: Clippy's constant assertion needed a
const block; an overbroad text edit caused a Python indentation error; Windows
mock path separators/absent Linux flag constants needed portable fake values;
a UID mutant needed scoping because the historical driver has a similar check.
A unittest discovery pattern selected zero wrapper tests; the explicit script
then ran all13. Initial wrapper constants were recalculated after LF normalization
and its actual13-test suite rerun. No unresolved three-fix failure is claimed.

No Linux compilation was attempted in this slice. Windows Clippy does NOT type
check the cfg(target_os=linux) reader/fixture. Hosted Linux compilation and fresh
independent Opus review of the final source are required before publication.
No actual Runner/LinuxFs/native fixture/helper, host/proc/sys/root/account/sudo/
policy/cgroup action, network, dispatch, variable, push, PR, merge, watch or
subagent action occurred. No native authority, acceptance, A7 retry or counter
reset follows. Hardware remains **unverified on real hardware**; M2 stays open.

### Exact source blobs (mode100644)

| Path | Blob |
| --- | --- |
| crates/arxd/src/exec/disposable/a12.rs | 709010f6c54c2544a0ad7ea10141967e3889a27f |
| tools/unix-exec-validation/driver.py | d49b2a55a2a5080237817fdc7d5c39d7d31af24c |
| tools/unix-exec-validation/test_driver.py | 782447cf3524fe94f8c2b5d88f8cea30628600b3 |
| tools/unix-exec-validation/mutation_check.py | bd66cc186ea380fe3c87beb5009204a179ccf76b |
| tools/ci/a7-admission-readonly.py | 67481533a213989951a3f0881e66cdc88ffaf8e6 |
| crates/arxd/src/exec/mod.rs (unchanged) | b150f543a894d57b25c56bab19c95b4f9bd2beeb |
| crates/arxd/src/exec/unix.rs (unchanged) | 0d087e37f31f3e17db652a87f2a0364eb398479e |

## Coordinator fixes after Opus B1/B2 source review

Review d2ee8e8 retained975f08b blocks initial source for two A12-only findings. B1: the original source.contains assertion matched its own expected string. The corrected test extracts native code before portable tests, checks the actual prepared_delivery body equals closed_read(read_delivery), and requires exactly the reader definition plus that one reference. In-memory direct-reader bypass and added-reader-caller controls must both fail this wiring predicate. No execution/delivery gate is enabled.

B2: embedded Linux debug data may exceed the128MiB cap; this was a source-based risk estimate, not a measured Linux size or explanation of A7 history. build_a12 now overrides CARGO_PROFILE_DEV_DEBUG=0 in its own copied build environment only. Shared cargo_env and the cap remain unchanged. Added mocked Popen environment assertion was observed red(None vs0) before the fix and green after; new in-memory mutation removing the override is assertion-killed. Consumed D7j wrapper changes only its exact driver size/SHA256 maintenance. A7 hosted NO-GO is unchanged.

Actually reran after audited pure/mock callgraphs: all7 portable A12 Rust tests,14 A12 delivery mocks,eight A12 assertion mutants,13 wrapper mocks, then full194 portable guard tests (including legacy/subprocess/feedback fences). All passed; internal/outer maximum757/2447 unchanged. fmt anddiff checks pass. Build/temp outputs D:/Code/arxcovo-a12-delivery-source. No nativefixture/host/root/account/proc/sys/Runner/LinuxFs/helper/sudo/dispatch ran. Full Linux compilation remains a hosted gate; no bounded-size Linux image claim is made before it is observed. Fresh independent Opus delta review precedes publication.

Current corrected source blobs (supersede the earlier table for these paths):

| Path | Blob | SHA256 |
| --- | --- | --- |
| `crates/arxd/src/exec/disposable/a12.rs` | `da5cd1283af919e0c4310a01eb656dfc3cb924f6` | `72601ed15220d6ade109852df6a585ff125f2d26fc932c17be04c3e397fc38e0` |
| `tools/unix-exec-validation/driver.py` | `5a7153595adae3b0adac57d0ea5fb007dfb134d8` | `a8a01bc59d694f0ab2a666a7f0135a1fc3ab46d646214c2644f70f548fb1abf5` |
| `tools/unix-exec-validation/test_driver.py` | `eb3c8430b7d2a3d88876b2ce31d8f66349fa1422` | `8d8061c5618ddcc1482ad3a430efac86d022050e5ac7c17172b8ef5f8f71b0ab` |
| `tools/unix-exec-validation/mutation_check.py` | `3095efd9885c9f63835da0d52953d6835dd9e50a` | `35383821a366c19a8b0e7628eddc29319bf4524f242bdc201ea459eee818adaa` |
| `tools/ci/a7-admission-readonly.py` | `f354bab7fa1b8b04209310886898be847cad5881` | `6be7c142b49c91ccc7c42d4eb88bea8f42e4c3d8428c09debf3e1199f19ddf80` |
