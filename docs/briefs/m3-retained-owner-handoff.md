# Retained offline owner handoff

Accepted design,2026-11-19. Frozen contract
`D:/Temp/arx-m3-owner-handoff-design-v2/contract.md`, SHA256
`f7e670d91c9165444cee2ff62dccbb5c2e1f32dfb5dad4752e662b23de78436b`.
Independent Sol6.1/high re-review GO, Standards0/Spec0/ST1-SP1-SP2closed at
contract level: `D:/Temp/arx-m3-owner-handoff-review-v2/report.md`, SHA256
`3c28ec551f2af4cf849de0b6f4c9859b598aa81a68dadda2ce9c7b487a2b63fb`.
Source target8bd12b7f; its firstCI44450232774 qualified568wall/563charged.

Implement only startup_plan.rs and consensus/startup_recovery.rs plus inline
non-Engine tests. Preserve prepare behavior; add private intent validation,
ownership transfer/borrowed checked inputs and controlled fixed-delta capture.
No raw owner extraction, Clone/serialization/unlock/reset or operational ready API.

The complete frozen contract controls exact APIs and the journal/fixture matrix:

- Validate mode/role/cluster/node/both canonical addresses before mutation; recovery
  uses its verified binding, founder requires node1/retained fresh cluster, Join refuses.
- Private Ready/Begun/Failed(first refusal), Begun before the first opener. Retain
  Prepared outside caught creation; sticky failure rejects before any later access.
  Success consumes it; no retry/repair/delete/recapture or process/rollback promise.
- Initialize only genuinely fresh founder state through existing real stores;
  verify exact untouched genesis, no applied/membership/vote/log/purge/Found.
  Recovery/UntouchedGenesis do not create or capture additions.
- Preserve original guards/lock; stage only DB/lock/optional-zero-journal delta
  while actual stores are owned. Atomic install; exact absent/zero identity masks
  survive drop/immutable inspection. Any transition or sidecar requiring recovery
  refuses. If the real positive cannot survive, stop/narrow rather than weaken.
- Real lock/identity/unchanged-refusal/store fixtures and positive counterparts,
  all journal masks, staged failures and caught unwind; sticky error after explicit
  fixture restoration. Unix mutation must execute; Windows denial is prevention only.

Use only named new pure/filesystem tests and directly affected existing non-Engine
tests, fmt, locked/offline all-target Clippy and applicable static guards. Preserve
first failures; after3 failed fixes same issue stop for independent disposition.
No local Engine/founder/native/daemon/client/network/container execution. Build
receives no behavior credit. Independent exact-source review/normal hosted gates
remain required; the design review grants neither operational nor merge acceptance.

Actual SQLite/Engine-opened-object binding, pre-return cleanup ownership, qualified
custody/CA constructor, actual Found publication and real retained job remain
unavailable. No frozen-core/config/transport/dependency/SDK/timeout/capacity/
quarantine expansion, fake lifecycle/publisher or new permission flow.
All TMP/TEMP/target/home/cache/observer/scratch D:,Cfree>=20GB before builds.
No admin/LabManager/state/VM/cache/userdata/OAuth/babysitter actions.
M2OPEN/M3UNCHECKED; **unverified on real hardware**; exhausted engine counters unchanged.
