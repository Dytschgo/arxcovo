# 0066. Offline VM snapshots and VM store v11

Status: Proposed (2026-11-24)

## Decision

An offline snapshot contains an independent copy of a same-VM disk and its private UEFI variables. TEST restores keep their original VM identity, golden lineage and lease. Static restores retain the displaced pair as a protected restore point. The static TLS name derives from the persisted first active generation, and the physical generation and boot witness still change on a subsequent start.

The authenticated foreground advertises four snapshot tools. Mutations use a domain-separated request binding and the existing caller operation ledger. The ledger retains snapshot result IDs independently of catalog and audit rows. List reads are limited to four points and bind their cursors to the host, principal, tool and VM. A caller can delete a sealed TEST point after release proof. Static restore points have no agent deletion route.

The VM store retains the v10 publication ledger and checks the typed publication intent and pending receipts in the snapshot transactions. A static operation takes the B2 effects slot after the foreground dispatch gate, checks the activation registry and local connector, and drops those short guards before entering SQLite. The effects slot remains held through copy settlement. Restore leaves the VM stopped with public routing unchanged. A future publication requires its own admission from a newly proved origin.

The worker records each staged file identity separately from its content evidence. A restore transaction verifies the expected active generation, writer epoch, selected point and operation, then changes the active generation for both files. TEST retirement carries the old charge until disposal. Static retirement transfers the old charge to a protected catalog entry. TEST expiry records a bounded non-active work set and reconciles exact pairs before the ordinary active-pair teardown. Unknown objects and unproved cleanup remain charged.

Native issuance remains closed. The foreground fixture uses private synthetic media and never grants authority to issue an offline lease or launch a real VM. Native path custody, qemu-img inspection, copy and retirement durability, Windows and Ubuntu cold boots, and Cloudflare route removal are **unverified on real hardware**.

## Upgrade

The opener accepts an empty version 0 store and versions 5 through 11. It applies the pending migrations in order under the immediate transaction, checks state headroom and foreign keys, then publishes version 11. Version 11 preserves activity IDs, their high-water mark, B2 intent and settlement evidence, and legacy operations and snapshot charges. The legacy ready bit supplies dashboard history. Typed requests use a distinct binding and only sealed restore points in the new catalog. An older opener refuses version 11.

Static origin identity backfills from the active generation in existing stores. The first activation assigns it on new instances. The later activation of a restored generation leaves the provisioned identity intact.

See [ADR 0063](0063-vm-store-v9-forward-only.md) for static generations and [ADR 0065](0065-vm-store-v10-publish.md) for B2 publication settlement.

