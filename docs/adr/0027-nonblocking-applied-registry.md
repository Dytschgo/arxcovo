# 0027: Fail-closed try-lock applied-registry callback

Status: accepted for the no-mutex-wait callback prerequisite only after independent Sol 6.1/high review,2026-11-18.

The current TLS verifier accepts a synchronous Registry callback. A callback using
StateStore's blocking mutex can wait withoutbound; an asyncTLS wrapper cannot
cancel that synchronous wait. The first live-transport prerequisite should remove
that wait from the specific callback selected for transport, without rewriting
arbitrary callbacks or claiming complete TLS ownership.

Propose a private factory capturing Weak<Mutex<StateStore>>. Each call upgrades,
try_locks, checks actual localapplied authorize_peer, and releases temporaryowners.
Storegone, contention and Mutex poisoning failclosed with existingUnavailable. No cached
snapshot, task, queue, capacity, dependency or timeout is introduced. The callback performs no explicit query/persistence I/O. Its upgraded temporary reference may become the final store owner at shutdown, and dropping it can synchronously run SQLite/file destructors. Live transport ownership must handle that race before claiming runtime-worker safety. The deliberate
tradeoff is transient handshake refusal under legitimate storecontention. Callers
may make a later explicitattempt; this callback never retries internally.

This preserves localapplied staleness and later revocation races; futureRPCs still
need reauthorization. It grants no owner/admission/bootstrap authority. Retained
TLSconfigs must not prolong storelife through the callback. Existingverifier and
configconstructors remain unchanged. Only this callback has non-waiting mutex
semantics; no hardwallCPU/scheduling or arbitrarycallback guarantee is claimed.

[Bounded contract](../briefs/m3-nonblocking-peer-registry.md) pins factory/scope,
realstore tests and futuretwo meaningfulcontrols. Accept after independentreview;
liveTLSdriver/connectionownership/inboundpolicy remain a separate ADR and proof. Mutex poisoning is distinct from the inaccessible SQL-write-poison flag, so no SQL health guarantee follows. Released observes preceding destructor completion, not verified OS/SQLite close success.
No enginefix, founderdiagnostic, timeout/quarantine/settings/counterchange follows.
