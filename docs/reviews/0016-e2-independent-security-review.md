# ADR0016 E2 independent security, standards and spec review

Status: **CHANGES REQUIRED (one blocking spec finding, mechanical fix)**. This is
a code review, not acceptance. It does not approve a merge or grant publication.
Opus 5.5 (claude-opus-5-5), high effort, review sub-agent, 2026-11-11. No
delegation, no subagents. Single reviewer covering both axes. The axes are reported separately.

## Pinned target and scope

- Reviewed head: `d28cbd749035dd62b0fb663ed6567b208bb9361c`
  (`integrate/windows-owner-provisioning`). Review ran in an isolated worktree branch at that commit.
- Full implementation base (E1): `4a5919e61542ee38507a9ba461549a61e4b3a67d`.
  Reviewed `git diff 4a5919e...d28cbd7` in full (13 files) and its 9 commits, not only
  the completion diff.
- Runtime/test source at the head equals `1ba515ef33fd084d6833286ecb4818dffcc9a8f7`.
  `git diff 1ba515e d28cbd7` touches only `docs/PROGRESS.md` and
  `docs/reviews/0016-e1-followup-recheck.md`. Production source equals the
  inherited Sol draft `2fa4d07`/`fb61a88`. Since then, `crates/` changed only in
  `owner_install_windows_tests.rs`.
- Read: AGENTS.md, CONTRIBUTING.md, KICKOFF (rules sections), ADR 0016, the
  install-provisioning brief, the full E2 evidence report, the junction debug
  report/patch, the E1 follow-up recheck, and the relevant `owner_windows`,
  `owner_scm(_windows)`, `layout`, `native` code and `owner_plan` registration.

**Independence.** I independently reviewed all Sol-authored E2 work: production
installer source, `owner_windows`/`owner_scm`/`mod.rs` visibility and `Option<Policy>`
changes, all E2 tests, both guide edits, the evidence report, and the coordinator's
`d28cbd7` integration. ADR 0016 and its brief were authored by Opus. My
conformance checks against them are therefore not independent of ADR
authorship. Sol's retained ADR/correction reviews supply that independence. The
junction fixture patch was Opus-authored. Sol independently reviewed and applied
it as `f635a73`. I only verified that the two are byte-equivalent: `git diff
b5194bf f635a73 -- crates/` is identical to
`docs/reviews/0016-e2-junction-fixture.patch` under `cmp`. I claim no independent
review of that patch.

## Checks actually run (this review)

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: PASS.
- `cargo nextest run --locked --workspace --all-targets --no-fail-fast`:
  **245 passed, 0 failed, 2 skipped**, run `b2f10f0e-6038-4736-a71b-8f9ec3d08cff`
  (10.348 s). This reproduces the builder's 245/2 count (`9bf178c5`). The two
  skipped tests are the existing subprocess-only `#[ignore]` fixtures, which their
  parents exercise. They were not forced.
- Focused junction/control/fence run (I5 junction, N3, S1 installer fences):
  3/3 PASS, run `fe038088-5655-4889-95f9-0670a6eb431d`.
- Grep: no test file calls `OwnerGuard::acquire`, `Layout::system()`,
  `WindowsScm::system`, `NativeScm::new` or `enable_restore()`. The single
  `enable_restore()?` hit is a source-fence string. Installer production code
  contains no name-based mutator (`DeleteFile*`, `MoveFile*`, `RemoveDirectory*`,
  `SetFileInformationByHandle`, `SetEndOfFile`, link creation, `std::fs`
  writes), no truncating disposition and no ACL setter.

### Source mutations (each applied to the real source, focused `service::owner_*` suite, 92 tests, then restored with `git checkout` and a clean `git diff` verified)

