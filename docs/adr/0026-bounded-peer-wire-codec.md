# 0026: Bounded peer wire codec before live transport

Status: proposed for independent review, 2026-11-18.

The three-container acceptance route needs real peer transport. Existing mem_net
is an in-process fixture, and the executable has no cluster listener/startup path.
Pinned OpenRaft types and existing durable codecs do not themselves define a
bounded public peer wire format.

Propose a closed manual ARXP/v1 binary frame for AppendEntries request and exact
AppendResult reply, with a44-byte header and264112-byte maximum frame. Reuse
canonical command/membership payloads, retain exact acknowledged positions, reject
unknown versions/tags and defer Vote/snapshot/otherRPCs. This is a pure codec
prerequisite, not an operational partial cluster or production acceptance.

[The brief](../briefs/m3-peer-wire-v1.md) pins fields, bounds, refusal tests and
source allowlist. This creates no storage migration, dependency, capacity, timeout,
quarantine, listener or permission change. No released peerwire compatibility is
promised; future schemas use explicit version/refusal rather than silent fallback.

Live TLS needs a separate ownership design because the synchronous registry
verifier can block on the store mutex. Async framing does not solve that problem.
New handshake/connection capacities and shutdown need a reviewed ADR before live
implementation. Bootstrap anchors, custody, admission, trusted-clock expiry and
owner-control authority retain their separate contracts.

Implementation follows independent technical review. Real network/container paths
and real multi-host hardware remain unverified. Existing exhausted fix counters
and no-further-founder-diagnostic rule remain unchanged.
