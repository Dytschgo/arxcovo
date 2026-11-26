# 0072 VM store v16 and native TEST custody checkpoints

Status: implemented with production TEST admission closed. Extends
[ADR 0054](0054-m4-dedicated-windows-runtime.md) (7c amendment, chunk 05)
and [ADR 0071](0071-vm-store-v15-compose-backup-retirement.md).

A native TEST effect creates the generation directory, its
`generation.lease` and each disk, VARS, seed or verification leaf by
relative create-new. The existing artifact row cannot hold an identity
before content exists. An empty object fails its `bytes>0` check, and the
row has no store, root or lease scope. A crash between create and the
first durable record left an object nobody could attribute. Store v16
records each intent and identity in the ledger before the next mutation.

## Ownership

Only the 7c issuer writes these rows. It writes them through the private
checkpoint API in `store::vm::native`, which the production adapter
reaches through `CustodyCheckpoints`. Each call commits one exact
transition in its own immediate transaction before the effect continues.

- `vm_native_generations` has one row per generation. The row binds the
  store host (`vm_state_host`), the production root volume and file ID,
  the origin protocol (`build`, `clone` or `restore`, checked against the
  generation's owner) and protocol version 1. It records the state
  `intended`, then `directory` with the directory ID and the lease create
  intent, then `bound` with the lease ID.
- `vm_native_slots` has one row per slot. A composite foreign key ties it
  to the exact `vm_artifacts` generation, role and key, through the new
  unique index `vm_artifact_slot`. Its states are `intended`, then
  `identified` with the file ID before any write, then `verified` with
  length and SHA-256 after write, sync and held content verification.
- An intent becomes `absent` only after an exact `NotFound` through the
  held parent. The observation is a bounded, attribute-only, no-follow
  probe of either object kind. A present object with no recorded identity,
  including an occupied leaf or an object of the other kind, becomes
  `quarantined` and is never adopted.
- Workspace settlement names the observed object (directory or lease) and
  carries its identity. A lost acknowledgement of a committed directory
  checkpoint finds the recorded directory and keeps `directory`. The live
  owner's next reservation resumes at the lease without creating the
  directory again. A missing or replaced recorded directory, or a present
  lease without an identity, is quarantined. An absent lease keeps its
  intent.

The four leaf classes are therefore: never attempted (no row), intended or
absent under exact proof, unidentified present, and identified or verified.
Only the create-to-checkpoint crash window can leave a new unidentified
leaf.

Recipe staging verifies the seed in the ledger before the VARS intent
exists. A failure on VARS still leaves the seed accounted. Holding a
workspace requires the exact bound scope.

The helper target is precreated empty and identified before any helper
opens it. A checked Mutate launch accepts only that typed `HelperTarget`.
It must be the plan's only writable input, at its exact slot path and
identity, and still held by the workspace. The ledger must still record it
as identified and unverified. Every other plan kind refuses a target.

Checkpoints refuse once the originating work no longer owns the generation:
a pending build with its pending build operation, a creating clone, or a
copy-phase restore work row with action 6 and its exact pending restore
receipt. The disposal fence interrupts that receipt before filesystem I/O,
so a stale restore worker loses custody authority even while the work stays
in its copy phase. A snapshot-action copy row never owns a restore
candidate. A stale worker after an abandonment or restart cannot add
custody. Exact reconciliation of an intent stays available.

Legacy paths acquire no authority. The migration infers no binding for an
existing row. A generation can be bound only while every artifact is still
`planned`. The restricting foreign keys refuse `forget_generation` and
build cancellation over native rows, and those transactions roll back as a
whole. The intent also moves the artifact row from `planned` to `created`.
Ordinary recovery therefore keeps attempted leaves charged and quarantined
under the existing rules. A trigger fences legacy identity writes on an
artifact row with native custody. Its volume, directory and file IDs may
only mirror the native slot's recorded identity and the bound scope.
Legacy `identify` can neither pre-empt a native identity nor replace it.

## Bounds

Roles are the four TEST slot roles. The primary key `(generation, role)`
allows at most four slots per generation, and generations remain bounded
by `vm_generations`. Identities are 16-byte NTFS file IDs, plus an 8-byte
volume shared by root, directory, lease and slots. `UNIQUE(generation,
file)` stops one object standing in for two slots. A verified length lies
between 1 byte and the 41 GiB disk ceiling. Triggers make recorded scope
and identities immutable and refuse slot rows for an unbound or disposing
generation.

Chunk 07's checked disposal uses the `disposal` columns. On a slot the
column is `intended` or `absent`, and it requires a recorded identity. On
a generation it is `lease`, `directory` or `absent`, and it requires
`bound`. A trigger refuses to advance generation disposal while any slot
remains present. No production path writes these columns yet.

## Upgrade and rollback

The migration is additive and runs inside the existing upgrade transaction
with its foreign key check. It does not rewrite any v15 row. The
compiled-schema test pins the exact DDL text of every new object. The
old-data test preserves an identified legacy row with its identity, digest,
length and charge. Older openers refuse v16. Rollback is forward-only: stop the
controller and restore a matched offline copy of the pre-upgrade state
directory. No down-migration exists. Production TEST admission was closed
at v15, so no native row predates this version.

## Owner-local reconciliation

An `intended` row without an exact absence proof, or a `quarantined` row,
keeps its charge. The owner reconciles it locally with the controller
stopped, using the same non-elevated account that owns the store. The
reconciliation names the exact charged generation and slot. It verifies the
store host, production root, directory and volume identity, and that no Job,
worker or handle can still write. It preserves the object as evidence. It
converges through the typed settlement path that chunk 07 adds. Confirmed
foreign objects stay outside daemon deletion authority. The procedure grants
no agent tool and no generic deletion permission. The runbook entry lands
with that settlement path.

## Chunk 04 lease handoff

The start refusal from chunk 04 keeps its originally held workspace lease.
It is bound at construction to the exact `StartPair`: the disk and VARS
slots with their evidence. The workspace binding it holds is now the durable
bound scope. `confirm_start_pair` accepts that pair only in the held
refusal's shape: a disk or verification overlay first, then VARS, with
distinct keys. Both slots must be identified or verified in this scope with
the same file IDs, directory and volume. The held release proof still compares content through
`matches_start`. A refusal from before acquisition spends its one-shot
attempt within its original cutoff. A production caller makes that attempt
through `hold_workspace`, which requires the same scope. The
receipt-independent release commit in chunk 06 validates its proof against
these rows.

Native creation, holds and reconciliation are **unverified on real
hardware**.

## Released TEST writer epoch (7c chunk 06)

The caller keeps the generation lease and both writer exclusions through
bounded disk and VARS hashing and the immediate ledger transaction. The
transaction checks the active TEST generation, exact writer epoch, v16
custody scope, both active role identities and the retained pair charge.
It records the observed pins and clean or dirty release together.
Identical proof can replay after a lost commit acknowledgement. A
different epoch, disposition or pair cannot replace that proof.

Stop admission stamps its optional receipt with the active generation and
writer epoch. The transaction samples the receipt clock after acquiring
the write transaction. Only a matching, pending, on-time clean stop can
succeed. A late or interrupted receipt, or a stop with no receipt, still
leaves the exact release recorded. Historical receipts retain their
original disposition. Dirty and overdue active instances remain charged
for ordinary recovery or expiry.

A failed transaction preserves the borrowed evidence for retry. The
unreleased epoch, bound slot identities and unchanged claim remain the
durable retry pointer if the caller yields. The adapter performs the
transaction outside the in-memory run map. Before activation only scoped
fixtures reach this adapter. Native release is **unverified on real hardware**.

## Resumable TEST disposal (7c chunk 07)

Chunk 07 consumes the v16 `disposal` columns through `store::vm::native::disposal`
and `production::disposal::NativeDisposal` without altering the schema. Every
slot deletion follows `NULL -> 'intended' -> 'absent'` under a checked
`DisposalScope` shared by transient-build disposal and ordinary sweep. Empty
bound generations follow `NULL -> 'lease' -> 'directory' -> 'absent'` under the
held production parent and an in-process per-generation guard spanning both
handle handoffs. `settle_cleanup`, `forget_generation` and `publish_build`
revalidate durable absence before removing native rows or releasing claims.
See the chunk 07 amendment in [ADR 0054](0054-m4-dedicated-windows-runtime.md)
for the exact handle access/share contract and absence rules. Native disposal
is **unverified on real hardware**.
