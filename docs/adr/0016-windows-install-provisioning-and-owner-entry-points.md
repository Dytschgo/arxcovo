# 0016: Windows install provisioning and narrowed owner entry points

Status: accepted contract within the settled independent review dispositions;
E1 library implemented locally and independently reviewed (full report
[42c923d](../reviews/0016-e1-independent-review.md), follow-up
[fa6f297](../reviews/0016-e1-followup-recheck.md)); hosted gates pending.
This port was prepared on main `0937cac` and refreshed onto `79b7879`.
Opus high integration review at `b79786c` passed; published-head hosted gates remain pending. E2 provisioning and CLI wiring are not part of this E1 boundary.
The original contract base was `54c43fbcc9b9a6050f067a3e437e58a018b96b00`;
[Sol contract review](../reviews/0016-windows-install-contract-review.md)
and [correction review](../reviews/0016-provisioning-correction-review.md)
remain retained. No new design is introduced. Every production path remains
**unverified on real hardware**; M2 and the dedicated-account blocker stay open.

Amends ADR 0009 (Windows install, "identical-only" idempotency) and ADR 0012
(session preconditions, `parts()`, PR-D). Windows counterpart of accepted ADR
0014 decision 6 (pinned `1e1e48b:docs/adr/0014-unix-owner-lifecycle-corrections.md`,
not in this base). ADR 0012's configure/recovery/uninstall fault model
(decision 6) is unchanged. ADR 0015 is macOS-only.

## Problem

- `WindowsOwnerSession::open()` needs existing private directories and
  `owner.lock`, so fresh install cannot use it; nothing creates them or the
  state triplet `native::state_guard` requires. `parts()` lends the raw right
  adapter, so wiring could revoke without the SCM `Deregistered` proof.
- A `Program Files\Arxcovo` with the stock inherited ACL fails startup's final
  `BinaryIntegrity` check: that policy does not ignore the inherit-only CREATOR
  OWNER full-control ACE (code reading of `layout::check_protection`; ACE from
  ADR 0013). Install checks it only as an ancestor, so it would register a
  service that cannot start.

## Decisions

1. **Guard, then observe, then mutate.** `OwnerGuard::acquire()` is exactly
   the binary-only part of ADR 0012 session steps 1–3 (elevated, Administrators enabled,
   `Layout::system()`, `pin_chain(binary, BinaryIntegrity)`, running-image
   `FileIdInfo` equal, image held with read sharing only); no privilege, lock
   or creation. Install then checks the pinned binary-directory handle under
   final `BinaryIntegrity`, as startup `observe_layout` does (install only),
   and calls `WindowsScm::system(layout).observe()`: `Absent` or
   `Ours { active: false, .. }` proceeds; `Foreign` → `Foreign`; active →
   `Active`; `Err` unchanged. Nothing is created, write-opened or locked before.
2. **SYSTEM owner at creation, by explicit descriptor.** Every new directory and
   file gets the existing `PRIVATE_DESCRIPTOR`
   (`O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)`) in `SECURITY_ATTRIBUTES` on
   `CreateDirectoryW` or `CreateFileW(CREATE_NEW)`. SYSTEM is neither the user
   nor an owner-capable group of an elevated admin token, so `TokenOwner`
   cannot select it and `SeTakeOwnershipPrivilege` only takes ownership for
   the caller; `SeRestorePrivilege` allows any valid owner SID. The existing
   `enable_restore` (`ERROR_NOT_ALL_ASSIGNED` fails as `OwnerRequired`) runs
   after decision 1 and the namespace pins, immediately before the first
   create, and stays enabled until exit. The protected DACL blocks inheritance
   from `ProgramData`. Each created object is re-checked on its handle
   (`PrivateDirectory`/`PrivateFile`, one link for files). There is no
   post-create fix: no `SetSecurityInfo`, `SetFileSecurity` or `SetNamedSecurityInfo`.
3. **Installer restore hazard fence (new strictness).** This decision applies
   only to provisioning during install. It does not replace ADR 0012
   configure/uninstall staging cleanup or compare-and-replace publication.
   With restore enabled,
   backup-semantics opens bypass the DACL for write-type access. Opens of
   pre-existing objects request only `READ_CONTROL`, `FILE_READ_DATA`,
   `FILE_READ_ATTRIBUTES`, `GENERIC_READ` and `SYNCHRONIZE`. Write or delete
   access exists only on handles from this call's own `CREATE_NEW`. Existing
   objects are never repaired, truncated, renamed, deleted or taken over.
