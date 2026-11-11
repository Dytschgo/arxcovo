# 0012: Windows owner transaction, recovery and batch-right provenance

Status: settled architecture contract (Opus 5.5 high, design sub-agent),
2026-11-11, prepared from base `39f9b0ccebb73a885357f82d82de0f41b580132d`.
Corrected the same day from base `68614bcdf7281f161c1d5ed339340685200b0f8c`
after independent Sol review finding P1
(docs/reviews/0012-owner-contract-review.md): the safety guarantee is
narrowed to an explicit fault model (decision 6). It needs a fresh independent
Sol recheck before implementation is merged. Nothing
here is implemented, installed or exercised. Every native path is **unverified
on real hardware**. This ADR establishes no milestone 2 acceptance.

Amends ADR 0009 "Owner subcommands" (`service configure` and `service
uninstall`) for Windows only. It replaces the `Journal`/`ConfigurationIo` shape
of the pure candidate in `service::owner_transactions` (`a8628c6`). Linux and
macOS owner paths are unchanged.

## Problem

`configure` must grant `SeBatchLogonRight` to the pinned build SID, and
uninstall must remove it only if Arxcovo added it (ADR 0009). LSA policy and
NTFS have no common transaction. A process can die after `LsaAddAccountRights`
takes effect and before any durable receipt. After that crash, the right's
presence cannot show who added it. The existing `granted_batch_right` config
flag cannot carry this either. It is rewritten by every configure, it is
not cleared when uninstall revokes the right, and uninstall can only read it
while the build account still resolves.

## Truthful limit

No user-mode mechanism can attribute a direct LSA assignment after a crash
inside the grant window. LSA keeps no per-assignment author. Assignment is
idempotent: an add reports success whether or not the right was already
there. Security event 4704 depends on an audit policy that is off by default,
and Arxcovo must not change audit policy. So the contract does not try to
prove who added the right in that window. It resolves a crash in that window
automatically, in the direction that never removes someone else's right.
Decision 6 states the fault model under which that holds, and the one
power-loss case where it does not. The remaining
cost is a batch right Arxcovo may have added that is left in place, and
uninstall reports it with a fixed token.

## Where each requirement comes from

| Source | Requirement used here |
| --- | --- |
| Kickoff (explicit) | Service runs in a dedicated standard account. Safety comes from technical fences, never user approvals. Strict argument validation, bounded reads, path and reparse-point refusal. No secrets or owner data in logs. Agents never perform owner admin steps. Unverified paths are labelled. |
| ADR 0009 (settled) | Owner-only elevated subcommands. Fixed layout. SYSTEM-owned protected config and state with squatting refusal. Explicit `S-1-5-18` owner via `SeRestorePrivilege`. Atomic serde config write. Grant only to the pinned build SID. Record a grant, and uninstall removes it. Uninstall never deletes config, state, workspace or account. |
| This ADR (chosen strictness) | Separate provenance record. Presence never implies ownership. The `unproven` state. Owner lock. Fixed staging names. Compare-and-replace on every write. Fixed-binary identity for configure and uninstall too, not only install. No compensating revoke during configure. A cross-SID record refuses. Automatic recovery inside existing owner commands. |

## Decision summary

1. **Provenance moves to a separate record.** The record is a SYSTEM-private
   file, `owner-record.json`, beside the config. It names one SID and one state:
   `none`, `intent`, `granted` or `unproven`. Only this record can authorize a
   revoke. `granted_batch_right` stays in schema `arxd-service/1` as a
   non-authoritative mirror. Nothing may decide a revoke from it.
2. **Meaning of `granted`.** The record enters `granted` only after both of
   these held while the owner lock was held:
   - the direct assignment was observed absent;
   - `LsaAddAccountRights` for exactly that SID then returned success.

   While the last record value that Arxcovo wrote and saw succeed is
   `granted`, the direct `SeBatchLogonRight` assignment of that SID belongs to
   Arxcovo. A `granted` value that reappears because a later record write was
   lost or rolled back carries no authority in this contract. Decision 6 says
   when that can happen. Uninstall removes it even if
   someone else also asserted it during that time, because a membership
   cannot have two owners. The owner guide states this.
3. **The only ambiguous state is `intent`.** The engine writes `intent`
   durably before the grant. Recovery decides from one fresh observation:
   - absent: the state becomes `none` (the grant did not take effect);
   - present: the state becomes `unproven`. Arxcovo never revokes an
     `unproven` right, and uninstall reports it as
     `batch-right-retained-unproven`.
