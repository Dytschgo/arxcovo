# ADR0016 E2 local implementation evidence and stopped handoff

Status: **local completion recorded below; independent full E2 review pending; do not publish or wire CLI**. Historical stopped-draft evidence follows. Sol / gpt-6.1-sol,
high effort, implementation sub-agent, 2026-11-11. No delegation.

Exact starting HEAD: `4a5919e61542ee38507a9ba461549a61e4b3a67d`.
Implementation HEAD: `2fa4d076a25728dbc1912b690ba781b8f7d3f5b9`.
Parent context: `review/windows-install-contract`. Initial worktree was clean.
E1 had the reported independent review at `31b939c`, with coordinator follow-ups
through the starting HEAD. This report does not grant exact-head merge approval.

## Files and callable API

- `crates/arxd/src/service/owner_install_windows.rs`: new Windows-only public
  `install(OwnerGuard) -> Result<(), OpenError>`; private generic
  `install_with<C: ScmCalls, K: Clock>(..., Namespace, &ProtectionPolicy, enable)`.
  Production Namespace consumes the binary guard, checks its binary-directory
  handle with final BinaryIntegrity, observes SCM before mutation, and pins the
  known-folder-derived ProgramData chain. The fixture variant is cfg(test) only.
  Creation uses explicit descriptors; existing opens pass the read-only mask
  fence. Owner lock precedes state creation; existing state classification
  precedes missing-member creation; SCM register is last. Held field order is
  members, pins, owner lock. No CLI, activation, schema or SQLite initialization.
- `owner_install_windows_tests.rs`: 12 ordinary-user owned-temp tests covering
  fresh order, refusal-before-restore, create-prefix and registration retries,
  retained database/journal bytes/IDs/write times, malformed headers, locks,
  sidefiles, hardlinks, ACL/type refusals, sharing, descriptors and None-leaf
  ancestor dispatch. The junction test currently fails during fixture setup.
- `owner_windows.rs`: narrow sibling visibility for policy/descriptor/structure,
  guard layout/pins and restore helper; `pin_chain`/shared walk now take
  `Option<Policy>`. None applies the ancestor rule at the leaf with read-only
  sharing. Image structural checks remain file checks. Concrete store/right
  adapters remain private, guard still has no Clone/Copy/Debug.
- `owner_windows_tests.rs`: adapt the existing shared-walk test to Some; add
  lexer-based installer source fences and a fixed owned-temp unprotected-DACL
  fixture helper. The latter is outside the installer files, cfg(test) only;
  installer production has no ACL repair API.
- `owner_scm.rs`: constructor is pub(super); test-only mutable fake-call accessor.
- `mod.rs`: cfg(windows) module export.

`native::rollback_database_header` was already pub(super) at the exact base.
It is reused directly, with a 20-byte read for a nonempty database. No predicate
copy, dependency, windows-sys feature, error variant or authority was added.

## Checks actually run

- `cargo fmt --all` and `cargo fmt --all -- --check`: PASS at final source.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo nextest run --locked --workspace --all-targets --no-fail-fast`:
  **240 passed, 1 failed, 2 skipped**, run
  `8ad909fd-17a6-4ec7-925e-dd326ec3db26`, 11.189 seconds test runtime.
  All existing E1 tests, including S1/U2/U3, passed. The existing ignored
  fixtures were not invoked directly; normal parent tests retained control.
- Focused initial seven-test provisioning run: 7 PASS. Expanded 13-test run:
  11 PASS, ACL-fixture and junction-fixture setup failures. After constructing
  an actually unprotected directory through the test-only helper, ACL refusal
  passed. Final full run has only the junction setup failure.
- `git diff --check`: PASS before implementation commit. Strict UTF-8 decode
  and no-BOM check: PASS for eight owner/SCM/module source files.

No meaningful E2 guard mutation/red-green experiment was completed. The failing
fixture setup is not evidence that the junction refusal works.

## Three-fix stop and next-provider evidence

Failing test: `service::owner_install_windows::tests::i5_junction_state_and_member_refuse`.
Failure occurs at its mklink setup assertion, before install is called:
`owned-temp junction setup failed: exit=Some(1), syntax=true, denied=false`.

Three attempted fixture corrections did not resolve it:

1. Relative link argument beneath the owned-temp working directory.
2. Normalize forward slashes in command arguments to Windows separators.
3. Set working directory to the link parent and pass only the leaf name.

Last focused run `2ea88e41-1946-4b4b-b923-3b3ad076646d`: existing
`owner_windows::tests::n3_junction_parent_refuses` PASS; new I5 junction setup FAIL.
Final full run reproduces the new test failure. No fourth correction attempted.
The final raw_arg format visibly contains `\\\"` in the Rust source around the
target. Compare its literal runtime quoting with the passing E1 fixture before
attempting the next correction. This is a next-provider inspection lead, not a
verified diagnosis. No raw fixture paths or command stderr were logged.

