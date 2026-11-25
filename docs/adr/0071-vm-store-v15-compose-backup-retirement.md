# 0071 VM store v15 and owner retirement of Compose backups

Status: implemented. Extends [ADR 0070](0070-vm-store-v14-compose-backup.md).

The two-copy project limit needs a way for the owner to make room for a new
backup. The owner runs the standalone `arxd backup-retire` command using the
same non-elevated OS account that owns the foreground VM store. The command
checks ownership of the state directory, lock, database and existing SQLite
companions. Both list and retire take the daemon's exclusive state lock.
List opens SQLite read-only. Retirement also requires an interactive
stdin confirmation of the full backup ID. The foreground VM profile has
host command execution disabled and exposes no maintenance tool.

## Custody and accounting

Store v15 preserves the v14 backup rows and their immutability triggers. It
adds a retirement ledger keyed by backup ID. The ledger retains the owner SID
or UID, start and completion times, and the recorded SHA-256 when available.
Before any deletion, a durable transaction records the intent. Once custody
proves removal, a second transaction fills the completion timestamp. Only
completed entries release the host disk charge and exclude a settled copy from
the project retention count. A repeated successful backup request still
returns its historical receipt.

The owner supplies the original backup root. The command opens its existing
private directories and checks their held identities against the recorded
namespace. For a settled copy, both the settled leaf and any original staging
hard link must match the recorded file identity before removal. On Windows
the command uses volume-GUID paths from the held directories and E4's
delete-by-handle primitive. On Unix it uses E4's descriptor-relative private
cleanup slot with identity revalidation before unlink. The final absence
check covers both public names and, on Unix, the private cleanup leaves. An
interruption between deletion and completion leaves the charge in place.
The same owner may repeat the command to verify absence and complete the
existing intent.

For new backup admissions, the host records the namespace before any guest
exchange. A v15 row that never bound a namespace failed before it could
create host staging. A bound witnessless partial can be retired only when
both expected leaves are absent from the held namespace. A leaf lacking a
file witness is unattributed and blocks completion. A witnessless v14 row
with no recorded namespace remains charged because the migration cannot
recover its original root identity. Native Compose activation was closed at
v14. The daemon's attributed reconciliation still applies to partials with
a recorded file witness.

## Upgrade and recovery

The migration is forward-only. The current controller upgrades the store,
and maintenance requires that version. The [owner runbook](../dev-host-setup.md#retire-a-compose-backup)
covers restart, interrupted retirement and rollback. Backup media power-loss
durability and real static VM backups remain **unverified on real hardware**.