4. **No compensating revoke in configure.** If configure fails after a durable
   `granted`, the right and the record stay. Rerunning configure converges on
   them, and uninstall removes them. The only revoke call site is uninstall,
   and only from `granted`.
5. **Recovery runs inside existing owner commands.** `configure` and
   `uninstall` run recovery under the owner lock before doing anything else.
   There is no `recover` subcommand, no acknowledge flag and no prompt. The
   one manual repair is for a record that is corrupt or wrongly protected, and
   it is documented.
6. **The safety guarantee holds under a stated fault model, not under any
   loss.** The contract distinguishes three classes of event:
   - **Process termination (inside the guarantee).** The owner process
     crashes, is killed or exits at any point, and Windows keeps running. Every
     LSA call and rename that returned success stays visible to the next
     process. Under this class, together with external rights changes made
     while no owner command runs, a revoke happens only for a membership that
     Arxcovo added from an observed absence, or that collapsed into it, while
     the last record value Arxcovo wrote was `granted` (P6). The worst outcomes
     are `unproven`, an orphan right, a re-grant, or a config that keeps its
     previous bytes.
   - **Lost LSA or registry writes alone (inside the guarantee).** Power loss,
     an OS crash or a hive rollback may lose or reorder LSA writes while every
     record write that returned success persists. The same invariant then
     holds (P6b). A lost grant behind a `granted` record leaves the right
     absent, and a later external add collapses into Arxcovo's membership
     (decision 2). A lost revoke behind a `none` record leaves an orphan that
     is never revoked. A lost config publication only reverts the config
     bytes, and the mirror is never authoritative.
   - **A lost or rolled-back record write (outside the guarantee).** Power
     loss, an OS crash (bugcheck), a hard reset or a crash-consistent disk or
     VM snapshot can lose a record rename that had returned success, because
     the new name binding has no documented flush (durability section). An
     administrator restoring an older `owner-record.json` from backup has the
     same effect, and the contract treats that as the administrator's
     assertion. If the lost write was a transition out of `granted` (to
     `none`, `intent` or `unproven`), the old `granted` value reappears, and
     Arxcovo cannot detect that. The next uninstall then revokes an external
     add made after the lost transition and reports `batch-right-removed`.
     Example: the record is `granted`; uninstall revokes and writes `none`;
     the owner grants the right; power loss restores `granted`; the next
     uninstall removes the owner's grant. Every unsafe outcome in this class
     needs a stale `granted` value (P6c). A stale `none`, `intent` or
     `unproven` value only causes an orphan or `unproven`, and a torn record is
     never published. The harm is limited to one direct `SeBatchLogonRight`
     assignment on the recorded SID. The uninstall token reports it, and the
     owner can re-add the right.

   Mechanisms that could close the third class were rejected as
   disproportionate or unproven. A whole-volume `FlushFileBuffers` needs an
   elevated raw-volume write handle, has no documented rename-durability
   guarantee, and does nothing against snapshot or backup restores. A
   rollback-resistant epoch in LSA private data adds a second LSA mutation and
   depends on undocumented write ordering inside the SECURITY hive. An owner
   acknowledgement step would be a product approval flow, which the kickoff
   forbids. None of these is part of the contract.

## Dependencies (not supplied here)

- **D1. Corrected native ancestor authority.** Independent review found that
  PR25's ancestor `BinaryIntegrity` check rejects the stock `C:\`,
  `C:\ProgramData` and `C:\Program Files` parents, which carry create-child,
  inherit-only and CREATOR OWNER grants. So the current `native::observe_layout`
  and `checked` cannot open the protected service layout. That fix needs
  separate, per-next-child authority checks for rename, replacement and
  re-ACL. This contract does **not** assume today's `observe_layout` works,
  and it relaxes no final-object policy: `Policy::PrivateDirectory`,
  `Policy::PrivateFile` and `Policy::BinaryIntegrity` keep their current meaning
  for the objects they protect. Every "pin" below means "open through the
  corrected ancestor walk". The macOS stock-parent findings are a separate
  matter and are unverified.
- **D2. Service workspace metadata boundary.** Review flagged that the
  SYSTEM/root service opens workspace metadata, which conflicts with ADR 0009.
  That service-side question stays open and must be deferred or recorded as
  its own ADR. This contract adds no service-side workspace open. The elevated
  owner `configure` keeps ADR 0009's existing metadata-only inspection
  through `Resolver::workspace_safe`.
- **D3. Execution identity.** Windows S4U is still not established (ADR 0009).
  A correct grant does not make S4U work.

## Storage

All paths are fixed, derived from `Layout::system()`, and never come from argv,
environment or config. `C` below means `{ProgramData}\Arxcovo\config`.

