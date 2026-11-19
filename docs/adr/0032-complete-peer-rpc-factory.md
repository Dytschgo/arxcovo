# ADR0032: Complete peer RPC and private network factory

Status: design accepted; exact-source GO, four local learner observations PASS
and two semantic controls qualifying; final PR review and hosted acceptance pending.
Date: 2026-11-19.

## Context

ADR0028 supplies authenticated Append transport. OpenRaft also needs real Vote,
explicit PreVote and full snapshot transfer before it can use this transport as
its network factory. The SDK's synthetic default PreVote response cannot provide
that evidence. Production startup and joining remain separate work.

## Decision

Keep v1 byte encoding closed and add a closed ARXP/v2 protocol for Append, Vote,
PreVote and full snapshots. The private listener accepts v2; it does not negotiate
or silently fall back to v1. Preserve full native fields and authoritative replies.
Validate routes, method directions and bounded lengths before body allocation;
bind snapshot bytes, cluster, position and membership before Engine dispatch.

Use the existing transport owner for all methods, retaining its shared32 exchange
and4 handshake units, deadlines, actual TLS identities and final applied-registry
checks. Supply lazy weak factory/client handles and an attach-once private
composition. Transport admission closes and tasks join before unchanged node
shutdown. Observed Engine failure remains an unavailable state; retained handles
cannot keep stores alive after successful close. Optional leader transfer refuses
without dialing. No production main listener is introduced by this slice.

The reviewed fixture uses four separate passive learner histories through the
real receiver Engine and genuine TCP/TLS, with an authenticated Engine-free sender.
Fixture socket reservations move by value into the same transport accept loop.
Incomplete cleanup retains supervision and cannot be reported as success.

Independent review accepted one narrow evidence exception: a valid canonical
snapshot cannot reach16384bytes under current storage bounds. Its conservative
upper bound is4866+55*209=16361bytes; attainability of that upper bound is not
claimed. Keep the16384byte transport cap and all native/wire/header oversize
refusals. Do not pad invalid images or change storage limits to make the fixture.

## Consequences

This provides a private required-RPC boundary, not production joining or three-node
acceptance. Startup, custody, admission, clock wiring and remote execution still
need implementation before real network container acceptance. v2 is deliberately
incompatible with the old private listener; no deployed schema migration occurs.
The change is reversible source integration.

Exact-source review precedes the four nominal Engine observations; separate
semantic controls and normal required cross-platform gates remain mandatory.
Engine Sol3/3 and historical Opus0/2 remain unchanged. No runtime repair,
diagnostic, timeout, capacity or quarantine change follows from this decision.
M2 OPEN / M3 UNCHECKED; hardware remains **unverified on real hardware**.

See [candidate evidence](../evidence/m3-complete-peer-rpc.md) and the
[three-container acceptance plan](../briefs/m3-three-container-acceptance.md).
