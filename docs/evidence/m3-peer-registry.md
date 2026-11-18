# Try-lock applied-registry callback validation

Sol6.1/high builder and coordinator,2026-11-18. Basegreenmain8b36405,
CI44286225997 attempt1GREEN689wall/686charged. AcceptedADR0027/contract narrow
scope: no mutexwaiting, not complete nonblocking/runtimeworker-safe TLS.

Changed only consensus.rs oneprivatebinding and newpeer_registry.rs. Factory
capturesWeakstore; upgrade/try_lock/actualauthorize_peer/staticrefusals, drops
localguard beforetempowner. No snapshot/queue/retry/explicitquery/persistenceI/O,
TLSconfig/listener/engine/settings/deps/timeout/quarantine change. Tempowner may
runSQLite/filedestructors iflaststrongowner; futureliveowner mustaccountforthis.
Mutexpoisoning distinctfromprivateSQLwritepoison; no SQLhealth guarantee.
Released observes priorfielddestructors, notverifiedSQLite/OSclosesuccess.

## Builder-run local checks

CWD D:/Code/Arxcovo; cargo-home/target D:/Temp/arx-identity-d-validation-f4e7e857;
TMP/TEMP D:/Temp/arx-peer-registry/tmp; logs.../logs; toolchain1.95 locked/offline,
incremental0. Ownedprocess240sbound, allexited. No Cbuild/worktree/install/cache
cleanup/VM/admin/LabManager/nativeM2acceptance. Synchronous librarytests only.
FmtPASS0.696s; workspacealltargetClippy-DwarningsPASS7.987s; focused
cargo test -p arxd --locked --offline --lib cluster::consensus::peer_registry::tests
-- --nocapture PASS4/4,27.482s including27.01compile, tests0.04s. No compiler or
semanticred. Deliberatejoinedhelperpanic creates Mutexpoison fixture, expected.

ActualStateStoreFound/Remove tests cover matching/wrongcluster/missing/key,
immutableobservation and removedbindingseenbyreusedcallback. Heldmutex test saves
spawn/recv outcomes thenreleasesguard/joins BEFORE assertions; unavailable before
release and explicitlatercallacceptance. Poisonrefusal persistswithoutrecovery.
Callbackclones retainno strongstore; droppedstoreWeakNone/releaseack/staticrefusal.
No engine/Tokio/TLS/socket, realtransport/custody/container/hardware proof.
Hardware **unverified on real hardware**. Localappliedstaleness/perRPCreauth remain.

Blobs consensus8f08c3334d8ac6d3aad804bbfa2566765a92aac7;
peer_registry1d4a4082ed0d9e253625dd34d0884f4347d1e204,
SHA256fdb6ca772ae496b7e795bcb520e882e8a40614402ad9fba07ae93cafadf9771a.
IndependentSol6.1 frozenauditPASS, finalpinnedreport pendingartifactinventory.
Future2controls notexecutedyet. Allnormalgates,new4eachOS,Windows<=660s,
freshgreenmain beforemerge. Sol3/3exhausted/Opus0/2unchanged; M2OPEN/M3UNCHECKED.

## Local artifact SHA256
1afbf52e57e9044b33d26b4a9402d61facca9bd088a0af65bb8b1ac0dc73adf5  clippy.json
cb6366e01787bfac3b7a83c4095ad0aca8ac6e02979c94dc71d1e45ccdbd4fdd  clippy.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy.stdout.txt
87c35da83e9562f066241cb574ef866cdc3370bce93dc017b96b34ba1dc4fcb5  fmt.json
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt.stdout.txt
fcb89d5ed1be9ffca71cd4c24df371649721bd649befa98b28394b23871b7687  format.json
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  format.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  format.stdout.txt
774deabd7941679adfd5d9e86a59f5e823749b5cfee6086e50631739eacb7a69  pure.json
76046240f322b95238d4f5bd2c2c0708bd6e7f1aaf6b01238b9b86e1016aa1bf  pure.stderr.txt
dc21d2110c4df7bddecd21d44d72979ddf9e0f4c4d7061eb9d9cd30e667e7602  pure.stdout.txt
