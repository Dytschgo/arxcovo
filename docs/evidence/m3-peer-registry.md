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

## Two designated controls and restoration

Independent Sol6.1 sourcePASS at3b5cdccc; rootassigned exactly2purecontrols onceeach.
M1 try_lock->lock compiled29.31s; heldmutex test savedtimeout, releasedguard and
joinedhelper, then expectedUnavailable observationassertion142 failed.1test/exit101,
34.838s. M2 Weak->strong compiled30.51s; strongcountassert179 failed2!=1.1test/
exit101,30.963s. Both ownedprocesses exited/no timeout. Rootread allpatches,
stdout/stderr/time/exit and restoremanifests; intendedsemanticassertionsconfirmed.

Beforecontrols, runner replacementmetadata setup failed (PowerShell nestedarray
flattening) BEFOREmutantsourcewritten orCargo launched. Originalissue retained
insemantic-harness-setup.json; correctednamedmetadata. Zeroexecution atsetup;
eachsemanticcontrol ranonce, neither repeated. No baseline/testsource fix.

Exactbaseline restored afterEACH, peer_registryblob1d4a4082ed0d9e253625dd34d0884f4347d1e204,
SHA256fdb6ca772ae496b7e795bcb520e882e8a40614402ad9fba07ae93cafadf9771a.
Rootverified finalhash and empty ALLnon-doc diff vs reviewed3b5cdccc. No postrestore
runtime rerun; retainedbinaryM2mutant. Prior4PASS/Clippy apply bysourceidentity,
normalhostedCI compilesbaseline. Independentreviewapplicability conditions met;
mutantoutcomes attributedbuilder/root, not reviewerexecution. No enginefix or
counterreset, no runtime/CIdeadline/settings/quarantine change.

Additional control artifact SHA256:
678c1b9a8eb260932c558ec1303c3270559573b5442427dc5048f789229c4d1c  m1.json
dcd02e1cc9ff66a8916ae6258f9e44163640bc0550ff1edf830b87e8d18dbe5d  m1.patch
134a85d6ed847a7820384a2d1dc9675730fc7a4af7d6433b37fbe8dd2912b803  m1.restoration.json
f8b2e334fd4677990337803fed3bc0fbaad47ac50f0e0ba6c690730ba89282b4  m1.stderr.txt
da9b01fe7a016aeb543893fd1ac661acfde20e44097a75ceff59aa356f86b338  m1.stdout.txt
66d86d540b593b1d15419bb5ab0e0ef9b1ddfc93bf77b896383bbb8308b554e3  m2.json
4b4843782377f8b5b4a36e9af0cb7bf2c8b21b19295ff68629ec2b70406fb0d5  m2.patch
813e2681b0dfc497412a38a8dbbfaddcb5224115f7574fe2df18947c7a010ca7  m2.restoration.json
f97a91dc5c74769dadffe632a1b2ea1800ca931668bcc83db8564e234d9e92fd  m2.stderr.txt
98945dc1077d8dd38a5f4b0cc325e603881f7426bf5cee84818aa00fb0b55b8e  m2.stdout.txt
57292adfd91e73002886edf25617bda25aba3da9057663ba5741e7231f07dfd7  semantic-baseline.json
91a16f8d460a9863b6a0296b4bf2adbc7fc427b100d2702c6e5342dbb07a688f  semantic-harness-setup.json
