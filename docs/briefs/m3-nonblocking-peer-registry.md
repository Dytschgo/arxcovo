# M3 try-lock applied-registry callback

Sol6.1/high coordinator contract from retained independent source investigation,
2026-11-18. Basegreenmain8b36405, CI44286225997 attempt1GREEN689wall/686charged.
This is the next live-TLS prerequisite after purepeercodec; not a live listener.

## Contract

Add private consensus/peer_registry.rs factory registry(&SmAdapter)->pki::verify::Registry.
Capture only Weak<Mutex<StateStore>>, not a cloned SmAdapter/store Arc or snapshot.
On eachcall upgrade weak; absent=>VerifyRefusal::Unavailable. Use try_lock only;
contention and Mutex poisoning=>Unavailable without recovering the poisonedguard. Call
actual state().authorize_peer(cluster,node,key) while holding this localguard,
mapping WrongCluster/NotMember/KeyMismatch to existing staticrefusals. Dropguard
and temporarystrongstore before completion. No explicit query or persistence I/O, await, queue, retry, clock or tasks; no 
newcapacity. Lookup uses existing boundedregistry. Publicfactory/rawidentity notadded.

Storecontention may reject a legitimate handshake: explicit fail-closed availability
tradeoff in ADR0027. Its temporary upgraded reference can become the final store owner during shutdown; dropping it may synchronously run SQLite/file destructors. Live TLS ownership must account for this race before runtime-worker safety is claimed. It does not wait for the mutex, but no hardwalltime claim
against scheduling/CPU. Retaining/cloning callback cannot retain the store. Actual
Remove is observed on nextcall, with no immutable registry snapshot. Existing
arbitraryRegistry callbacks remain unchanged and may stillblock; no globalTLS
claim. Later TLS configurations must explicitly choose this reviewedcallback.

Localapplied state can be stale withoutbound during isolation. Remove may follow
a successfulhandshake; every futureRPC must reauthorize the retainedtuple before
dispatch. This factory supplies no AuthenticatedPeer, owner/controlauthority,
perpetualauthorization, admission or bootstrapanchors. Anchors remain separately
typed receipt-fenced inboundreplication-only permission, not registry entries.

## Source scope and proof

Allow only consensus.rs oneprivatemodulebinding and newpeer_registry.rs with
factory and synchronous no-Raft tests. Freeze verify/configbuilders/Owners/
fresh_read/issuance/founder/engine/settings/storage/deps/workflows/quarantine.
No source implementation until separate independent contractreview. Mutex poisoning is distinct from the private SQL-write-poison flag; this scope cannot inspect that flag and proves no SQL-store health. Released observes completion of preceding field destructors, not verified SQLite/OS close success.
No TLS socket,
handshake/connectionpool, listener or productionstartup wiring in this slice.

Use actual disposableStateStore and realapplyFound/Remove. Test matchedtuple,
wrongcluster/missing/key, actualRemove observed on reuse, poisonedlock refusal,
storegone refusal/releaseack with callbackalive, and strongcount unchanged by
factory/callbackclones. Hold actualstoremutex, start callbackhelper, observe
Unavailable BEFOREreleasing mutex. Alwaysreleaseguard and joinhelper before
asserting, including deadline/error paths and before every potentially panicking assertion/unwrap; tests use channels, not sleeps/retries.
A bounded test-onlydeadline may detect failure to return; it is not production
policy. Aftercontention/refusal, a laterexplicitcall afterrelease can succeed;
no internal retry. Demonstrate source-state immutability where applicable.
No engine/Tokio/realTLS runtime needed; errors contain staticlabels only.

Afterpositivebaseline/independent source review, root may assign exactlytwo
purecontrols onceeach: try_lock->lock (heldmutex testmustfail pre-releaseassertion,
thenreleaseandjoin); capturestrongstore instead ofWeak (retention/releaseproof
mustfail). Compile/unrelatedred/survival is not intendedkill. Restoreexactbytes
and hash aftereach, retainlogs, no silent runtime rerun or enginefixcounterreset.
D-only lockedofflinefmt/Clippy/focusedtests; allownedprocessesbounded/observed.
Independentactualsource review and normalfirstCI/allfive/newcases eachOS/
featureWindows<=660s/freshgreenmain before coordinator merge.

## Later live-driver work

This removes one reviewed store-mutex wait only. LiveTLS still needs its own ADR
for pinnedasyncadapter vs manualrustls, explicit handshake/connectionlimit/taskowner,
inbounddeadlinepolicy, boundedbuffers and cancellation/shutdown proofs. Outbound
RPCOptionsoft_ttl cannot supply an inbound pre-request deadline. Never silently
borrow or enlarge existing proposal/readunits. ExistingenginecounterSol3/3exhausted
andOpus0/2 unchanged, no founderdiag/fix. M2OPEN/M3UNCHECKED, hardware unverified.
