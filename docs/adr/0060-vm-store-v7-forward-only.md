# 0060: VM store schema v7 is forward-only

Status: Accepted (2026-11-23). Context: PR #275. The statements below are the merged VM store in [vm.rs](../../crates/arxd/src/store/vm.rs) at main `f4360120`. `MIGRATE_V7` was introduced in PR #267 (`afedc4f9`). PR #275 (`2afb6704`) is the reaper, expiry and shutdown split recorded with this decision.

## Decision

VM store `user_version` 7 adds `cleanup_confirmed` to `vm_operations`, the operation ledger. The column is an integer constrained to 0 or 1. A non-NULL value is allowed only when the row status is between 1 and 3.

A database at version 6 applies that addition. A database at version 5 applies the existing v6 replacement first, then the v7 addition. Version 0 is accepted only for an empty database: the opener creates the v5 schema, then applies v6 and v7. A populated version-0 database fails the empty-schema check and is refused. The backfill copies `cleanup_confirmed` from a still-retained `activity` row when that row is the ledger row's activity, the actor is `agent`, the principal, VM, action and status match, the ledger status is between 1 and 3, and the activity value is not NULL. Caller receipts keep this proof because audit rows are pruned. NULL means the proof is unavailable, and the receipt omits cleanup.

A clone receipt takes its template from the immutable ledger subject. The store's receipt projection uses `subject_id` only when `action` is 1 and `binding_kind` is `template`. A legacy clone bound to a reusable VM ID has no template, and the receipt omits it.

The migration is forward-only. The opener accepts `user_version` 0 (empty database only), 5, 6 or 7 and rejects any other version. An upgrade sets `user_version` to 7. There is no downgrade. That is the same rule the module already states for the v6 migration: a v5 opener rejects v6, and a v6 opener rejects v7. Upgraded state needs a matching or newer opener.

## Consequences

This record does not accept M4. Cleanup proof already pruned from `activity` stays NULL. Native VM paths remain **unverified on real hardware**.