Per the explicit three-fix rule, implementation stopped and this local draft is
handed back for the other provider. No agent was spawned and no publication or
external write occurred.

## Remaining work and limits

- Resolve junction fixture setup and actually exercise both state/member refusals.
- Complete meaningful E2 guard mutations and restore all mutations.
- Strengthen any remaining exact-token/error-boundary and retained-owner-lock
  identity assertions needed by independent review. Current retained snapshots
  cover database/journal and surviving state members; they do not establish all
  directory/owner-lock identities across every crash prefix.
- Update `docs/service-install.md` Windows step 4 with the explicit OWNER-only
  binary-directory ACL prerequisite, labelled unverified; update the current
  callable API/status in `docs/service-foundation.md`. These requested guide
  changes were not made before the stop condition.
- Obtain independent Opus review and green hosted required gates before any
  publication or CLI work. E1 exact-head follow-up is still pending.

No production constructor, SCM/LSA/restore call, binary-folder ACL recipe,
manager/account/service operation, VM, old Lab Manager, system ProgramData or
Program Files fixture was used by the new tests. Full-suite existing tests ran
normally. PROGRESS, scheduling, CI gates and publication queue were untouched.
No push, PR, merge, watch or dispatch occurred. M2 remains open; no third S4U probe.

All privileged native provisioning/SCM behavior is **unverified on real hardware**.
Current-user fixture ownership and in-memory SYSTEM descriptor parsing cannot
prove SeRestorePrivilege accepts SYSTEM ownership, production known-folder
behavior, durability under OS/power loss, or installed service acceptance.

## E2 completion disposition (Sol high, 2026-11-11)

This section supersedes the stopped-draft remaining-work list above. It is local
implementation evidence, **not independent review of Sol's E2 source**, merge
approval or installed acceptance. Exact task base:
`b5194bf21154740b35800b300a4d9a59b0480ac8`, local branch
`t3/windows-provisioning-draft`. Inherited E2 runtime/test source equals the
incomplete `fb61a88` draft on E1 `4a5919e`; coordinator-supplied E1 follow-up PASS
is report `fa6f297`. Optional S1 pins.clear advisory was not changed.

### Patch review and source commits

Sol independently reviewed the **Opus-authored fixture patch only** against the
report and both test builders. It changes the cmd.exe image path to separate
System32/cmd.exe joins; the raw command arguments, loops and assertions are
unchanged. The diagnosis's argv parsing explanation is inferred from its probes;
the path-only fix matches the passing N3 builder. No production behavior changes.
`git apply --check` passed, then the exact reported I5/N3 expression passed 3/3:
`09138e88-a5ef-41b9-85c0-0fe6f7225056`,
`2c066da7-4cab-4a2e-8b80-1f84b87bdf0f`,
`5f87e3ad-3dc1-4556-967d-9c18dc57dc64` (2/2 each).

- `f635a73`: test-only junction image-path correction.
- `d63175e`: complete local E2 boundary/retention tests.
- `8e06eab`: owner-only binary ACL prerequisite and current API/limits guides.

