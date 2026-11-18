# 0028: Owned one-exchange TLS/TCP Append transport

Status: accepted for bounded implementation, 2026-11-18, after independent
[Sol 6.1/high contract review](../reviews/m3-owned-tls-append-contract-sol61-review.md).
No live transport or acceptance result exists.

ADR 0026 supplied a bounded Append codec. ADR 0027 supplied the specific
fail-closed try-lock applied-registry callback, without complete TLS ownership.
The next step toward the three-container acceptance route is an actual socket
exchange with an explicit application end condition and task/store lifetime.
An async wrapper alone cannot supply those guarantees.

Propose tokio-rustls =0.26.6, defaults disabled, ring only, over the pinned rustls
0.23.46/Tokio 1.48.0 and existing reviewed TLS 1.3 mutual-auth configs. The cached
manifest permits those pins; actual lockfile/feature unification remains unrun.
Use the adapter's pre-handshake buffer hook for a 65536-byte send-buffer limit;
this is not a total TLS memory bound. Preserve existing receive/handshake limits,
no resumption/early data, and real AuthenticatedPeer extraction.

Propose new per-owner transport policies: 32 admitted inbound/outbound exchange
tasks shared across both directions, four active handshakes shared across both
directions, and five seconds absolute inbound accept-to-reply/termination time.
Acquire without queueing. These bound transport resource occupancy and silent
pre-request peers. They do not derive from or change existing proposal/read
units, election settings, engine limits, quarantine, CI budget or fix counters.
Outbound uses its original method-entry plus RPCOption::soft_ttl deadline, never
hard_ttl or per-stage renewal. Five seconds from close entry bounds graceful
transport drain; abort then join is mandatory cleanup and no hard total wall-time
shutdown guarantee follows. Synchronous crypto/scheduling are not preemptible by
Tokio timeouts.

One connection carries one bounded ARXP/v1 request/reply. Client writes/flushes
then shuts down the TLS write direction before reading. Server reads exact
header/body and requires clean TLS application EOF before reauthorization and
dispatch; byte/error/missing close refuses. Server similarly writes/flushes/TLS-
shuts down its reply; client requires clean EOF before accepting its result.
Raw TCP EOF without close_notify is failure. This closes the extra-byte ambiguity
without making the server and client wait on each other. It does not inspect raw
TCP after authenticated TLS close. Source feasibility is established, real
network half-close behavior is not yet tested.

Bind actual certificate cluster/node/key to configured cluster/local target,
header source/target and Append vote leader. Require a committed Append leader
vote before dispatch; the syntax codec's uncommitted value is refused with a real
TLS zero-dispatch test, preserving the pinned engine's committed-leader assumption.
Reauthorize the retained actual
tuple using the reviewed callback immediately before dispatch and before
returning a reply outcome. Validate reversed reply routes and exact sent
acknowledgment/conflict/higher-vote semantics. Local-applied staleness and
post-check revocation races remain; no owner/control/bootstrap or Byzantine
member authority is added. Canonical numeric endpoint validation is a new
transport boundary; the permissive membership address syntax remains unchanged.
A private test-only loopback constructor cannot become live endpoint policy.

The private owner registers tasks and closes admission under one short gate.
Tasks retain sockets, buffers and units despite caller loss; closure stops/joins
the accept loop, drains, aborts if needed, and joins every task. Keep a legitimate
SmAdapter alive through all callbacks, preventing their temporary Weak upgrades
from becoming the final store owner on a TLS worker. Release transport-held
store/Engine/config references only after joins and before the unchanged full
ConsensusNode::shutdown store-release proof. Abort/drop alone is no completion
proof. Cancellation of close() leaves the accept handle, JoinSet, original close
phase/deadline and SmAdapter keeper in persistent owner state. A later close
resumes that state without a new grace budget; no aborted-but-unjoined work is
dropped by cancelling the close future. After joins, close severs its optional
keeper and strong dispatcher/config/engine paths even if closed client/shared
handles remain alive. Focused cancellation/resume and retained-handle tests must
prove both properties. Production Drop/runtime teardown, the start-before-store factory cycle
and a composite node/transport shutdown budget need separately reviewed
integration; this slice adds no production startup hook.

Implement only a private typed NetAppend sender and receiver with a test-only
recording dispatcher. Do not claim RaftNetworkFactory support: its actual Network
bound requires Vote/snapshot/stream/backoff/transfer traits and its V2/NetVote
PreVote default grants synthetically. Full Vote, explicit PreVote and snapshot
protocols must precede a production factory and networked-engine proof. Joining,
custody, self-admission/promotion, trusted expiry and isolated container startup
remain further prerequisites; direct cluster initialization or mem_net cannot
satisfy product acceptance.

[The bounded implementation brief](../briefs/m3-owned-tls-append.md) pins the
allowlist, actual APIs, deadlines, lifecycle limitations, meaningful real-TLS
positive/refusal tests and two once-only semantic controls. No source changes,
builds, tests, controls or acceptance runs were made for this decision. Existing
Sol 3/3 exhausted and historical Opus 0/2 counters and stop rules remain intact.
M2 remains OPEN, M3 UNCHECKED, and real multi-host paths are **unverified on real
hardware**. Independent source review and implementation evidence remain required.
