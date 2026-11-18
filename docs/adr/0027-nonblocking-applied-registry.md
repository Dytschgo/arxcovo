# 0027: Fail-closed nonblocking applied-registry callback

Status: proposed for independent review,2026-11-18.

The current TLS verifier accepts a synchronous Registry callback. A callback using
StateStore's blocking mutex can wait withoutbound; an asyncTLS wrapper cannot
cancel that synchronous wait. The first live-transport prerequisite should remove
that wait from the specific callback selected for transport, without rewriting
arbitrary callbacks or claiming complete TLS ownership.

Propose a private factory capturing Weak<Mutex<StateStore>>. Each call upgrades,
try_locks, checks actual localapplied authorize_peer, and releases temporaryowners.
Storegone, contention and poison failclosed with existingUnavailable. No cached
snapshot, task, queue, capacity, dependency or timeout is introduced. The deliberate
tradeoff is transient handshake refusal under legitimate storecontention. Callers
may make a later explicitattempt; this callback never retries internally.

This preserves localapplied staleness and later revocation races; futureRPCs still
need reauthorization. It grants no owner/admission/bootstrap authority. Retained
TLSconfigs must not prolong storelife through the callback. Existingverifier and
configconstructors remain unchanged. Only this callback has non-waiting mutex
semantics; no hardwallCPU/scheduling or arbitrarycallback guarantee is claimed.

[Bounded contract](../briefs/m3-nonblocking-peer-registry.md) pins factory/scope,
realstore tests and futuretwo meaningfulcontrols. Accept after independentreview;
liveTLSdriver/connectionownership/inboundpolicy remain a separate ADR and proof.
No enginefix, founderdiagnostic, timeout/quarantine/settings/counterchange follows.
