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
