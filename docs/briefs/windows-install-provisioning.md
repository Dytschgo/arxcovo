# Brief: Windows install provisioning and owner entry points (ADR 0016)

Status (2026-11-12): accepted contract only within the settled review
dispositions; E1 library implemented, independent full and follow-up reviews
passed. See [full E1 review](../reviews/0016-e1-independent-review.md) and
[follow-up recheck](../reviews/0016-e1-followup-recheck.md). E1 subsequently merged in PR40 after independent review and hosted gates.
E2 is the separate local layer described below. This brief records
settled design, without adding new decisions. Library only: no CLI, MCP,
`Plan::execute` or activation. Production paths are **unverified on real hardware**.


Publication candidate checkpoint (2026-11-12): actual base is
`2b805c450880fd12b9f0cd9771974e4fa1337a55` after PR45. The six provisioning
files still equal reviewed `1103a66` and tested `000835d`; inherited E1,
N3/N2, artifact CI, SYSTEM test and state-exit changes are preserved. The
[independent assessment](../reviews/0016-e2-current-parent-assessment.md)
permits only a draft/candidate publication recommendation. The coordinator
accepts that limited disposition; merging remains held for exact-head review,
all hosted gates and a separate explicit decision about the unresolved
ExecutionFailed. No pass is a fix or waiver. See
[publication evidence](../evidence/e2-publication-refresh.md). No CLI/native
activation or M2 acceptance; paths remain **unverified on real hardware**.

Historical E2 verification checkpoint (2026-11-12): E1 merged through PR40; PR41 N3,
PR42 N2 and PR43 artifact CI are inherited on base `fbb1813`. E2 provisioning
is now ported locally from reviewed `1103a66`, with private authority fields and
ordering unchanged. Prior [full security review](../reviews/0016-e2-independent-security-review.md)
at `42718df` and [authority correction recheck](../reviews/0016-e2-authority-correction-recheck.md)
at `1103a66` remain historical evidence. Architecture credit stays with the prior
Sol review of the Opus design. E2 remains **HELD** for fresh independent Opus
assessment and coordinator publication disposition: the unexplained uninstrumented
`ExecutionFailed` is unresolved; instrumented passes are neither fix nor waiver.
No CLI wiring or activation. Native/installed/dedicated paths remain
**unverified on real hardware**; M2 incomplete.

## Build order (one concern per PR)

- **PR-E1, narrowing** (`owner_windows.rs`): extract only the token/layout/binary part of ADR 0012 steps 1–3 into
  `OwnerGuard::acquire()`. Both `open(guard)` and `uninstall(guard, proof)`
  retain the existing private-directory `pin_chain` loop before privilege
  enabling, lock acquisition and steps 4–7; keep those pins throughout the transaction and release the lock last.
  Remove `parts()`;
  make `WindowsOwnerStore`/`WindowsBatchRight` private; add the wrappers,
  `uninstall` and `uninstall_flow`. Implement shared `pin_private_dirs` and
  U3 in this PR, not PR-E2. N1–N10 change only their access path.
- **PR-E2, provisioning** (new `#[cfg(windows)] owner_install_windows.rs` +
  tests; after E1): `pin_chain(path, leaf: Option<Policy>)` (`None` = ancestor
  rule on the leaf too); `native::rollback_database_header` → `pub(super)`;
  `service-install.md` Windows step 4 gets ADR decision 9 (**unverified**);
  update `service-foundation.md`. PR-D (CLI) later calls only this API.

## API (exact; private fields; no Clone, Copy or Debug on guard and session)

```rust
pub struct OwnerGuard { layout: Layout, pins: Vec<File> }      // pins: binary chain + image
impl OwnerGuard { pub fn acquire() -> Result<Self, OpenError> } // production only
impl WindowsOwnerSession {
    pub fn open(guard: OwnerGuard) -> Result<Self, OpenError>;  // configure only
    pub fn read_config(&mut self) -> Result<Option<Vec<u8>>, Failure>;
    pub fn observe_batch(&mut self, sid: &str) -> Result<BatchObservation, Failure>;
    pub fn configure(&mut self, req: &Configure) -> Result<(), Failure>;
    pub fn uninstall(guard: OwnerGuard, proof: Deregistered) -> Result<RightReport, OpenError>;
}
// uninstall: proof moved in first; private chains pinned, then steps 4–6
// (shared with open, which adds 7);
// then this test seam runs step 7 (recover) and uninstall_right.
fn uninstall_locked(_: Deregistered, s: &mut impl OwnerStore, r: &mut impl BatchRight)
    -> Result<RightReport, Failure>;
pub(super) fn uninstall_flow<C: ScmCalls, K: Clock, R>(
    scm: &mut WindowsScm<C, K>, then: impl FnOnce(Deregistered) -> R) -> Result<R, Error>;
// owner_install_windows.rs
pub fn install(guard: OwnerGuard) -> Result<(), OpenError>;     // production wiring only
fn install_with<C: ScmCalls, K: Clock>(scm: &mut WindowsScm<C, K>, ns: Namespace,
    policy: &ProtectionPolicy, enable: impl FnOnce() -> Result<(), Error>) -> Result<(), OpenError>;
```

