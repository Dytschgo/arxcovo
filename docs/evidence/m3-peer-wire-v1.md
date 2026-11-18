# Peer wire codec local validation

Builder Sol6.1/high; coordinator Sol6.1/high,2026-11-18. AcceptedADR0026 and
m3-peer-wire-v1 brief define exactscope. Main6bf8f46 CI44276313116 isGREEN.
PR180docsheadb1cf9d50 CI44279691035 executedWindowsjobtimeout remains retained;
this is genuine codec implementation, not thattimeoutrepair, docs-onlytrigger or
sameheadrerun. No runtimefix/counterreset/timeout/quarantine/settings change.

Sourceonly consensus.rs oneprivatemodpeer_wire and new consensus/peer_wire.rs.
Header44 ARXPv1, request/reply caps264068/26, exactframe/trailingrefusal, canonical
command/member reuse and nativepositions/votes, exactAppendResultNone/partialack/
conflict/highervote. Allpositionsdurableindexbound; nativezero term/node syntax
preserved separately from nonzero routeclaims. Codec grants noauth/dispatch;
no sockets/TLS/engine. Existing runtime/storage/Owners/TLS/dependencies frozen.

## Builder-run checks

Allcommands fromD:/Code/Arxcovo. Cargo home/target reuseD:/Temp/arx-identity-d-validation-f4e7e857;
TMP/TEMP D:/Temp/arx-peer-wire/tmp; logs D:/Temp/arx-peer-wire/logs; toolchain1.95,
locked/offline/incremental0. Ownedprocess240s bound, allexited. No Cbuild/worktree/
install/cachedelete/VM/admin. Sourcebuildonly, no localWindowsengine.

Initial Clippy compilation found6derivedgeneric-signature errors: native Entry/
EntryPayload generics differ from assumedArx parameter. Mechanicallycorrected
newmodule to EntryOf<Arx> and EntryPayload<Command,Members>, before anytests.
Corrected cargo fmt --all --check PASS0.726s; cargo clippy --workspace --all-targets
--locked --offline -- -Dwarnings PASS8.815s. Focused cargo test -p arxd --locked
--offline --lib cluster::consensus::peer_wire::tests -- --nocapture PASS8/8,
32.420s including31.97compile; actualtest execution0.02s. No semanticred.
Coordinator read final8namedPASS output, Clippy timing/exit and gitdiffcheckPASS.

Tests cover independentexpectedheartbeat/mixedBlank+allcurrentCommand+Membership
bytes, exactnativeoptional/fieldchanges/replies, maxcount/headercaps, canonical
entrybounds/legacytag, truncation/header/optionmalformation, durableindexlimits/
nativezerofields and staticerrors. No Raft/Tokio/socket/TLS execution. Fulltransport/
quorum/join/container/hardware acceptance notclaimed; hardware **unverified on real hardware**.

Sourceblobs consensus.rs8fc252170acc819a7fb4685e0c34e465ccbcc660;
peer_wire.rsb0a2b56494c6614a41a50663afbee7bc287f1325,
SHA256fefe3f76d21a0071bbb23f7ffcbabaf22f7ffe1c1b31f9fa6aeafcd50e2ee868.
Independent Sol6.1 actualsource reviewpending; planned3semanticcontrols notrunyet.
NormalnewsourcefirstCI/allfive/new8eachOS/Windows<=660s/freshgreenmain beforemerge.
Sol3/3exhausted/Opus0/2 unchanged; M2OPEN/M3UNCHECKED. No founderdiag/fixauthority.

## Local log artifact SHA256
0b3b9d112ac443bb450a4178adbef66790a902a119c8464b7e4a6d8ce48f07bf  clippy-corrected.json
758eb6d9af361d8f88f00ae12ffb4af8171f04c083c113063aadceff4e34bb2d  clippy-corrected.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy-corrected.stdout.txt
c029459e989d8dc8d2c4a949fb842bf915c0e90abb441a22ec53e9c23671554f  clippy.json
f3341250abd365c2356b8220ee4bd9d477e3cb287da7f6d2a9c0fcebc58e35a2  clippy.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  clippy.stdout.txt
22aee924395a0982ec5cc847b2bab234c72ac5fbf8888062161dfb4b844a3230  fmt-corrected.json
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt-corrected.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt-corrected.stdout.txt
3bfb076dab5d190c934b6ee7758bc3413ca8e464c4f7748231979176bc076c14  fmt.json
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  fmt.stdout.txt
a67091dbd797e7726a7cc45374de2f03ebf49d876e03ac8cd1afa234b95c664a  format-corrected.json
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  format-corrected.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  format-corrected.stdout.txt
111e1aee4d43f426555eb1931a1648c660231b49d2eac0f0edbc2a7e51dbd8fc  format.json
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  format.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  format.stdout.txt
6adf13534dc8e9921906592a8fff4281b74773307c765958c56b667f6c61e605  pure.json
50d5c6663de0f9d70f55a3e10b6253db525da3f1569492c36ba1f29ffb8f8b3a  pure.stderr.txt
12fffb17f8af0f818237f9cbcc55ea72fcb60905e6abaf78fc51fb3e2160f14f  pure.stdout.txt
