# ADR0041: Withdraw learner receipt authority from applied state

Date: 2026-11-20. M3 Step 9.

The join publication retains the original Redeem position, voter anchors,
leaf and sealed member credentials. The earlier learner admission brief
proposed rewriting the receipt to remove its anchors after catch-up. Step 9
keeps the publication intact and derives withdrawal from the learner's
durable applied position, canonical membership and exact local binding.

Only the published learner's incoming transport can provisionally accept
an original cluster, node and key tuple absent from the applied registry.
It checks authority after TLS and before reading application bytes, then
repeats the checked observation before dispatch. The final check grants
this exception to AppendEntries and full snapshots.
A conflicting applied key, unavailable store, store poison or failed clock
sample refuses. The outgoing verifier uses applied bindings.

At the original index, the full term, leader and index must match. A later
validated applied image can cross the boundary with canonical membership
and the exact local binding. A conflicting crossing or loss of the local
binding permanently refuses inbound traffic for that live job. A later
inconclusive observation cannot restore receipt authority.

A same-process reopen consumes a private expectation from a verified
publication after proof of strict eligibility and joined owned close.
Reacquisition checks the original installation objects and validates
read-only history before writable acquisition. The exact operational pair
undergoes the same history checks and preflight. Recovery rejects an
incompatible surviving row at the original index and installs a strict
transport under the recovered checked-clock floor.

Step 13 owns authenticated history for process restart. Same-account disk
rollback and power loss are outside Step 9's guarantees. Service and
multi-host paths are **unverified on real hardware**.

## Inbound applied-store contention, 2026-11-21

Rustls verifies the learner's incoming certificate chain, cluster identity
and proof of key possession. Immediately after the handshake and before
reading any application bytes, the transport checks learner authority.
Refusal closes the connection before framing. Final dispatch repeats the
authorization using the same policy and a new store observation.

Each boundary allows up to 4 ms to acquire a busy applied-store mutex.
The task awaits short Tokio timer intervals between try-locks, so other
tasks on the same executor can run. Acquisition after the budget expires
refuses. The two budgets total up to 8 ms within the existing 75 ms
AppendEntries caller deadline, which the transport continues to enforce.

The store guard still covers the internal poison check, checked-clock
sample, phase evaluation and binding authorization. Mutex poison or a dropped
store refuses directly. An unhealthy store or clock refuses as soon as the
guarded observation detects it. The phase lock is still a try-lock. Admission
reply authorization and the applied registry callback in ADR0027 keep their
immediate try-lock behavior. The acquisition wait holds neither lock.