4. **Create-or-verify, fixed names only.** `{ProgramData}` (known folder) and
   its ancestors are pinned under `check_windows_ancestor`, read sharing only.
   Then `Arxcovo`, `Arxcovo\config` and (under the lock, decision 5)
   `Arxcovo\state` each get `CreateDirectoryW`; on success or
   `ERROR_ALREADY_EXISTS` (183) the path is opened no-follow and must pass
   structural and `PrivateDirectory` checks, else install refuses. Pins share
   read only, except `config` (read and write, as in the session, so a
   configure rename is never blocked); never delete. No new path, allowlist
   or policy.
5. **Serialization.** Directly after `config`, install acquires
   `config\owner.lock`: `CREATE_NEW` with share mode 0 and the descriptor, then
   the same checks as `WindowsOwnerStore::with_lock` (policy, one link, zero
   length) and `FlushFileBuffers`. On `ERROR_FILE_EXISTS` (80) it opens the
   existing lock with share mode 0. A sharing violation gives `Busy`. All later
   creation, including the `state` directory, happens under this lock, which is
   held with every pin until install returns.
6. **State provisioning** (ADR 0014 decision 6 classes, Windows checks), in
   the pinned `state` directory:
   - `activity.sqlite3-wal` and `-shm` must each give `ERROR_FILE_NOT_FOUND`
     on a no-follow open; anything else is `Permissions`.
   - Each present member is opened read-only, no-follow, and checked for
     structure, `PrivateFile` and one link: `activity.lock` with share mode 0,
     database and journal with read sharing only. A sharing violation is `Busy`.
   - `activity.lock` must be empty. A nonempty database must be at least 20
     bytes, and its first 20 bytes (bounded loop) must pass the existing
     `rollback_database_header`. A nonempty journal is kept.
   - All members are classified (Fresh, Empty partial, Retained) before any is
     created. Only missing ones are created, in the order lock, database,
     journal: `CREATE_NEW`, descriptor, zero length, handle check,
     `FlushFileBuffers`. A create returning 80 is `Conflict` and is not opened.
     WAL/SHM absence is then rechecked.
   - No SQLite open, header, schema, journal mode or host identity
     (`Activity::open_service` does that at first start). No history deletion.
7. **Registration last; never start.** `WindowsScm::register()` runs only
   after decisions 1–6, with every pin and lock held; its own checks still
   refuse `Active`/`Foreign`. Install never starts, never sets
   `SERVICE_AUTO_START`, and never opens a session or runs recovery.
8. **Owner entry points replace `parts()`**, which is removed;
   `WindowsOwnerStore` and `WindowsBatchRight` become module-private.
   - `WindowsOwnerSession::open(guard)` (configure only) first pins every
     `layout.private_dirs()` chain under `PrivateDirectory`, retaining
     all ancestor and leaf handles. This completes ADR 0012 step 2, which the
     binary-only guard cannot perform on a fresh install. It runs before
     enabling restore and before any lock, staging, record or right access.
     Missing directories refuse; they are never skipped or created by a session.
     It then runs ADR 0012 steps 4–7 and offers `read_config`, `observe_batch(sid)` (the opaque
     `BatchObservation`, built only by `owner_transactions::observe_batch`)
     and `configure(&Configure)`.
   - `WindowsOwnerSession::uninstall(guard, proof: Deregistered)` takes the
     non-Clone proof by value first, then performs the same private-directory
     pinning before steps 4–7 (privilege, lock, staging cleanup, `recover`) and
     `uninstall_right`; returns `RightReport`. Session pins live until after
     every store operation; the lock is released last. A bad private chain
     refuses before cleanup, recovery or any right call.

   ADR 0012 still permits opening a policy-checked pre-existing staging file
   with `DELETE` for cleanup and replacing an existing target through its
   checked publication protocol. Those session operations are outside the
   installer-only fence in decision 3; no other repair authority is added.

   Uninstall is guard, `WindowsScm::deregister()`, then `uninstall`: every SCM
   `Err` returns before any store, record, recovery or LSA call, by type.
   `owner_scm*` never references the store or right; the installer references
   neither the right adapter nor `owner_transactions`. No raw grant, revoke or
   store call is reachable otherwise. PR-D's CLI calls only these methods.
