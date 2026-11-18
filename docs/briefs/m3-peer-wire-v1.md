# M3 peer wire v1: bounded AppendEntries codec prerequisite

Sol 6.1/high design, coordinator disposition, 2026-11-18. Base green main
6bf8f46bc87a05bed01e342c1a37f68427e4c16b. This is the first codec prerequisite
for the three-container plan. No socket, TLS, quorum, joining or acceptance credit.
[ADR0026](../adr/0026-bounded-peer-wire-codec.md) records the protocol choice.

## Scope

Use a closed manual binary encoding, reusing existing enrollment and membership
codecs; do not serialize native OpenRaft objects with arbitrary serde/JSON.
AppendResult has no serde implementation in pinned OpenRaft0.10.0. Unknown versions
or schemas refuse with no negotiation, downgrade or fallback.
Source allowlist: consensus.rs one private module declaration, and new
consensus/peer_wire.rs with conversions and synchronous tests. Freeze dependencies,
existing codecs/storage, TLS, fresh_read, Owners/start/shutdown, ingress, fixtures,
workflows, quarantine, settings, deadlines and permits. No listener/factory yet.

## Exact frame

One complete frame has a fixed44-byte header; all integers big-endian:
magic ARXP(4), version1(u8), kind(u8:1AppendRequest/2AppendReply), reserved flags0(u16),
payload length(u32), cluster16bytes, nonzero source(u64), nonzero target(u64).
Separate header inspection from full decode so a later socket reader checks its
allocation bound before reading a body. Neither function authenticates anything.
Reject unknown magic/version/kind, nonzero reserved, zero routes, truncation,
length disagreement and trailing bytes. Vote/pre-vote/snapshot/leadershiptransfer/
enrollment/forwarding/control/MCP kinds are unsupported and refused.

Vote is term:u64,node:u64,committed:u8 restricted to0/1. A log position is
term:u64,leader:u64,index:u64, respecting existing durable index range. Optional
positions use0absent/1present then position; other option tags refuse.
AppendRequest is vote, optionalprev, optionalleadercommit, count:u8(0..64), entries.
Each entry is length:u16 then position24bytes and canonical payload. Payload tags
are Blank0, Membership3(u16length+canonicalbytes<=4096), or existing command tags
1Mint/4Found/5Remove/6Redeem with their existing canonical fields. OldRedeem2 refuses.
Reuse enrollment::codec::{encode_command,decode_command,Reader}, membership encoding
and parent conversions; do not expose the private durable log-row codec as a wire
API. Require full nested consumption and canonical re-encoding. Codec syntax is
not command authorization or apply-time semantic acceptance.

Max entry body is1+2+4096=4099bytes; existing command maximum is smaller. Max entry
including position is4123; largest fixedrequestprefix is68 with both optionals.
Maximum request payload is68+64*(2+4123)=264068bytes; maximum completeframe264112.
Check native count/member bounds before cloning or growing output; checked length
arithmetic, same encode/decode limits. These are wire bounds, not engine capacities.

Reply is one tag and outcome:0Accepted(optional exact acknowledgedposition),
1Conflict(exact rejectedposition),2HigherVote(vote). Largest replypayload26bytes.
Convert exhaustively to/from native AppendResult; omit only optional conflict hint,
which carries no acknowledgement. Preserve accepted None and partial acknowledgements.
Do not use AppendEntriesResponse conversion helpers: conversion to Success discards
exact acknowledgedposition, and Conflict->AppendResult can unwrap absent prev.
No request is needed to reconstruct reply acknowledgement; do not invent its value.
A later client must additionally validate returned positions against its sent request.
No bytes/missing reply implies success. Static WireError variants only; no payload
Debug or commands/keydigests/verifiers/certificates in errors.

## Authentication boundary for later transport

Header cluster/source/target are routing claims, not authority. Codec constructs
no AuthenticatedPeer, consults no registry and dispatches no engine operation.
Later listener must complete TLS1.3 with reviewed configs/resumptiondisabled,
extract retained cluster/node/key, bind configuredcluster/localtarget/TLSsource,
require request vote leader==TLSsource, and reauthorize tuple on receiver applied
registry for everyRPC before dispatch. This registry view may be stale under
isolation; it is not fresh owner/control authority. Outbound verifies expected
peer target; addresses remain dial hints. Anchor-only bootstrap is separate:
inbound AppendEntries/snapshot under receipt fence only, never Vote/pre-vote/
forwarding/control. Anchors and admission are not implemented by this codec.

## Pure test plan and bounded controls

Synchronous tests, no engine/runtime: independent byte fixtures for heartbeat and
mixed Blank/Found/Mint/Redeem/Remove/Membership; exact field roundtrips;64entries
accept/65refuse; oversized membership/nestedlength/advertisedtotal refusal before
collection allocation; every truncation boundary; invalid fields/flags/options/
tags/trailingbytes; full vote/log/entry changes affect bytes; all exact reply
outcomes including None/partialack/conflict independent of request predecessor;
wrong direction/kind refusal; static errors contain no payload data.
Designated controls after passing baseline and separate bounded assignment: omit
count bound, accept trailing bytes, force accepted position toNone. Each must compile
and fail its intended assertion once, then restore exact source bytes. No controls
run yet. Independent source review and normal gates precede publication/merge.

## Live transport blockers and follow-up

Current deps have synchronous rustls and Tokio net/io, no tokio-rustls. Manual
rustls driving or a pinned async dependency needs its own source scope/ADR. The
synchronous verifier registry callback may block indefinitely on the store mutex;
cancelling an async wrapper does not bound underlying work. Before live sockets,
review handshake admission, blocking ownership, connection/task limits, shutdown
and secret lifetime. Do not silently borrow/increase proposal/read capacity.
new_client must not dial eagerly; later deadlines derive from RPCOption::soft_ttl
without altering engine settings. Keep snapshot traffic separate from heartbeats.
Pinned full snapshot API is full_snapshot(vote,SnapshotOf<Arx,Vec<u8>>,cancel,option),
receiver install_full_snapshot; legacy chunkInstallSnapshot is removed. Current
state image cap16384 and adapter blob-position/metadata checks must remain intact.
Loss/cancellation after possible engine dispatch is unknown, never rollback.

No build/engine/network/container action performed for this proposal. M2OPEN,
M3UNCHECKED, Sol3/3 exhausted/Opus0/2 unchanged. Hardware **unverified on real hardware**.
