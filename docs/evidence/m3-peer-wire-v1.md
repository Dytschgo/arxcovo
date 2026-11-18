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

## Independent review advisory and final baseline refresh

Independent Sol6.1 actualsource audit found no blocking issue, with one test
coverage advisory: correctlyadvertised request payload with count0 plus an extra
byte lacked a direct assertion for requestinner fullconsumption. Coordinator added
three testlines only, no productionfunction change. Reviewer rechecked actualdelta
and closed advisory at moduleblob9322c5c6694da224ed7687e21a31e9ea5ede8b19.
Root ran D-only fmtcheckPASS, workspacealltargetlockedofflineClippyPASS7.95s, then
same focused8/8PASS (testcompile28.30s). Allrootcommands/session exited normally;
no fixed240s enforcement claim for this rootrefresh. Source stayed withinallowlist.
Original testbaseline remains separatelyrecorded; newtest delta justifiesrefresh.

Refined3planned controls per independentreview: omit decoder countguard only,
omit requestinner fullconsumption only (outerguard retained), forceacceptedSome
ack toNone. Each once afterpassingbaseline, compiled intendedassertion only;
exactrestore aftereach; no postrestore runtime rerun. No controls executed yet.

Additional rootrefresh artifacts SHA256:
f5de70c16b9360707d16ff2015cbe07e06eae66a721c565ef0944eebed4d9a54  root-clippy.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  root-clippy.stdout.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  root-fmt.stderr.txt
e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  root-fmt.stdout.txt
acdd6a264869ef5ea95e7a20db197cc16dd390210200b345c48944a4a38a5d5e  root-pure.json
5c1df12c2406a3cad7130b623d7e4383c57b82f5fa0d2f8c536d973ee8b78d15  root-pure.stderr.txt
7d908ea486f6772e7a8b9976a103057273acbc5e4c1f8461ae399a664f3cd6ba  root-pure.stdout.txt

## Three designated semantic controls and exact restoration

After independent sourcePASS and strengthened8/8 baseline, coordinator assigned
builder Sol6.1/high exactly3pure controls once each with240s ownedprocessbound.
M1 omitted decoder countguard only: constructed65-entry request assertion failed
at535 after27.53scompile, wall27.980s. Encoderguard stayed intact.
M2 omitted requestinner fullconsumption only: correctlyadvertised count0+extra
payload assertion failed at578 after28.80scompile, wall29.287s; outerguard intact.
M3 forced acceptedreplyposition toNone: independentexpectedbyte equality failed
at643 after27.74scompile, wall28.222s. Each exacttest1run/exit101, no unrelatedred.
Allownedprocessesexited, no timeout. Coordinator personally read all3patches,
stdout/stderr, exit/time and restorationmanifests and verified intendedassertions.

M2 originally completed its intended semanticfailure, then its runner checker
expected an outdated assertion string and stopped. Originalrunner and separate
m2.checker-issue.json retained; only checkerbookkeeping for unrunM3 was corrected.
M1/M2 NOT repeated. M3 executed once in separate M3-only runner under explicit
coordinator steering. This is evidence-parser correction, not a source/testfix.

Exactbaseline restored afterEACH: peer_wire Gitblob9322c5c6694da224ed7687e21a31e9ea5ede8b19,
SHA256ffb505d40c787658821ea1b4fb0e1b6dbf94b7657db842a8166d9e1395cb9c7f.
Rootverified finalblob and empty ALLnon-doc diff from reviewed6a27902c.
No postrestore runtime rerun; retained localbinary isM3mutant, notrestoredsource
proof. Priorbaseline8PASS and Clippy apply by exactidentity; hosted compiles the
restoredsource. Independent sourceverdict applicability conditions satisfied;
futuremutantresults attributed to builder/coordinator, not reviewer execution.

No engine/transport/socket/TLS/runtimefix/timeoutchange/stabilitycredit. Sol3/3
exhausted/Opus0/2unchanged. OlddocsCIWindowscancelled remainshistoricalblocked
attempt; a substantive codecnewhead needs normalfirstCI/allgates/new8eachOS/
Windows<=660s/freshgreenmain before merge. No same-head rerun or docs-onlytrigger.

Additional control artifact SHA256:
c7a25ff30b4c512575b10f844b7434142f32217809ea287705e726c86f0458a5  m1.json
7d60fc0043f94a38301c5cf668817b1bca32549a3dd75b9ada6d3280d46441c5  m1.patch
70e6665277f855186f554f8999af2cbf46520c837a150dc69bca3f28cfe10c02  m1.restoration.json
9da5419260fe0230d767967c4d5f92924f96812e763daf14aedce1f12ba9ea6a  m1.stderr.txt
9bce27d02aee2c32d763e0f9a22adf8eaa054b16ba1ba046140d46996701a530  m1.stdout.txt
a8a9982c0d727c18da386ffddfd5256f5a73f04e78fdce9ef8afef54e41ed1f7  m2.checker-issue.json
216899f522ed3bffe18afede46280ed6b78c29f13008ab75ccb33db619aa8e31  m2.json
ab195da4ca1c6e64b6c8c4be276b0f0d92e77683b149b98a950cc1f286565148  m2.patch
1f5d471b18a5ac5d374903914d3abc7890ec18ef8ff312d9487991862c37462f  m2.restoration.json
2b57119d33536ec085bbe60d270969a71ed7f2ac901605b5556bf977667989bd  m2.stderr.txt
3979da977af39953afc18ca3f5ca2be0631a88dcb07a99a421241c23be6858f4  m2.stdout.txt
c1f3d3f535cba523c17817987fe69696495765e8a1a2d1d584ac0d3db83c97c7  m3.json
9ed73eb69c90c50d814e548a5998214450fab80c9cca7f40f1626ba93a1702bb  m3.patch
07a42ecd09a680e9032e8b35209adc54c6f425decaf5a7e92f640cef97ec8996  m3.restoration.json
55099eee695adc433cdc1b837375901a4b6b77aa67f620fc29d72351befdddec  m3.stderr.txt
9f7808814187f762b3cae8fba5adb97c74c24662b83cb0b0d3db3c1738983496  m3.stdout.txt
3491387ea996c8f0fd90404ed71fdb51e8492a27c62e911d968483f63ef61970  semantic-baseline.json
37177aee6ce9e5ccd70805284653ea11236fb9a7847a0f67daef3ba7c206408d  semantic-m3-baseline.json
