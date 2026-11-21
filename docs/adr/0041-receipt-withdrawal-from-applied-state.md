# ADR0041: Withdraw learner receipt authority from applied state

Date: 2026-11-20. M3 Step 9.

The join publication retains the original Redeem position, voter anchors,
leaf and sealed member credentials. The earlier learner admission brief
proposed rewriting the receipt to remove its anchors after catch-up. Step 9
keeps the publication intact and derives withdrawal from the learner's
durable applied position, canonical membership and exact local binding.

Only the published learner's incoming transport can provisionally accept
an original cluster, node and key tuple absent from the applied registry.
It checks authority after TLS and before reading any Arxcovo frame, then
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
reading any Arxcovo frame, the transport checks learner authority.
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

## Strict transport extension, 2026-11-21

C11's first outbound AppendEntries reached the learner listener, then the
issuer's TLS client refused when its own strict applied registry was
unavailable. The earlier learner-only change could not cover this site.

For ordinary owned peer RPCs, both sides complete cryptographic TLS before
checking the local strict applied binding asynchronously. The client
checks before its first frame write and the server before its first frame
read. During the handshake, Rustls can decrypt and buffer a bounded amount
of application data before the applied binding check (its 16 KiB plaintext
backpressure threshold, with record-deframer batch caps); Arxcovo neither
parses nor acts on those bytes, and refusal drops that data with the
connection. Strict
server dispatch, including admission control, and the client's final reply
check take a fresh applied observation with the same 4 ms acquisition
budget. Deadline and cancellation checks still cover the exchange.
Internal store poison, mutex poison and unavailable or dropped stores
refuse immediately when detected. TLS still uses its existing time provider.
The learner receipt path still samples the checked clock during authorization.

The learner's outbound admission-anchor verifier and raw test TLS probes
keep their immediate authorization checks. This preserves the explicit
admission route and the semantics of callers without an owned RPC boundary.

## Amendment A22, 2026-11-21

In main CI run 44940294254, C11 completed its outbound TLS handshake at
4.234 ms and refused outbound applied pre-frame authorization as unavailable
at 8.323 ms. The 4.089 ms interval exhausted the 4 ms store wait with about
66.6 ms left on the 75 ms soft TTL. The inbound pre-frame check had passed
at 6.423 ms.

Each applied-store acquisition wait now has a 20 ms cap, shortened to the
remaining local RPC deadline where one applies. The cap applies from the
first attempt, including founder node admission and the guarded availability
and offer rechecks during sealing. Once admitted, a fresh read uses its
separate 1.5 s budget. The 250 µs poll interval is unchanged.
From a 75 ms budget, two sequential
checks can consume up to 40 ms of acquisition waiting, leaving at least
35 ms for the rest of the exchange. Three can consume up to 60 ms, leaving
at least 15 ms. Admission and transfer retain their 5 s budgets.

The outer budgets remain 75 ms for AppendEntries, 1.5 s for a fresh read,
5 s for admission, inbound, transfer and Found, and 16 s for the owner.
All other D6 boundaries remain unchanged.