| Object | Path | Created by | Policy | Size cap |
| --- | --- | --- | --- | --- |
| owner lock | `C\owner.lock` | install, exclusive create, 0 bytes | `PrivateFile`, one link | 0 |
| record | `C\owner-record.json` | first configure | `PrivateFile`, one link | 512 B |
| record staging | `C\owner-record.json.staging` | each record write | `PrivateFile` | 512 B |
| config | `C\arxd.json` (`Layout::config`) | configure | `PrivateFile`, one link | `MAX_CONFIG_BYTES` |
| config staging | `C\arxd.json.staging` | configure `prepare` | `PrivateFile` | `MAX_CONFIG_BYTES` |

Record encoding uses serde, `deny_unknown_fields`, with no trailing data:

```json
{"version":1,"sid":"S-1-5-21-<a>-<b>-<c>-<rid>","state":"none|intent|granted|unproven"}
```

Decode refuses with `RecoveryRequired` if any of these hold: the size is over
512 bytes, the version is not 1, a field or state is unknown, or the SID
fails `ordinary_sid`. Parsing is not provenance. Only bytes read through the
pinned private handle count. The record has no Debug output and never appears
in MCP responses, logs or errors. Errors carry fixed tokens only.

Uninstall never deletes the record. It writes `none`. An absent record means
`none` for every SID.

### Protection of created objects

Every staging file is created with `CREATE_NEW` and an explicit absolute
security descriptor from the fixed constant
`O:SYG:SYD:P(A;;FA;;;SY)(A;;FA;;;BA)`. This uses SYSTEM as owner, a protected
DACL, no inheritance, and exactly the two ACEs that ADR 0009 allows.
`SeRestorePrivilege` must be enabled in this process first, and the enable must
not report `ERROR_NOT_ALL_ASSIGNED`. After writing, the handle is checked
against `Policy::PrivateFile` with `GetSecurityInfo`, the same evaluator that
startup uses. `NumberOfLinks` must be 1. Arxcovo never repairs an existing
object's owner or DACL.

### Compare-and-replace publication

The single primitive is `replace(target, expected: Option<[u8; 32]>, bytes)`.
Callers hold the owner lock and the pinned parent directory.

1. Create the staging file. Write the bytes. `FlushFileBuffers` on that
   handle must succeed, which requires `GENERIC_WRITE`. Check its
   protection and link count.
2. Check the expected target state:
   - `expected = None`: the target must not exist (`ERROR_FILE_NOT_FOUND`).
   - `expected = Some(h)`: open the target with `FILE_FLAG_OPEN_REPARSE_POINT`,
     access `GENERIC_READ | READ_CONTROL`, and sharing
     `FILE_SHARE_READ | FILE_SHARE_DELETE`. No write sharing, so nobody can
     write the target in place after the check. Read at most cap + 1 bytes,
     require SHA-256 = `h`, require `PrivateFile` policy, one link and no
     reparse tag, and record its `FileIdInfo`. Keep this handle open through
     the rename.
3. Just before the rename, reopen the target path with no-follow and compare
   its `FileIdInfo` with the held handle. A mismatch is `Conflict`.
4. Rename the staging handle with `SetFileInformationByHandle(FileRenameInfoEx)`,
   using the full fixed target path and a NULL `RootDirectory`. The flags are
   `FILE_RENAME_FLAG_REPLACE_IF_EXISTS | FILE_RENAME_FLAG_POSIX_SEMANTICS` when
   `expected` is `Some`, and `0` (fail if the target exists) when it is `None`.
5. Reopen the target with no-follow. Its `FileIdInfo` must equal the staging
   handle's, and its bytes must hash to the new value.

Any failure after step 4 begins is reported as `Io`, because publication may
already have happened. The caller never infers state from such an error.
For the record, the next owner command reads it again. For the config,
rerunning configure compares the actual bytes again.

### Sharing and pins

- The immediate parent `C` stays open for the whole session with
  `FILE_LIST_DIRECTORY`, sharing `FILE_SHARE_READ | FILE_SHARE_WRITE`, and
  **never** `FILE_SHARE_DELETE`. Write sharing is needed because a rename
  opens its target directory for write. Higher ancestors follow the D1 rule.
- The owner lock is opened with sharing `0`. A sharing violation means another
  owner command holds it and maps to `Busy`.
- A native current-user test, not this ADR, settles the exact sharing rule
  (test N4). If staging create or rename fails under the pin, the only allowed
  change is adding `FILE_SHARE_WRITE` on the immediate parent. Adding delete
  sharing is never allowed.