**PR-E1 shared pin phase.** Add one module-private `pin_private_dirs(paths,
policy, check_ancestor) -> Result<Vec<File>, OpenError>`. Both configure and
uninstall call this function with `guard.layout.private_dirs()`, the fixed
production `ProtectionPolicy` and `layout::check_windows_ancestor`. Extract
the existing no-follow structural/identity walk from `pin_chain` so this phase
and the tests use the same code. Parameterize the private leaf policy and
ancestor checker in E1; do not defer this to E2. Production has no runtime
selector. Owned-temp tests pass fixture paths, their fixture leaf policy and
a bounded fixture ancestor checker; they must not implement a second walk.
Missing paths refuse with the existing I/O mapping, never filter/skip/create.
Preserve the original `pin_chain` error layers: structural rejection remains
`Store(RecoveryRequired)`; leaf/ancestor policy rejection is
`Layout(Permissions)`. Private-directory admission maps structural or policy
rejection to `Layout(Permissions)`; U3 asserts that refusal token for both
session paths. Missing paths retain `Store(Io)`.

`Namespace`: production = `{ProgramData}` chain under ancestor policy plus the
guard's binary-directory handle (final check); `#[cfg(test)]` = owned-temp
parent without ancestor policy (like `open_state_fixture`). Production passes
`enable_restore`; tests record `restore` in the call log. Order: ADR decisions
1 (after the guard), 4, 2/5, 6, 7. All handles held until return; lock last.

## Tokens (existing only)

| Condition | Result |
| --- | --- |
| not elevated, or restore enable fails | `Layout(OwnerRequired)` |
| binary dir fails final policy; pinned chain, reparse or nonlocal | the existing `pin_chain` mapping (`Permissions`/`Layout`; structural `Store(RecoveryRequired)`) |
| SCM `Foreign`, active, other SCM error | `Foreign`, `Active`, the unchanged SCM `Error` |
| existing private dir unsafe; WAL/SHM present; state member unsafe, nonempty lock, bad or short header | `Layout(Permissions)` |
| existing `owner.lock` unsafe or nonempty | `Store(RecoveryRequired)` (`with_lock` checks) |
| sharing violation (either lock, database, journal); member create gives 80; other I/O | `Store(Busy)`; `Store(Conflict)`; `Store(Io)` |

## Required tests (Windows, ordinary user, owned temp, fake `ScmCalls`/`Clock`)

- **I1 Fresh:** exactly 3 directories, `owner.lock` and 3 members; each
  fixture-owned, protected, one link, zero bytes. Log: observe, restore,
  creates, register; no start, auto start type or session.
- **I2 No mutation before observe:** `Foreign`, active, each SCM `Err`, and a
  binary-dir policy failure leave the fixture parent listing unchanged, with
  no restore or register.
- **I3 Rerun** after an injected failure following each create, or a register
  `Err`, converges to I1; pre-existing file IDs and bytes are unchanged.
- **I4 Retained:** valid rollback-header database and nonempty journal keep
  bytes, file ID and write time; a missing lock is created.
- **I5 Refusals** (no member created, no register): `-wal`; `-shm` (also as a
  directory); nonempty `activity.lock` or `owner.lock`; WAL header (bytes
  18–19 = 2); garbage header; 1–19 byte database; hardlinked member; junction
  as `state` or a member; foreign ACE on an existing directory or member;
  unprotected existing directory.
- **I6 Busy:** second install; `activity.lock` held share 0; database open for write.
- **I7 Installer restore fence:** the installer existing-object open helper rejects access bits
  outside decision 3; installer files lack `SetSecurityInfo`,
  `SetFileSecurity`, `SetNamedSecurityInfo` and `rusqlite`.
- **I8 Sharing:** reproduce the ADR table (create under read-only share pins
  succeeds; rename inside one gives 32; pinned directories cannot be renamed).
- **I9 Descriptors:** directory use of `PRIVATE_DESCRIPTOR` parses as in N9;
  in-memory SDDL of the decision 9 ACL passes final `BinaryIntegrity`, and
  the same plus `(A;OICIIO;GA;;;CO)` fails.
- **U1/U2:** every `deregister` `Err` → `uninstall_flow` never calls `then`;
  `uninstall_locked` (fixture store, fake right) recovers, then one revoke from `granted`.
- **U3 (PR-E1) Private chains:** invoke the shared `pin_private_dirs` phase
  through the configure/uninstall fixture paths. A missing directory, replaced
  identity, reparse or untrusted parent refuses before cleanup, recovery or
  right calls; assert those call logs stay empty. A safe control reaches a
  fake right callback, where renaming the pinned config directory refuses and
  a second owner-lock open is Busy. After dropping the session, rename and
  lock-open succeed. Mutating away the shared private-pin call must fail U3.
  Review field/drop order separately for lock-last; post-drop success alone
  does not prove that order. Existing checked cleanup/publication tests remain
  unchanged and outside the installer fence. No production guard or LSA call.
- **S1 Production pin call sites:** non-test `pin_private_dirs(` calls occur
  only in `open` and `uninstall`, before `enable_restore` and lock acquisition,
  with production `ProtectionPolicy` and `layout::check_windows_ancestor`.
  Removing either real call or weakening its checker must fail S1, independently
  of fixture calls. Verify source structure, not matching comments or strings.
- **S1 Scan:** no `fn parts`; `uninstall_right(` only in `uninstall_locked`;
  `owner_install_windows*` has no `BatchRight`, `owner_transactions`,
  `grant(`, `revoke(` or `.start(`; `owner_scm*` has no store/right reference.

## Forbidden

No test calls `OwnerGuard::acquire`, `Layout::system`, `WindowsScm::system`,
`NativeScm`, LSA, `enable_restore` or real `ProgramData`/`Program Files`; no
`#[ignore]`. No account, password, S4U retry, prompt, recovery flow, new crate,
`windows-sys` feature or path allowlist. Never touch the old Lab Manager, owner
PC services/rights or the VM. No PROGRESS edits. Native privileged proof only
on a separately reviewed disposable hosted runner.
