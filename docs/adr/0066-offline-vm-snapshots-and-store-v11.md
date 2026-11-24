# 0066. Offline VM snapshots and VM store v11

Status: Proposed (2026-11-24)

## Decision

An offline snapshot contains an independent copy of a same-VM disk and its private UEFI variables. TEST restores keep their original VM identity, golden lineage and lease. Static restores retain the displaced pair as a protected restore point. The static TLS name derives from the persisted first active generation (`origin_generation`), while the physical generation and boot witness change on a subsequent start.

Static per-VM capacity is bounded to three manually created restore points plus one reserved fourth slot for retaining the displaced active pair on restore, within a shared 16-point host cap and conservative checked disk claims. Because committed static restore points (both `ready` and `dirty_release`) have no agent deletion route, filling all four slots or the shared host cap refuses further manual creates and restores until a future owner-authorized pruning capability lands.

The authenticated foreground advertises four snapshot tools (`vm_snapshot_create`, `vm_snapshot_list`, `vm_snapshot_restore`, `vm_snapshot_delete`), bringing the foreground profile to 22 tools (18 B2 tools plus 4). Mutations use a domain-separated request binding and the existing caller operation ledger. The ledger retains snapshot result IDs independently of catalog and audit rows, so accepted replay survives deletion, expiry, audit pruning (>128 rows) and restart. List reads are limited to four points and bind their cursors to the host, principal, tool and VM. A caller can delete a sealed TEST point after release proof. Static restore points have no agent deletion route and refuse with `static_backup_delete_forbidden` at both tool and store layers.

The VM store retains the v10 publication ledger and checks the typed publication intent and pending action 15/16 receipts in the snapshot transactions. A static operation takes the B2 effects slot after the foreground dispatch gate, checks the activation registry (rejecting active, pending, lost, unconfirmed-ingress or pending-stop states) and local connector, and drops those short guards before entering SQLite. The effects slot remains held through copy settlement. Fresh same-VM `unpublish_site` calls with no durable publication intent refuse with `busy` while snapshot work is unsettled, preserving ADR 0065 early owner revocation, replay and admitted-unpublish wait when an intent exists. Restore leaves the VM stopped with public routing unchanged. A future publication requires its own admission from a newly proved origin; intents naming a displaced generation are refused.

### Narrow `vm/runtime.rs` fence extension

Outside design §7.2, `crates/arxd/src/vm/runtime.rs` carries a minimal interface extension required for B2 mutual exclusion and stable static origin naming:
- `Origin` separates `origin_generation` (stable provisioned TLS naming) from `generation` (mutable active physical generation).
- `Publication::snapshot_route` claims the B2 effects slot and conservatively rejects active, pending, lost, unconfirmed-ingress and pending-stop states before returning a `SnapshotRoute` guard.
- In test builds only, `SnapshotRoute` carries an independently bound `(generation, epoch)` local-origin release witness. This grants no native authority.

### Pre-create boundary, expiry cancellation, reconciliation and retry

Each candidate staging leaf crosses an explicit durable pre-create boundary (`planned` -> `created` with `file_id IS NULL`) before exclusive filesystem creation, then records its initial file identity (`volume_id`, `directory_id`, `file_id`) and finally its completion digest/length (`identified`). Restart recovery (`abandon_inactive` excludes typed `vm_snapshot_work.candidate` rows) and expiry abandonment (`abandon_copies`) preserve provably untouched `planned` leaves, move identified leaves to `pending_disposal`, and quarantine attempted-but-unidentified leaves (`created` with `file_id IS NULL`).

TEST lease expiry splits release proof (`release_only: true`, empty artifacts) from active-pair disposal while non-active snapshot work remains, keeping active media last and writing tombstones only after every pair settles. Expiry signals scoped per-VM cancellation to any live snapshot worker and awaits release; a cancelled worker can hand off its proven leaf identities into `pending_disposal` without rewriting the interrupted caller receipt. Orphaned live `copy` intents reconcile after independent quiescence, interrupting only a still-pending caller receipt. Daemon reconciliation emits a bounded correlated `action=19` receipt linked via `vm_snapshot_reconciliation` (`source_activity`, `expiry_activity`, closed `cause`), recording quarantine uncertainty once (`quarantine_audited`) rather than repeating per backoff tick. Transient SQL settlement errors keep claims armed and retry after backoff; reaped snapshot worker panics/failures allow same-process foreground SDK recovery once the reaper reconciles the orphaned intent while still failing clean shutdown drain so worker failure evidence is never hidden.

After quiescence the reaper re-reads the selected generation, cleanup phase, ownership and exact artifact identities in an immediate transaction. If the selection changed, the reaper skips disposal. For an orphaned copy, that transaction interrupts its pending operation receipt before custody receives the files. The worker cannot subsequently publish that generation, even if removal or SQL settlement fails.

Each copy verifies host reserve and disk capacity before staging, at 6a checkpoints, during final recheck, and inside the settlement transaction itself. Immediately before committing the transaction, the controller checks the worker token against the current monotonic clock. Expiry rolls back the pair publication, receipt and charge changes together.

Native issuance remains closed. The foreground fixture uses private synthetic media and never grants authority to issue an offline lease or launch a real VM. Native NTFS/Job path custody, qemu-img inspection, copy and retirement durability, Windows and Ubuntu cold boots, physical reserve behavior, and live Cloudflare/TLS rollback are **unverified on real hardware**.

## Upgrade

The opener accepts an empty version 0 store (refusing nonempty version 0) and versions 5 through 11. It applies pending migrations in order under an immediate transaction with `version < n` guards, checks state headroom and foreign keys, then publishes version 11. Failed migrations roll back cleanly and can retry. Version 11 preserves activity IDs, `sqlite_sequence` high-water marks, B2 intent and settlement evidence, and legacy operations and snapshot charges. The legacy ready bit supplies historical dashboard projection alongside newly verified TEST `ready` points (hiding staging, deleting, quarantined and static points). Typed requests use a distinct binding (`snapshot`) and collide with accepted legacy action 5/6 keys without promoting legacy rows. An older v10 opener refuses version 11 without modifying database bytes.

Static origin identity backfills from the active generation in existing stores. The first activation assigns it on new instances. Later activation of a restored generation leaves the provisioned identity intact.

See [ADR 0063](0063-vm-store-v9-forward-only.md) for static generations and [ADR 0065](0065-vm-store-v10-publish.md) for B2 publication settlement.