- The running service reads the config with read-only sharing at startup, so
  a configure that overlaps that read can fail with `Io`. Rerun it. A
  configure takes effect at the next service start.

### Durability: honest limit

Staged data is flushed before the rename, so a torn config or record is
never published, assuming the disk honours the flush. Microsoft documents no
user-mode, non-administrative directory flush. `MOVEFILE_WRITE_THROUGH` only
guarantees copy-and-delete moves. So the durability of the new name binding
across a power loss is **not guaranteed**. LSA rights live in the registry
SECURITY hive, which is also lazily flushed. Decision 6 states the
consequences. Lost LSA, registry or config writes stay inside the safety
guarantee. A lost record rename that undoes a transition out of `granted` is
outside it, and can revoke one later external grant. Nothing in this ADR has
been tested against physical power loss, OS crashes, NTFS metadata loss or
SECURITY hive rollback. These statements rest on the documented API
guarantees and the pure state model only.

## Owner session preconditions (native, before any engine call)

The native session `WindowsOwnerSession` opens in this order. Every failure
returns a fixed `layout::Error` or `Failure` token.

1. Check the process token. `TokenElevation` must be elevated, and
   `CheckTokenMembership` must show Administrators enabled. Otherwise
   `OwnerRequired`.
2. Resolve `Layout::system()`, then pin the layout through D1.
3. For install, configure and uninstall: the process image's `FileIdInfo`
   (volume serial and 128-bit ID) must equal the pinned layout binary's, and
   the binary stays pinned with no write or delete sharing. ADR 0009 requires
   this only for install; this ADR adds configure and uninstall. Start, stop
   and status skip this check.
4. Enable `SeRestorePrivilege`.
5. Pin `C`, then open `owner.lock` (sharing 0, `PrivateFile` policy, one link).
   If the lock is missing, return `Layout`: install has not run, and
   configure never creates the lock.
6. Remove leftover staging files. A fixed staging name that passes
   `PrivateFile` with one link is opened with `DELETE` and removed through
   `FileDispositionInfoEx` (POSIX semantics) on that handle. A staging name
   that fails the policy refuses with `RecoveryRequired` and is never deleted.
7. Run `recover` (below).

The lock is released last, when the session is dropped. Arxcovo processes
are serialized by the lock. Unprivileged users are excluded by the protected
DACLs. A concurrent administrator or SYSTEM writer that ignores the lock can
still race the step 2–4 window of compare-and-replace. That case is outside
the guarantee, consistent with service-foundation.md.

## State machine

The table covers one SID, with the engine holding the lock. `P` means the
direct assignment is observed present, and `A` means observed absent.

| Record before | Event | Record after | LSA mutation | Result |
| --- | --- | --- | --- | --- |
| none or absent (any SID), unproven/granted (same SID) | configure, `A` | intent | — | continues |
| intent | grant returns Ok | granted | add (done) | continues to publish |
| intent | grant returns Err, then `A` | none | — | `Io` |
| intent | grant returns Err, then `P` | unproven | — | `RightUnproven` |
| intent | receipt write fails | intent (durable) | add (done) | `RecoveryRequired` |
| intent | recover, `A` | none | — | `Recovery::NotGranted` |
| intent | recover, `P` | unproven | — | `Recovery::Unproven` |
| none or absent, `P` | configure (no add) | none (this SID) | — | publish |
| granted (same SID), `P` | configure (no add) | granted | — | publish, mirror `true` |
| unproven (same SID), `P` | configure (no add) | unproven | — | publish, mirror `false` |
| granted or unproven (other SID), or intent | configure | unchanged | — | `Conflict` (intent: `RecoveryRequired`) |
| granted, `P` | uninstall | none | remove | `Removed` |
| granted, `P` | uninstall, revoke Err | granted | unknown | `Io`; rerunning retries |
| granted, `A` | uninstall | none | — | `AlreadyAbsent` |
| unproven, `P` | uninstall | none | — | `RetainedUnproven` |
| unproven, `A` | uninstall | none | — | `AlreadyAbsent` |
| none, `P` / `A` | uninstall | none | — | `RetainedNotArxcovo` / `NotRecorded` |
| absent record | uninstall | absent | — | `NotRecorded` |

There is no `intent` row for uninstall, because the session's recovery
resolves `intent` first. A publish failure leaves the record as it was after
the grant step, and the command returns `Io`.

## Pure engine API (`service::owner_transactions`, exact)