9. **Binary placement is an explicit owner prerequisite.** Elevated, the owner
   creates `C:\Program Files\Arxcovo`, replaces its inherited ACL with
   `SYSTEM:(OI)(CI)F`, `Administrators:(OI)(CI)F`, `Users:(OI)(CI)RX`
   (`icacls … /inheritance:r /grant:r` with SIDs), then *copies* `arxd.exe`
   in (a move keeps the source descriptor). Install refuses anything else
   under decision 1 with a fixed token. Agents perform none of this.

## Fault model (install)

- **Process termination** leaves a prefix of decisions 4–7, and a rerun accepts
  every prefix: verified directories and lock, Empty partial or Retained
  members, and ADR 0012's SCM convergence. Nothing is rolled back. The
  registration exists only after every earlier step finished.
- **Power loss or an OS crash.** Only flushed *empty file contents* are
  requested durable. Name bindings have no documented user-mode flush, so a
  lost creation is a missing member that the rerun creates. Untested.
- **Concurrency.** `owner.lock` serializes installs from decision 5; earlier
  create-or-verify is idempotent. Uninstall's SCM step is not lock-serialized
  but has no right authority. Lock-ignoring admin/SYSTEM writers are outside
  the guarantee (ADR 0012).
- **Uninstall partial state.** SCM removal can succeed before session private-
  directory checks refuse. No record or right has been touched. After the owner
  resolves the existing refusal, rerunning uninstall accepts the absent service
  and repeats the checked store phase. No service recreation or rollback occurs.
- **Refusals** never self-resolve: the owner removes unsafe objects by hand
  (ADR 0009 squatting rule). No prompt, acknowledgement, recovery flow,
  account creation, password fallback or S4U retry.

## Parent-pin sharing evidence (bounded, unprivileged)

Probe, 2026-11-11: non-elevated user, GUID directory in the session's scratch
temp, NTFS, Windows NT 10.0.26300, no system path or privilege, cleaned up. Pins:
`READ_CONTROL|FILE_READ_DATA|FILE_READ_ATTRIBUTES`, backup semantics, no-follow;
creations used a current-user protected descriptor.

| Pin share on parent | Operation | Win32 result |
| --- | --- | --- |
| read only (parent, then nested child pin) | `CreateDirectoryW` child; `CREATE_NEW` file + flush | 0; 0 (both levels) |
| read only | `CreateDirectoryW` existing / `CREATE_NEW` existing | 183 / 80 |
| (lock held, share 0) | second open, share 0 | 32 |
| read only | `MoveFileExW` rename *within* the pinned parent | 32 |
| read and write | the same rename | 0 |
| read only, or read and write | rename of the pinned directory itself | 32 |

On this host, creating a direct child does not conflict with a read-only share
pin on its parent; a rename does (hence `config` needs write sharing, as in
ADR 0012). **Unverified:** `ProgramData`, SYSTEM-owned protected directories,
an elevated token with restore enabled, other filesystems or builds. Brief
test I8 reproduces this table. If a production create fails under a pin, the
only allowed change is `FILE_SHARE_WRITE` on that pin, never delete sharing.

## Deviations and new strictness

| Item | Source | Kind |
| --- | --- | --- |
| Converge from prefixes; existing objects pass strict policy, not ACL identity | ADR 0009; ADR 0014 d7 | deviation (as ADR 0014, ADR 0012 SCM) |
| Final `BinaryIntegrity` on the binary directory at install | startup `observe_layout` | new strictness |
| Installer-only read-only opens of pre-existing objects under restore | decision 3 | new strictness |
| Binary-only `OwnerGuard`; session pins private chains before restore; install restores after SCM observation | ADR 0012 steps 1–4; SCM brief §13 | reorder, same checks |
| Uninstall consumes the proof before `recover` | ADR 0012; SCM brief §13 | recovery stays first among store calls |
| Binary-directory ACL owner step | ADR 0009 guide step 4 | added owner prerequisite |

## Unverified (only a separately reviewed disposable host, never the owner PC)

`CreateDirectoryW`/`CreateFileW` accepting owner `SY` with restore enabled
(PR-B staging relies on it too); pins and creation under the real `ProgramData`
with an elevated token; the binary-directory ACL recipe; a started service
initializing a provisioned empty triplet; every item in the SCM brief §12.

## References

[Privilege constants](https://learn.microsoft.com/en-us/windows/win32/secauthz/privilege-constants)
(`SE_RESTORE_NAME`), [CreateFileW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew),
[CreateDirectoryW](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createdirectoryw),
[TOKEN_OWNER](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_owner).
Build brief: [windows-install-provisioning.md](../briefs/windows-install-provisioning.md).