Files changed in this completion: `owner_install_windows_tests.rs`,
`docs/service-install.md`, `docs/service-foundation.md`, and this report.
Production source is unchanged from the exact task base. API remains
`owner_install_windows::install(OwnerGuard) -> Result<(), OpenError>`; private
`install_with` and cfg(test) owned-temp Namespace are unchanged. No dependency,
windows-sys feature, schema, error token, runtime selector, CLI or authority added.

### I1-I9 disposition

- I1 verifies all three directories, owner.lock and triplet through policy/structure
  (owner, protected DACL, file single-link checks), exact listings, empty files,
  and the complete observe/restore/create/register log without extra operations.
- I2 asserts exact Foreign/Active/Manager/Permissions layers for fake observation
  failures and the final binary check. Restore panics if reached; the parent has
  no Arxcovo entry and no register occurs. Restore failure separately asserts
  OwnerRequired before creation. The None-leaf ancestor rule is exercised.
- I3 fails once after each of seven real creations with Store(Io), then retries
  exactly once. It compares surviving file bytes, IDs and write times, including
  owner.lock, plus every surviving directory ID. Register Manager failure/retry
  preserves all four file snapshots and all three directory IDs. Directory write
  times are deliberately not claimed unchanged: adding children changes them.
  New callbacks check exclusive owner lock and non-renamable root/config/state
  at state/member creation and registration, including reinstall. Field order
  was read separately: Held declares members, pins, then owner_lock, so lock drops
  last; post-return success alone is not the drop-order proof.
- I4 preserves the valid rollback-header database (including tail) and nonempty
  journal bytes/IDs/write times, and creates the missing activity.lock.
- I5 asserts exact Permissions for WAL/SHM (including directories), malformed/WAL
  headers, every short length 1..19, nonempty activity.lock, hardlinks, unsafe
  directory/member ACLs/types, and both junction placements. Unsafe/nonempty
  owner.lock asserts Store(RecoveryRequired), including hardlink/directory cases.
  No state member or register occurs on refusal. A callback introducing WAL
  after final member creation proves the second sidefile check blocks register.
- I6 checks exact Busy for held owner.lock/activity.lock and writable database
  **and journal**, then one successful install after releasing each blocker.
- I7 checks every forbidden mask bit before absent-object lookup, the second
  open-helper fence directly, real missing-parent Io, sharing Busy, CREATE_NEW
  80 Conflict, and a real member race after classification: creating the database
  after activity.lock yields Store(Conflict), leaves journal missing and never
  registers. Existing lexer-based source fences remain green in the normal suite.
- I8 retains the actual owned-temp pin-sharing table (create, rename-inside and
  rename-directory), exact Win32 32 checks and success after dropping pins.
- I9 retains in-memory production private-directory descriptor and final binary
  ACL checks, including rejection of inherit-only CREATOR OWNER full control.

### Actual source mutations (each restored)

Each row changed the true installer/shared structural source, compiled it and
ran the selected real test. No assertion was changed for a mutant. Exit 100
below means a compiled test assertion failure, not a build failure. The runner's
FAIL lines are repeated in its summary; those repeats are not additional kills.

| Mutant | Test/result | Run |
| --- | --- | --- |
| Remove installer OPEN_REPARSE_POINT | I5 junction assertion fails | `4d173114-a756-4a67-962a-bf3b5baea11b` |
| Remove existing member policy check | I5 ACL/type assertion fails | `9cb547e7-eba9-4b28-8dea-26f145c023ab` |
| Remove directory policy check | I5 ACL/type assertion fails | `701f096e-af73-4839-98a2-d5f6b8f6a77e` |
| Remove rollback header predicate | I5 unsafe-state assertion fails | `7a124bd5-5db9-4195-badc-3e2cc00c2236` |
| Disable both read-access fences | Both I7 tests fail assertions | `e8be9b83-7a6d-4d82-ba1f-ff7db5515f00` |
| Remove both sidefile checks | I5 initial and late-sidefile assertions fail | `09c50865-2f15-4292-95fb-cf2898dcd50c` |
| Remove creation descriptor | I1 real Fixture directory creation policy check fails during setup | `a3868f92-2dfb-4354-9aba-2d6fc5f8ab6a` |
| Existing owner.lock share 0 to read sharing | Initially survived (2 tests green); strengthened reinstall shared-reader probe then fails at its Busy assertion | `52bed56e-7e6d-4c22-baae-82d71ca3a52d` then `aef435c8-5a4d-46cf-8a55-7da6086451c8` |
| Remove only shared structural reparse-attribute predicate | I5 green; N3 explicit structural assertion fails | `1686909f-35bb-4414-9c83-445aeafacbba` |