```rust
pub enum Failure { InvalidPlan, Io, Conflict, Busy, RecoveryRequired, RightUnproven, Unsupported }
// tokens: service-owner-{plan,io,conflict,busy,recovery-required,right-unproven,unsupported}

pub enum RightState { None, Intent, Granted, Unproven }      // serde lowercase
pub struct OwnerRecord { version: u8, sid: String, state: RightState } // no Debug
impl OwnerRecord {
    pub fn encode(&self) -> Result<Vec<u8>, Failure>;
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure>;  // 512 B, strict
}

/// Durable SYSTEM-private store. The caller holds the owner lock.
pub trait OwnerStore {
    /// Record plus the SHA-256 of its exact bytes, for compare-and-replace.
    fn record(&mut self) -> Result<Option<(OwnerRecord, [u8; 32])>, Failure>;
    fn replace_record(&mut self, expected: Option<[u8; 32]>, next: &OwnerRecord)
        -> Result<[u8; 32], Failure>;
    /// Steps 1-2 of compare-and-replace for the config. No publication.
    fn stage_config(&mut self, bytes: &[u8], expected: Option<[u8; 32]>) -> Result<(), Failure>;
    /// Steps 3-5. An Err may mean the config was published.
    fn publish_config(&mut self) -> Result<(), Failure>;
}
/// Direct assignment of SeBatchLogonRight to exactly `sid`. Nothing else.
pub trait BatchRight {
    fn observe(&mut self, sid: &str) -> Result<bool, Failure>;
    /// Any Err means the outcome is unknown.
    fn grant(&mut self, sid: &str) -> Result<(), Failure>;
    fn revoke(&mut self, sid: &str) -> Result<(), Failure>;
}

/// Only obtainable from this function; there is no public constructor.
pub struct BatchObservation { present: bool, recorded_grant: bool }
pub fn observe_batch(sid: &str, s: &mut impl OwnerStore, r: &mut impl BatchRight)
    -> Result<BatchObservation, Failure>;
// recorded_grant = present && record == {sid, Granted}

pub enum Recovery { Clean, NotGranted, Unproven }
pub fn recover(s: &mut impl OwnerStore, r: &mut impl BatchRight) -> Result<Recovery, Failure>;
pub fn configure(req: &Configure, s: &mut impl OwnerStore, r: &mut impl BatchRight)
    -> Result<(), Failure>;
pub enum RightReport { Removed, AlreadyAbsent, RetainedUnproven, RetainedNotArxcovo, NotRecorded }
pub fn uninstall_right(s: &mut impl OwnerStore, r: &mut impl BatchRight)
    -> Result<RightReport, Failure>;
```

Changes to existing code:

- `owner_plan::configure_plan` takes `batch: BatchObservation` instead of
  `batch_present: bool`. It emits `BatchRight { add: true }` exactly when the
  right is absent. Its mirror is `add || recorded_grant`, and
  `previous.granted_batch_right()` no longer feeds it. Expected old bytes are
  still `previous.serialized(previous.granted_batch_right())`, so they are the
  exact canonical bytes on disk.
- `Configure::from_plan` takes the same `BatchObservation`, stores it, and
  requires `config.granted_batch_right() == add || recorded_grant`.
  `configure` calls `observe_batch` again under the lock and refuses
  `Conflict` if the result differs from the stored observation.
- `owner_plan::uninstall_plan` no longer emits `BatchRight { add: false }`.
  On Windows, revoke authority comes only from `uninstall_right`. Uninstall no
  longer needs a config that revalidates or a build account that still
  resolves, because it revokes by the record's SID.
- `Journal`, `ConfigurationIo`, `Phase` and the compensating branch of
  `recover` are deleted, together with the tests that assert compensation.
- `Plan::execute` must never be used for Windows configure or uninstall.

### `configure` order

1. Read the record (an absent record means `none`).
2. Refuse if the state is `intent` (`RecoveryRequired`). Refuse if the record
   names another SID in `granted` or `unproven` (`Conflict`).
3. Call `observe_batch` and compare it with the request's observation.
4. `stage_config`.
5. If adding the right: write `intent` over the read hash, call `grant`, then
   write `granted`. If the grant fails, observe again and write `none` or
   `unproven`. If not adding and the record is absent or names another SID
   (in `none`), write `{sid, none}`.
6. `publish_config`.

## Native adapter contract

**LSA** (`BatchRight` for Windows):

- Validate the SID with `ordinary_sid`, convert it with
  `ConvertStringSidToSidW`, check `IsValidSid`, and free it with `LocalFree`.
