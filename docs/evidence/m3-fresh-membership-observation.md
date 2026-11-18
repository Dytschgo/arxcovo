# Fresh membership observation local evidence — 2026-11-18

Builder Sol6.1/high; coordinator Sol6.1/high. Basegreenmain6f4618c from PR178,
CI44269052508 attempt1GREEN603wall/600charged. Independent M3 underADR0023;
M2OPEN/M3UNCHECKED. No founder repair/diagnostic or engine execution authorized.

Implemented only consensus.rs one module binding and new membership_observation.rs.
Retains authenticated leaf tuple; bounds full effective/committed memberships before
clone, samples committed vote/currentleader; same existing fresh_read store guard
validates applied membership and registry. Exact IDs AND contents, uniform config,
watermark, established read leader and stable second sample required. No retry,
clock/deadline/capacity/Owners/fresh_read change. Guarded watermark is not atomic
across metrics/store, perpetual authorization or production admission. Result
retains no store/connection/permit/task/key. Static errors only.

## Builder-run validation

Working directory D:/Code/Arxcovo. Cargo home/target are existing
D:/Temp/arx-identity-d-validation-f4e7e857/{cargo-home,target}; TMP/TEMP under
D:/Temp/arx-membership-observation/tmp, logs under that task's logs directory.
No C builds/worktrees/install/cachedelete/VM/admin. Toolchain1.95.0, locked/offline.

- Initial workspace Clippy had2type inference compile errors in test maps, zero
  tests executed; corrected only explicit BTreeMap generics in newmodule.
- Corrected Clippy passed. Initial seven-test pure baseline passed.
- Coordinator requested stronger actual API test for synthetic peer; builder
  polls actual observe_membership once with no engine/store/runtime/zero units,
  asserts immediatePeerRefused, no blocking/owner work. Existing test strengthened.
- Final cargo fmt --all --check PASS; final workspace all-target locked/offline
  Clippy -Dwarnings PASS7.63s (builder timing).
- Final cargo test -p arxd --locked --offline --lib
  cluster::consensus::membership_observation::tests -- --nocapture:
  PASS7/7,26.064s including25.59compile. Seven exact names inspected in retained log.
- git diff --check PASS. No semantic red, mutants or actual engine execution yet.

Pure tests use real Found/Remove apply and canonical membership; cover exact
positive observation, immutable state, effective/committed ID/full-content drift,
learners/addresses, missing/joint/equal-index watermark conflicts, vote/follower/
establishedleader/bracket drift, wrong cluster/key/missing/removed peer, direct
synthetic API refusal, metric bounds and irrelevant progress exclusion.
They cannot prove actual quorum/adapter races/cancellation; future live proof
is required before admission uses this API. Existing read ownership evidence
reusable only with unchanged fresh_read source. Hardware **unverified on real hardware**.

Source Git blobs:
consensus.rs11c515954f057dd7c31acbfa1940d9270b95c662
membership_observation.rse925a7e23db98f6502e5d8554c5f051f1e6b9851
Independent Sol6.1/high actual-source review pending. Allfive gates,
freshgreenmain/Windows<=660s required before merge. Sol3/3 exhausted/Opus0/2 unchanged.
No production endpoint, owner authority, transport/custody/expiry or acceptance claim.

## Retained local artifact SHA256
6e9f1d33ed2da390de7833f03ac20b1ca6779531a31b813a57c410e9838d9e52  clippy-corrected.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy-corrected.stdout.txt
a68c6e134da47fa21db1e8014958e05eb6ff8c3e62863c7427a715c36663bd0e  clippy-final.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy-final.stdout.txt
f09cc319d35bb7d40cc66619760770f721dc73eb9507323a24ad658e8ba90a73  clippy.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy.stdout.txt
3a86fb50350684e73dd8ed4979467c39f09814d4fc2d6ea9259bfcfed614325b  pure-final.json
85bd3550104578afc60f53d000dd7b7398c220f9c0ce8e283eb62cbd51a16756  pure-final.stderr.txt
33222685b25f1f3f2b4d4ad0a500b8bb6ead909babbef3e08842dd840b9f674b  pure-final.stdout.txt
346e5c94ee26f9256068a6f2e1f2d31acf96ebd4c4701791d6328e35c06dbc1d  pure.json
028166772a3ac65212df86aa6d866c923f2a64df60fbe2c3066549dd56d742bd  pure.stderr.txt
33222685b25f1f3f2b4d4ad0a500b8bb6ead909babbef3e08842dd840b9f674b  pure.stdout.txt