| Id | Mutation | Result | Run |
| --- | --- | --- | --- |
| A | `enable()?` moved before `ns.pin()?` (restore before namespace pins) | **survived** 92/92 | `770ac6f5-f733-4135-8520-319fd2d7b764` |
| B | `walk_chain`: drop `&& final_policy.is_some()` (None leaf pinned with write sharing) | **survived** | `e400b317-57a9-4306-aab7-024bb003dd50` |
| C | `Held` fields reordered so `owner_lock` drops first | **survived** (expected; see the evidence's field-order note) | `0931449c-47a4-4944-a084-5f2edf4597f2` |
| D | existing `activity.lock` classified with `FILE_SHARE_READ` instead of 0 | **survived** | `3be08e89-9853-476b-8d00-8c8daafdb032` |
| E | `create()`: remove post-create `policy.check` | **survived** (not representable with a fixture-owner descriptor) | `41bfa308-1d49-4426-9cee-e9ce9d9437db` |
| N | fresh `owner.lock` `CREATE_NEW` with `FILE_SHARE_READ` instead of 0 | **survived** | `c4889e01-5c7c-44c6-a13c-5c2303091540` |
| I | spot-check of the builder's existing-`owner.lock` share mutant | killed: `i3_lock_and_pins_…` assertion (exit 100) | `c0a74ab3-5c86-4486-afc9-2a9593145c3d` |
| P | `binary_check` moved after `scm.observe()` | killed: `s1_installer_source_fences` assertion (exit 100) only | `af9aafc4-13bd-4a78-825e-09930eb60d9c` |

Every kill was an assertion failure, not a build or setup failure. Every
surviving mutant compiled and ran green. The production branch of
`Namespace::binary_check` (the `len-3` index and final `BinaryIntegrity`) has no
runtime path in tests, so no meaningful mutant can be run for it. Its
correctness below comes from code reading only.

### Builder evidence assessed, not rerun

The builder's seven assertion kills, the descriptor-removal setup failure, the
reparse-bit survival (killed only by existing N3) and the strengthened owner-lock
share probe are consistent with the tests as written. I re-ran one of them (I).
Their descriptions are accurate: descriptor removal fails in fixture setup, not
as a post-install kill, and installer-specific reparse isolation is correctly
not claimed. SYSTEM ownership under restore, the effects of restore privilege,
power-loss durability and flush omission remain **unrun**.

## Standards

Sources: AGENTS.md, CONTRIBUTING.md, the brief's API/fence rules, and the Fowler
baseline (judgement only). Tooling (fmt, Clippy `-D warnings`) is green.

No blocking standards breach. Advisories:

- **ST-1 (medium, advisory). `Local` became safely constructible service-wide.**
  `owner_windows.rs:40`: `pub(super) struct Local(pub(super) *mut c_void)`, and its
  `Drop` calls `LocalFree`. Any `crate::service` module can now write
  `Local(ptr)` in safe code, which makes freeing an arbitrary pointer reachable
  from safe code. Previously that soundness boundary was the module.
  The installer and tests only need to read the pointer. Keep the field private
  and add `pub(super) fn as_ptr(&self)`. Fix together with SP-B1.
- **ST-2 (low). Visibility wider than need.** `walk_chain` (arbitrary leaf and
  ancestor closures) is `pub(super)` only for one installer test. `WindowsScm::new`
  is `pub(super)` only for test fakes. `identity` is `pub(super)` for test snapshots.
  `cfg(test)`-gated visibility, or moving the None-leaf test into
  `owner_windows_tests`, would keep the production surface unchanged.
- **ST-3 (low). Possible Duplicated Code.** The installer's `open()`/`directory()` and
  the test `unsafe_directory` repeat the `SECURITY_ATTRIBUTES` + wide-name +
  `CreateFileW(OPEN_REPARSE_POINT|BACKUP_SEMANTICS)` shape of
  `owner_windows::raw_open`, so the no-follow flags are maintained in two places.
  The shared `structural` reparse check is the backstop. A distinct installer error mapping is a
  legitimate reason for a separate function.
- **ST-4 (low). Possible Primitive Obsession / Repeated Switches.** Member
  semantics are keyed by index (`i == 0` lock share 0 and must be empty, `i == 1`
  header). `if i == 0 { 0 } else { FILE_SHARE_READ }` appears twice
  (`owner_install_windows.rs:162,187`). A small per-member table (name, share,
  kind) would remove the index coupling.
- **ST-5 (info). Possible Mysterious Name.** `READ` (requested access) and
  `READ_ONLY` (fence mask) are easy to confuse. `FENCE_MASK`/`EXISTING_ACCESS` would read better.
- **ST-6 (info).** `pub(super) use owner_transactions::Failure` and the
  test-helper placement of `SetNamedSecurityInfoW` in `owner_windows_tests.rs`
  keep forbidden tokens out of the installer files. Both are semantically harmless
  (an error enum, and an owned-temp test helper) and are disclosed in the evidence.
  They are recorded here so later fence tightening does not mistake them for gaps.

## Spec (ADR 0016 decisions and brief)

### Blocking

- **SP-B1 (blocking, low effort). `OwnerGuard` fields are `pub(super)`, which
  violates the brief's exact API ("private fields") and is not recorded as an ADR
  deviation.** At `owner_windows.rs:361-363`, `pub(super) layout`/`pub(super) pins`
  makes `OwnerGuard { layout, pins }` constructible, and `pins` mutable, from every
  `crate::service` module, including the agent-facing `application` runtime. A
  sibling can now build a guard that skips `owner_required`, the binary pin and the
  running-image identity check, then pass it to `WindowsOwnerSession::open` or
  `uninstall`. It can also reorder or clear `pins`, which breaks the `len-3`
  invariant that `binary_check` relies on. It stays unforgeable outside
  `service`, and no code currently forges or mutates a guard. Practical exposure
  in an installed service is reduced because the registered required privileges
  omit SeRestore (**unverified on real hardware**). The E1 type-level property
  is still weakened, without the ADR record AGENTS.md requires.
  **Requirement:** restore private fields. Expose only narrow read or consume
  methods from `owner_windows`, for example a binary-directory accessor computed
  next to `acquire` and a consuming method yielding layout and pins for the
  installer. Neither adds authority. Alternatively, record the deviation in ADR 0016
  with rationale. Either way, the source-fence suite should assert that no
  `OwnerGuard {` literal appears outside `owner_windows.rs`.

### Verified as specified (code reading plus the tests and runs above)

- **D1, guard → binary → observe → mutate.** `install()` builds
  `WindowsScm::system` lazily (`NativeScm::new` opens nothing), then `install_with`
  runs `binary_check`, then `observe()`. Only `Absent` or
  `Ours{active:false}` proceeds. `Foreign` and `Active` map as specified, and SCM
  `Err` passes through unchanged. Nothing is created, write-opened or locked earlier.
  Ordering is enforced by S1 (mutant P killed). Production `binary_check`:
  `acquire` pushes one pin per path component from the drive root through `arxd.exe`
  (`walk_chain` skips only empty components, which do not occur for an absolute
  known-folder path), then exactly one running-image pin. `pins[len-3]` is
  therefore the binary's parent directory. It gets `structural(directory)` plus
  fixed `Policy::BinaryIntegrity` (the `policy` argument is ignored in production,
  so no selector exists), and `checked_sub` refuses underflow. That invariant holds
  by local reading only (see SP-B1, SP-A4).
- **D2/D3, descriptor at creation and the read-only fence.** Every
  `CreateDirectoryW`/`CREATE_NEW` call carries `creation_descriptor()` in
  `SECURITY_ATTRIBUTES`. `sd`, `sa` and the wide name all outlive the call. No ACL
  setter exists. `existing()` checks the mask before any native call, so an absent
  path never reaches `CreateFileW` with a forbidden bit (I7 checks all 32 bits).
  Only `CREATE_NEW` handles get `GENERIC_WRITE`, and none request `DELETE`. Existing
  objects are never truncated, renamed or deleted. `enable()` runs after binary
  check, observe and namespace pins, immediately before the first
  `CreateDirectoryW`, and is never disabled.
- **D4, fixed names and known folder.** `Layout::system` derives ProgramData from
  `SHGetKnownFolderPath(FOLDERID_ProgramData)` with no environment or runtime
  override. `pin_chain(ProgramData, None)` applies the DRIVE_FIXED root check and
  no-follow opens, and checks every component including the leaf with
  `check_windows_ancestor`, with read-only sharing. The installer's
  `Arxcovo`/`config`/`state` names equal `Layout::windows` `private_dirs`, and
  `config\owner.lock` equals the session's `config().parent()/owner.lock`. Root and
  state pins share read only. Config shares read and write. `Option<Policy>`:
  `Some` behaviour is identical to E1 (shared-walk and S1/U2/U3 tests stay green),
  and `None` forces directory type and the ancestor rule. It cannot relax image
  type or final policy because the only `None` caller is ProgramData.
- **D5, owner.lock.** It is created after `config` with share 0, the descriptor,
  a handle check and a flush. On 80 the existing lock is opened with `READ`,
  share 0. The policy, single-link and zero-length checks map to
  `Store(RecoveryRequired)`. A sharing violation gives `Busy`. A directory,
  hard-linked or nonempty lock gives `RecoveryRequired`, with no member and no
  register (observed in tests, not inferred from Win32 codes). `state` is created
  only afterwards.
- **D6, state.** The WAL/SHM pre-check accepts only `Ok(None)`. Every other
  outcome, including directories, reparse points, `Busy` and `Io`, maps to
  `Permissions`. All members are classified before any missing member is created,
  in lock, database, journal order. `activity.lock` is opened with share 0 and must
  be empty. The database and journal use read sharing. A nonempty database must be
  at least 20 bytes and passes a fixed 20-byte `read_exact` into the shared
  `rollback_database_header`. A nonempty journal is kept. A create returning 80
  gives `Conflict` and is not opened. Created members are policy-checked on their
  own handle and flushed. The sidefile check runs again before `register`. No
  SQLite or schema access occurs.
- **D7, register last.** `scm.register()` is the final statement, with `Held`
  alive. Field order members → pins → owner_lock makes the lock drop last on
  success and on every error after acquisition. The fake panics on start, stop,
  delete and poll, and `set_start` asserts Demand. The installer has no
  recovery, LSA or right reference (S1).
- **E1 narrowing intact** except for SP-B1: no `parts()`. The concrete store
  and right stay private, the pin error layers and the U3/S1 call-site fences
  pass, and the guard has no Clone, Copy or Debug.
- **Tests I1–I9** call the real `install_with` with fake `ScmCalls`/`Clock`, an
  `enable` callback and owned-temp fixtures. I1 checks the exact log and listings.
  I2 covers the exact error layers with restore panicking. I3 covers seven
  create prefixes plus a register retry, with identities for directories,
  owner.lock, database and journal, and lock/rename coverage at the boundaries.
  I4 checks retention of bytes, ID and write time. I5 covers 27 state cases plus
  ACL/type, junction, late-WAL and owner.lock/WAL-directory tokens. I6 covers
  `Busy` for four holders. I7 covers mask, error-mapping and member-race cases.
  I8 covers the sharing table, and I9 the in-memory descriptors. The policy
  fixture owner is `cfg(test)` only, and the production `Namespace` has a single
  variant.

### Advisories (non-blocking)

- **SP-A1 (low). Ordering and share-mode test gaps.** Mutants A, B, D and N
  survive. Add source-fence orderings `ns.pin()?` < `enable()?` < first
  `directory(`. Assert a None-leaf child rename gives 32. Assert that a reader
  sharing read and write cannot open a held fresh `owner.lock`, and that a
  read-sharing reader blocks `activity.lock` classification.
- **SP-A2 (low). Second fence narrower than its comment.** The `open()` fence
  checks only `OPEN_EXISTING`. A future caller passing `TRUNCATE_EXISTING`,
  `OPEN_ALWAYS` or `CREATE_ALWAYS` with write access would bypass it, which
  contradicts "a future caller cannot bypass existing's access fence". Restrict
  `disposition` to `{OPEN_EXISTING, CREATE_NEW}`, or fence every non-`CREATE_NEW`
  disposition.
- **SP-A3 (low). Source-fence breadth.** It lists `SetFileSecurityW` and
  `SetNamedSecurityInfoW` but not the `A` variants, and no name-based mutators.
  `Storage::FileSystem::*` is glob-imported, so `DeleteFileW`, `MoveFileExW`,
  `RemoveDirectoryW` and `SetFileInformationByHandle` are in scope without a new
  `use`. None are present today. Prefix matching or an explicit mutator list
  would make decision 3 mechanically enforced.
- **SP-A4 (low). Production `binary_check` has no runtime coverage.** The I2
  "binary" case exercises the `cfg(test)` fixture variant, which uses fixture
  private-directory policy rather than `BinaryIntegrity`. The evidence wording
  "the final binary check" should say so. With SP-B1's accessor, an owned-temp
  pin vector (a file at `len-3` versus a directory) could distinguish an index
  error by token (`RecoveryRequired` versus `Permissions`) without any production
  constructor.
- **SP-A5 (info). Post-create handle check (mutant E) and lock-last order
  (mutant C)** are confirmed by reading only. The fixture cannot produce a created
  object whose descriptor the policy rejects, and drop order is not observable
  after return. Both are disclosed honestly in the evidence. E1-R1 (`pins` rebinding
  in `open`, which S1 does not catch) remains an open optional advisory.

### Guides and evidence (precisely reviewable; advisories)

- `service-install.md` step 4 is clearly marked **owner-only, DOCUMENT ONLY,
  unverified on real hardware**. It uses fixed SIDs, says agents must not execute
  it, says copy rather than move, and notes that the future CLI is unavailable.
  The recipe was not executed (by me or, per the evidence, the builder). The icacls
  grants are equivalent to the SDDL that I9 accepts. Three precision advisories:
  1. "The final directory policy must reject those grants" is overbroad.
     `BinaryIntegrity` accepts untrusted ACEs without write-type bits, so it
     rejects write-capable grants (and inherit-only CREATOR OWNER GA), not every
     unrelated grant.
  2. The resulting owner is unstated. `BinaryIntegrity` requires a trusted owner,
     and I9 assumes `O:BA`. If the host's default owner for elevated admins is the
     individual account, install refuses, which fails closed. Say so.
  3. Tell the owner to stop if `New-Item` fails (directory exists) rather than
     continuing to `icacls`. Pasted PowerShell continues after non-terminating errors.
- `service-foundation.md`: the current API/limits section is accurate. It marks
  earlier sections historical and requires review and hosted gates first.
- The evidence report is accurate where checked (test contents, mutant
  descriptions, the 245/2 count, field order, owner.lock-directory token). Its
  first "Files and callable API" section still says the junction test "currently
  fails". Only the remaining-work list is explicitly superseded. A one-line
  historical marker would help. SP-A4's I2 wording applies.
- PROGRESS (`d28cbd7`) claims match the source and evidence I checked.

## Limitations

- Every installed, SYSTEM-owner-under-restore, real SCM, known-folder ProgramData,
  Program Files ACL recipe, service-token privilege and dedicated-account path
  is **unverified on real hardware**. No production guard, layout, SCM,
  privilege, LSA, owner/admin, VM or Lab Manager operation ran. No owner command,
  native experiment or S4U probe ran. M2 stays open. This review is not acceptance.
- Win32 behaviour claims are bounded to this host's owned-temp NTFS runs and code
  reading.
- Tests ran on one Windows 11 host. Hosted required gates have not run on this head.
- The public queue pause (4 PRs) is unchanged. No push, PR, status, approval,
  merge, watch or dispatch was performed. Only this report is committed. All
  mutated sources were restored byte-for-byte (`git diff` clean after each).

## Summary

Standards: 0 blocking, 6 advisories. Worst: ST-1, `Local`'s raw pointer is
constructible service-wide, so `LocalFree` of an arbitrary pointer is reachable
from safe code.
Spec: 1 blocking, 5 advisories plus guide/evidence notes. Worst: SP-B1, `OwnerGuard`
fields are `pub(super)`, which breaks the brief's private-field API and makes the guard
constructible and its pins mutable across `crate::service`.