- `observe` uses `LsaOpenPolicy(NULL, POLICY_LOOKUP_NAMES)` and
  `LsaEnumerateAccountRights`. `STATUS_OBJECT_NAME_NOT_FOUND` or an empty
  set means absent. The right is present only if an entry exactly equals
  `SeBatchLogonRight`. The count is capped at 64, each name is at most 128
  bytes with even length, and `LsaFreeMemory` runs on every path. Any other
  status is `Io`.
- `grant` uses `POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT` and
  `LsaAddAccountRights` with exactly one right.
- `revoke` uses `LsaRemoveAccountRights` with `AllRights = FALSE` and exactly
  one right. `AllRights = TRUE` is forbidden, because it deletes the LSA
  account object, including any deny rights the owner set.
- Only the direct assignment is observed. A right inherited through a group,
  for example Administrators, is not provenance and does not count as present.
- NTSTATUS values never reach output. They map to `Failure` only.

**Store** (`OwnerStore` for Windows): the storage section above. The policy
type is a value. The production constructor fixes SYSTEM and the descriptor
above. A `#[cfg(test)]` fixture constructor uses the current user's SID, so
tests never relax a production check.

**SCM** (a separate PR; ADR 0009's `Registration` is the contract):

- *Install.* Use `CreateServiceW` with the quoted fixed image and
  `service run`, `SERVICE_WIN32_OWN_PROCESS`, `SERVICE_AUTO_START`, account
  NULL (LocalSystem), and never start the service. Then apply each
  `ChangeServiceConfig2W` value: delayed auto-start, unrestricted service SID,
  the four required privileges, a 30000 ms preshutdown, failure actions
  5000/30000 ms with an 86400 s reset, and the non-crash flag set to false.
  An existing `arxd` service is treated as Arxcovo's only when its base
  configuration exactly matches: own-process type, auto start, exact
  `lpBinaryPathName` bytes and LocalSystem. In that case install converges
  the config2 values. This recovers from a crash between `CreateServiceW` and
  the config2 calls without manual repair, which is a small departure from
  ADR 0009's identical-only rule. Any other mismatch refuses with `Conflict`.
  The SCM object DACL is left at its default and is not compared.
- *Start, stop, status, uninstall.* Each first runs the same base-configuration
  comparison, and a mismatch refuses: a foreign `arxd` is never started,
  stopped or deleted. Stop polls until stopped, with a 30 s bound; a timeout
  returns `Io` and deletes nothing. Uninstall order:
  1. stop;
  2. `DeleteService`; a missing service counts as already removed, so a rerun
     after a partial uninstall continues;
  3. `uninstall_right`.

  An SCM failure stops the uninstall before the right is touched.
- *Status mapping.* `ERROR_SERVICE_DOES_NOT_EXIST` → `not-installed`. Stopped
  with a zero exit → `stopped`, and with a nonzero `dwWin32ExitCode` or
  service-specific code → `failed`. Start pending → `starting`, running →
  `running`, stop pending → `stopping`. Any paused or continue state →
  `failed`.

## External and stale changes

| Change | Result |
| --- | --- |
| Someone removes a `granted` right | The next configure observes it absent and goes through intent and grant again, which the owner's own configure authorizes. Uninstall reports `AlreadyAbsent`. |
| Someone adds the right while `granted` | Collapses into Arxcovo's membership, and uninstall removes it (decision 2). Re-add it after uninstall if you want to keep it. |
| Someone adds the right while `none` | Never Arxcovo's, never revoked, unless a later power loss or restore brings back a stale `granted` record (decision 6, outside the guarantee). |
| Someone adds the right while an owner command runs | The lock does not serialize them. If the owner process dies before the receipt, recovery records `unproven`, never revokes it, and reports it. If the grant returns Ok and the record reaches `granted`, the add cannot be told apart from Arxcovo's and collapses into it (decision 2). |
| Power loss, OS crash or snapshot revert after an owner command | Lost LSA, registry or config writes stay safe. A lost record write that undoes a transition out of `granted` is outside the guarantee (decision 6). |
| Configured account name missing | Preflight reports `account_missing`. Uninstall uses the record's old SID without requiring the account to exist. |
| Configured name resolves to a different SID | Preflight reports `account_identity_changed`. Configure refuses the SID change, as `configure_plan` already does, and the cross-SID record refuses. Uninstall uses the record's old SID. |
| Domain GPO manages User Rights Assignment | Outside the guarantee: a policy refresh may remove or replace the assignment. Preflight reports `batch_right_missing`. |
| Record hand-edited, restored from backup or deleted by an administrator | Deleting it forgets the grant, so the right is never revoked. A valid edit or a restored older copy is treated as the administrator's assertion, including a restored `granted` (decision 6). A record that is invalid or wrongly protected refuses with `RecoveryRequired` (manual repair below). |
| Config hand-edited | Startup still fails with exit 4 if the owner changed. Configure refuses `Conflict` if the expected bytes differ. The record is unaffected. |

## Limits nothing here can resolve

- A crash between the grant and its receipt, with the right present, gives
  `unproven`. The right may be Arxcovo's, and it stays until the owner
  removes it.
- Recovery assumes that no LSA call issued by a terminated process is still
  in flight once the lock can be acquired. Microsoft does not document this.
  If the assumption fails, `none` is recorded and the right appears later as
  an unreported orphan. That outcome is safe but incomplete.
- A record write lost to power loss, an OS crash or a snapshot revert can
  bring back a stale `granted` value, and the next uninstall can then revoke
  one later external grant (decision 6). Arxcovo cannot detect this.
  Uninstall reports `batch-right-removed`, and the owner can re-add the right.
- Privileged writers that ignore the lock can race compare-and-replace.
  Domain GPO can change rights. Neither is serialized.
- Ownership of a set membership cannot be exclusive (decision 2).

## Manual repair (the only one)

The owner repairs by hand only when `owner-record.json` is unreadable,
oversized or wrongly protected, and every configure or uninstall then reports
`service-owner-recovery-required`. Repair is to inspect the record as an
administrator and delete it. Arxcovo then treats the right as not its own.
If the right is unwanted, remove it in `secpol.msc`. Crash ambiguity never
requires manual repair.

## Test matrix

**Pure tests.** These use a recording model with two ghost variables. The
first is the last record value Arxcovo wrote and saw succeed. The second is
the latest acknowledged act on the membership: an Arxcovo grant or revoke, an
external remove, or an external add tagged with whether that last written
record value was `granted` at the time. An **unsafe revoke** removes a present
membership whose latest acknowledged act is an external add made while the
last written record value was not `granted`. External events happen only
between commands or after a crash. Concurrent writers inside a command are
outside the model.

| # | Case | Required assertion |
| --- | --- | --- |
| P1 | add path | call order: record, observe, stage, intent, grant, granted, publish. Final state `granted` with the new config. |
| P2 | right already present, record `none` | no intent, publish, `{sid, none}`, mirror false. Uninstall gives `RetainedNotArxcovo` with no revoke. |
| P3 | stale observation in either direction | `Conflict` before staging, with no LSA mutation. |
| P4 | grant Err, then absent | `none`, `Io`, no publish, no revoke. |
| P5 | grant Err, then present | `unproven`, `RightUnproven`. A rerun without add succeeds and keeps `unproven`. Uninstall gives `RetainedUnproven` and `none`, with no revoke. |
| P6 | process crash after each call, both external toggles | Crash model: every completed write persists. Sequences combine configure and uninstall (each crashed after every call, with grant or revoke returning Ok, Err with effect, or Err without effect), external add and external remove. Each command starts with recovery. **Invariant: no unsafe revoke.** |
| P6b | lost LSA writes | P6 sequences plus a fault that reverts the LSA membership to any earlier value while every record write persists. **Same invariant: no unsafe revoke.** |
| P6c | lost record writes (documented limit) | P6 sequences plus a fault that reverts the record to any earlier written value. Assert the decision 6 example exactly: the second uninstall revokes and returns `Removed`. **Characterization: every unsafe revoke happens in an uninstall that read a `granted` value restored over a later non-`granted` write. A rollback to any other value never produces one.** This test pins the limit. It is not a safety claim. |
| P7 | receipt write fails | `RecoveryRequired`, durable `intent`. Recover gives `Unproven` when the right is present. |
| P8 | publish fails | `Io`, record `granted`, no revoke. |
| P9 | uninstall from `granted`, present | exactly one revoke, with the record's SID. A revoke Err leaves `granted`; the rerun gives `Removed`. |
| P10 | uninstall from `granted`, absent | `AlreadyAbsent`, no revoke. |
| P11 | configure with `intent` pending | `RecoveryRequired` with no observation. |
| P12 | cross-SID: record for A in `granted`, `unproven` or `intent`, configure B | refuses with no mutation. A `none` record for A gets replaced. |
| P13 | stale compare-and-replace hash | `Conflict`, record unchanged. |
| P14 | decode | 512-byte bound, unknown field or state, version, reserved and noncanonical SIDs, and trailing bytes all refuse. |
| P15 | mirror | `from_plan` rejects a mismatched mirror. A fresh observation that differs from the plan gives `Conflict`. |
| P16 | `granted`, then absent, then configure | goes through intent and grant again and ends `granted`. |
| P17 | static scan | no revoke reachable from `configure` or `recover`. `uninstall_plan` emits no `BatchRight`. |

**Native tests.** These run on Windows as the current user, in a temporary
directory with the fixture policy. None of them makes an LSA, SCM, account or
privilege call.

| # | Case |
| --- | --- |
| N1 | Create-new publication. An existing target refuses. The published file ID equals the staged file ID, and the bytes match. |
| N2 | Replace with a matching hash succeeds. A mismatch gives `Conflict`, leaves the target unchanged and removes the staging file. |
| N3 | A hard-linked target or staging file refuses. A junction parent refuses (junctions need no privilege to create). |
| N4 | While the session runs, staging create and rename succeed, and rename or delete of the pinned parent fails. After drop, the parent can be renamed. |
| N5 | A second session gets `Busy`. It succeeds after the first is dropped. |
| N6 | A leftover staging file with valid policy is removed. One with a foreign ACE refuses and is kept. |
| N7 | A 513-byte record and a config of `MAX_CONFIG_BYTES` + 1 both refuse. |
| N8 | A foreign handle holding write access makes compare-and-replace refuse, and the target is unchanged. |
| N9 | The production descriptor constant parses to owner SY, a protected DACL and exactly two ACEs (SY and BA). |
| N10 | LSA result mapping over synthetic buffers: not-found, empty, present, over-count, odd length, and other status. |
| N11 | SCM base-configuration comparison and status mapping over synthetic structures, including the exact quoted image bytes. |

**Disposable experiment.** This needs its own review before any dispatch, in
the pattern of PR12 and PR14. It runs only on an ephemeral GitHub-hosted
Windows runner with a generated account: install, configure (grant), status,
uninstall (revoke), then a pre-existing right that uninstall must retain. It
never runs on the owner PC. Until it runs, every native LSA, SCM, ACL and
rename path is **unverified on real hardware**.

## Build brief for Sol (high), one concern per PR

- **PR-A (pure).** Revise the engine, `configure_plan`, `Configure::from_plan`
  and `uninstall_plan` to the exact API above. Delete `Journal`,
  `ConfigurationIo` and compensation. Add P1–P17 with P6b and P6c. P6, P6b
  and P6c are exhaustive over crash prefixes and over bounded event sequences
  of at least depth 6. Two mutations must fail P6: writing `granted` for a
  right that was already present, and recovery writing `granted` for a
  present right. Neither P6b nor P6c may be described as modelling physical
  NTFS or LSA behavior. No native code.
- **PR-B (native store and LSA).** Add `owner_windows.rs` with
  `WindowsOwnerSession`, `OwnerStore` and `BatchRight`. Run N1–N10 locally
  without privilege. The session's ancestor pins wait on D1. Until D1 merges,
  N tests use a fixture root, and production `open` must fail closed with
  `Layout`. No CLI.
- **PR-C (SCM).** Install, start, stop, status and uninstall adapters, plus
  N11. No CLI.
- **PR-D (wiring).** Connect the `arxd service install|configure|start|stop|status|uninstall`
  commands to the session. Uninstall prints one fixed `RightReport` token,
  `batch-right-{removed,already-absent,retained-unproven,retained-not-arxcovo,not-recorded}`.
  Tests must not dispatch SCM or run owner arguments on the owner PC.
- Each PR is reviewed by another provider. The owner performs no step. None
  of these PRs establishes milestone 2 acceptance. Install also has to
  provision the state files the Application requires (service-foundation.md);
  that is outside this ADR.

## References

- [LsaRemoveAccountRights](https://learn.microsoft.com/en-us/windows/win32/api/ntsecapi/nf-ntsecapi-lsaremoveaccountrights)
  (`AllRights = TRUE` deletes the account object) and
  [MS-LSAD LsarEnumerateAccountRights](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-lsad/920f27bb-de33-418b-b6a9-add38bcb4f57)
  (not-found versus an empty set).
- [FILE_RENAME_INFORMATION](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntifs/ns-ntifs-_file_rename_information)
  and [FileRenameInformationEx](https://learn.microsoft.com/en-us/openspecs/windows_protocols/ms-fscc/4217551b-d2c0-42cb-9dc1-69a716cf6d0c)
  (replace and POSIX semantics).
- [FlushFileBuffers](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-flushfilebuffers)
  (requires `GENERIC_WRITE`) and
  [MoveFileExW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
  (write-through only guarantees copy-and-delete moves).