## Three pure semantic controls and exact restoration

After independent Sol sourcePASS, coordinator assigned builder Sol6.1/high exactly
three brief-designated pure mutants, each once with240s ownedprocess cap. All
compiled and ran exactly1specifiedtest, exiting101 on intended assertion:
M1 omit both full-content comparisons, retain IDs -> full_effective_and_committed_identity_and_contents
assertion269 failed;25.985s. M2 omit authorize_peer -> real_registry_binding_and_removal_refuse
expectedPeerRefused assertion339 failed;26.885s. M3 omit establishedleader equality,
retain second-sample comparison -> leader_epoch_and_bracket_changes_refuse
assertion315 failed;26.643s. These are meaningful guard-omission kills, not engine fixes.
All owned PIDs exited. Coordinator personally read all3patches/stdout/stderr/
exit+timing and restoration manifests. No compile/unrelated failure counted.

Exact source bytes restored in finally after EACH. Each restoration manifest
records baselineSHA256617d60d7aa7d8156404180f4820992973e43bbf410539fe406174703eae9bcc8,
Gitblobe925a7e23db98f6502e5d8554c5f051f1e6b9851. Final coordinator hash and empty
non-doc diff vs reviewed1d068d5f confirm exact final restoration. No postrestore
runtime rerun; retained binary is mutant3, not restored-source proof. Earlier
baseline7/7 and final Clippy evidence apply by source identity; normal hosted
checks compile restored source. Independent sourcePASS applies under its explicit
exactrestoration/docs-only condition, not an independent audit of future mutantlogs.

Additional artifacts SHA256 (local logs root unchanged):
06f9bbad30426d47f53b1f36330d70e38318d44275946caf5aed93e834bb0ffb  m1.json
44814203dbb385c78baff3e5108f8214e6ca93e2cc5bee5ead80bf0552955fa0  m1.patch
44fbc30cb515c2cfa98e61fe6f231d74f3594e2667a344063c7501aff4ed1c71  m1.restoration.json
403121b10e2a3cbe250d9203e094afd8f5630fec0d99742d5fdec395d97fb808  m1.stderr.txt
acaa6f1d198277e257fc2d06c56de9b0fbc75c1a5b85d665ad9b45bccc6f5a58  m1.stdout.txt
f464c03db1c3e2222072e757eaa902909286ac304211f894f9b2b92a950fe766  m2.json
5d2e1520434bd119da113e5bbf9f1554c971b82882afe91bddf95f5814629e89  m2.patch
bcd9569241183c7be444e768f336609d3676bc5b85803a3e2977c650394d64a7  m2.restoration.json
cc6ed684b8294c22774916cfeac69c48bb7243c26ba9cc906ff7d62b09a9046c  m2.stderr.txt
aaa940fcef188b7ffd34be9a3baedee38aef5439f3e3e4681cff03dab26adefc  m2.stdout.txt
ae393b27a9f730c22fdb4f055dc443eb303db817aaf71f572d6c6c86abd407c9  m3.json
b018233ddf9ed9b18c6768f0c76f70630ffec9b95ca4f066e06226423ba67965  m3.patch
e8457ec96d672c59f91121b7baeede5d9d9f7431b30b247ac1d59fa32e4a93ae  m3.restoration.json
fa92079cc65bf113ea813d9eca4aef6db4c0634050d53eebb10eaae530e41878  m3.stderr.txt
d67f0e69340e2c4869ca3d119a81c7ccca88619700c83598fa5cd476b75a3553  m3.stdout.txt
4a478239e5e442ad670eb4b735e44d5bcf5b925a9cb4676f04723639e8f96741  semantic-baseline.json