Reparse disposition: retain the strict policy and existing N3. I5 genuinely
exercises rejection, but does not isolate the reparse attribute branch: member
type checks and state ACL/owner checks also reject these cmd-created junctions.
N3 directly pins the shared predicate. No stronger reparse-only E2 fixture was
added: the allowed cmd fixture does not establish a policy-valid no-follow
junction descriptor, and weakening policy would defeat the test's purpose. Do
not claim installer-specific isolation of that bit. No fake security assertion
or widened fixture authority was introduced.

The creation-descriptor mutant is a **setup policy rejection**, not an I1
post-install assertion kill; it cannot isolate production SYSTEM ownership under
the current-user fixture. A selective production descriptor-ownership mutant,
restore privilege effects, filesystem name durability and flush omission are
**unrun/unrepresentable here**, not mutation kills. SYSTEM ownership is parsed
in memory only. No attempt was made to simulate power loss. All mutated files
were restored byte-for-byte; final diff contains no production changes.

The first strengthened focused run passed 15/15 (`40ec6f13-14b5-46a8-ac22-c38e653dd7dc`);
the improved lock/registration snapshots passed 15/15
(`a404f7e3-ddba-44b8-bac9-8a0ed4dc3026`). The later owner-lock/WAL-directory test
is covered by the final normal workspace run below. These are deliberate runs
for new assertions, not retries to hide failures.

### Safety and review boundary

Only ordinary-user owned-temp fixtures, fake SCM/clock/enable callbacks, and
in-memory descriptors were added/run directly. No production guard, fixed
layout, SCM constructor/query, LSA, privilege enable, install, admin command,
private-state JSON, VM, old Lab Manager, system ProgramData/Program Files fixture
or real service pipe connection was invoked by the new tests. The normal workspace
suite ran existing parent-controlled fixtures normally; ignored children were
not forced. No new ignore, skip, suppression, permission repair or production
policy weakening. The guide commands were **not executed**. PROGRESS belongs
to the coordinator and was untouched; no remote Git/GitHub write, scheduling,
watch, dispatch or publication occurred.

Full independent Opus security review must cover **all inherited Sol E2 runtime
source**, the completed tests and guide changes at final head; Sol's narrow
review of the Opus patch is not that review. Green hosted required gates are
still mandatory before publication/CLI. M2 remains open. Every installed,
SYSTEM/SCM/known-folder/dedicated-account path is **unverified on real hardware**.

### Final checks and coordinator inspection note

Final source is `8e06eab` (test commit `d63175e`, patch `f635a73`); this
report-only commit adds evidence without changing tested source.

- `cargo fmt --all`, then `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS
  (43.14 seconds in this checkout).
- One final `cargo nextest run --locked --workspace --all-targets --no-fail-fast`:
  **245 passed, 0 failed, 2 skipped**, run
  `9bf178c5-dced-452e-ab72-efbac9f2139d`, 14.070 seconds test runtime.
- `git diff --check` and strict UTF-8 decode/no-BOM checks: PASS for all four
  changed files against the exact task base (including this evidence update).
- Production installer and owner source compare unchanged to the task base;
  no mutations remain. No Node/Python suites were run (neither was changed).

The coordinator's owner.lock-directory inspection note was already covered by
`i5_unsafe_owner_lock_and_wal_directory_tokens`, added before that note. It
creates the pre-existing directory in owned temp and calls the real installer
seam, asserting **Store(RecoveryRequired)** exactly, no member creations and no
registration. The final workspace run passed that test. This is observed token
evidence, not an inference about a Win32 code. No production change or repair
authority was needed on this host.
