# M3 owned TLS/TCP Append transport

Status: accepted bounded implementation contract, 2026-11-18, after independent
[Sol 6.1/high review](../reviews/m3-owned-tls-append-contract-sol61-review.md).
Coordinator authorized the specified source slice. This document records source
inspection, not a build, dependency-resolution result, network test or acceptance
pass. [ADR 0028](../adr/0028-owned-tls-append-transport.md) proposes the new policies.

The next source slice carries the existing AppendEntries frame over actual TCP
and mutual TLS, with explicit task ownership and refusal tests. It is a transport
prerequisite for the [three-container route](m3-three-container-acceptance.md).
It cannot complete M3, joining, a usable OpenRaft network factory, or owner testing
of the full service. ADR 0023 permits this independent M3 work while M2 stays open.
Sol 3/3 exhausted and historical Opus 0/2 remain unchanged: no founder fix,
diagnostic, rerun, timeout increase or quarantine change follows.

## Bounded source slice

Allow only:

- `crates/arxd/Cargo.toml` and `Cargo.lock`: add only
  `tokio-rustls = { version = "=0.26.6", default-features = false, features = ["ring"] }`
  and its necessary lockfile resolution; no unrelated upgrades.
- `crates/arxd/src/cluster/consensus.rs`: one private module declaration only.
- New `crates/arxd/src/cluster/consensus/peer_transport.rs` and its private
  `peer_transport/tests.rs` test module: typed Append sender/receiver, transport
  owner, address validation, and focused real-socket tests.

Keep peer_wire, peer_registry, PKI/verifier/config builders, enrollment, authority,
issuance, adapters, engine parameters/Owners/start/shutdown, storage and service
startup/configuration unchanged. No workflows, feature gates, ci-time-budget,
quarantine, test exclusion or capacities outside this transport change. Evidence
and PROGRESS updates belong to the coordinator's delivery record, not this source
allowlist. A necessary scope expansion returns to independent review first.

Implement a private typed `AppendClient` using the actual `NetAppend<Arx>` method:
`append_entries(&mut self, AppendEntriesRequest<Arx>, RPCOption)` returns
`Result<AppendResult<Arx>, RPCError<Arx>>`. Its construction stores bounded target
metadata and does not dial or allocate an RPC task. Implement a private receiver
with a typed asynchronous Append dispatcher returning either the exact
`AppendResult<Arx>` or a static unavailable refusal. Tests supply a recording
no-engine dispatcher; no success default, registry bypass or mem_net forwarding.
Do not add an unused or misleading `RaftNetworkFactory` constructor/implementation.

OpenRaft 0.10.0's factory Network bound includes NetVote, NetSnapshot,
NetStreamAppend, NetTransferLeader and NetBackoff. `ConsensusNode::start` also
requires `SnapshotData = Vec<u8>`. NetAppend alone does not satisfy it. In
particular, both NetVote and RaftNetworkV2 default PreVote to a synthetic grant.
This slice implements neither trait and cannot silently inherit that grant.
Unsupported inbound method/version/kind closes without dispatch or a success reply.

## Actual dependency and PKI boundary

The cached 0.26.6 manifest requires rustls ^0.23.27 with std/defaults disabled and
Tokio 1. Current exact pins are rustls 0.23.46 and Tokio 1.48.0. Compatibility is
source/manifest inference only. Later implementation must verify the lockfile and
feature tree: one intended rustls/Tokio line, ring/std, no newly enabled aws-lc,
TLS 1.2, logging, early-data or compression defaults. No dependency installation
or lockfile change was performed for this contract.

Build configs internally with `PeerVerifier::new(root, cluster,
peer_registry::registry(&sm))`, `.server()` inbound and `.client(expected_node)`
outbound, then the existing `verify::server_config`/`client_config`. Use the
existing supplied TimeProvider and `SERVER_NAME`, without a clock-policy change.
Do not accept arbitrary caller-provided verifier/config/Registry in the live path.
The reviewed configs require client certificates and TLS 1.3, disable resumption
and tickets, and send no SNI. Do not enable early data. Test credential generation
is synthetic and test-only; production CA/leaf/key custody remains outside scope.

Use `TlsAcceptor::accept_with` / `TlsConnector::connect_with` to call
`set_buffer_limit(Some(65536))` before handshake I/O. This limits rustls send
buffers, not total connection memory. Receive records, handshake deframing and
crypto use separate pinned-library limits; do not advertise a 64 KiB total cap.
No read_to_end, unbounded buffering, blocking socket I/O, spawn_blocking handshake,
DNS lookup, task per read or user callback inside the admission mutex.

After the handshake, use the typed stream's `get_ref().1` and
`AuthenticatedPeer::from_server` / `from_client`; extract
`authenticated_identity()` before erasing its rustls connection type. `assume`
provides no identity and is forbidden as positive transport evidence. Retain the
actual `(cluster, node, key)` tuple for the exchange, not just the node number.
Do not duplicate the PKI parser or expose its private `peer_identity` helper.

## Framing, half-close and authentication

Exactly one request and one reply per TLS/TCP connection. No pooling, pipelining,
reconnect, prefix splitting, retry or protocol fallback in the driver.

Read exactly the existing 44-byte header into a fixed array, call
`peer_wire::inspect_header`, verify expected direction/kind and fixed route claims,
then allocate only the checked body size and read it exactly. The request body is
at most 264068 bytes; the whole frame at most 264112 bytes. Reply body is at most
26 bytes, whole reply 70 bytes. Header/body addition must be checked; never
allocate from an uninspected claimed length. Use existing encode/decode functions,
not serde or a second frame schema. Fragmentation is ordinary read_exact work.

The application terminator is an authenticated TLS sending-direction close:

1. Client writes the one bounded request, explicitly flushes it, then calls
   `AsyncWriteExt::shutdown` on the **TLS stream** before reading the reply.
   This sends close_notify and half-closes TCP WRITE while preserving reads.
2. Server reads the bounded complete request and then attempts one more TLS
   application-byte read. Only clean zero-byte EOF permits dispatch. One byte
   means extra application payload and refuses before dispatch. Error, missing
   terminator or deadline refuses. No fresh timer starts for this probe.
3. With a clean terminator, validate identity and request semantics, reauthorize,
   then call the dispatcher once. Encode the exact result, write, flush and
   TLS-shutdown the reply direction, using the same inbound deadline.
4. Client reads and decodes the bounded reply, then requires the same clean TLS
   EOF before accepting any AppendResult. Extra data, absent terminator, protocol
   error or deadline returns an RPC failure, even after a complete success frame.

Do not call shutdown on the raw TcpStream as the normal terminator, fully drop
both directions before reading, or wait for peer TCP EOF after a valid TLS close.
Clean TLS close mid-header/body is still a truncated frame. TCP EOF without
close_notify is UnexpectedEof and never success. Reject extra TLS application
bytes before the authenticated close; do not claim to inspect raw TCP bytes
sent after TLS close. This protocol resolves the otherwise circular server-extra-
byte/client-waits-for-reply deadlock. It is source-feasible but remains unrun.

Require request.vote.committed == true before dispatch (and before outbound
encoding). The syntax codec deliberately preserves both boolean values, but
AppendEntries needs a committed leader vote: pinned engine vote update can
accept/persist a candidate vote, while following_handler assumes a committed
leader. Refuse an authenticated member's uncommitted Append request without
dispatch; do not edit the codec or engine to mask this transport-shape fence.

Before dispatch, reject nonconsecutive entry indices, checked-add overflow, or
full LogIds that fail strict ordering from prev through the entries. The first
entry index is prev.index+1, or **0 when prev is None**, matching pinned
OpenRaft 0.10.0 following_handler/Option next_index semantics. Do not invent an
index-1 initial position. An empty heartbeat does not need a synthetic entry.
Preserve full term/leader/index identities; old-leader entries are legitimate,
so do not require every entry leader to equal the current vote leader. These
are request-shape fences; command authorization stays with the state machine.

Inbound must bind all of: configured cluster, configured local node as target,
authenticated peer cluster and source node, header cluster/source/target, and
`request.vote.leader_id.node_id == authenticated source`. Header syntax alone is
not identity. Reinvoke the actual try-lock applied-registry callback on the
retained tuple immediately before dispatch, after full frame and terminator
validation, with no intervening await. Wrong cluster, nonmember, wrong key,
contention, mutex poison or vanished store refuses without dispatch. An actual
Remove after handshake must be observed at this check. This is local-applied
authorization: isolation staleness and a Remove racing after the check remain;
it is not atomic dispatch/revocation or Byzantine-member protection.

Outbound encodes only the configured local source and expected target; vote
leader must equal local source. After TLS, require the server's actual tuple to
match configured cluster/expected target. Require reply route to reverse the sent
route exactly, then reauthorize that actual server tuple before returning success
or any semantic AppendError. Sender substitution, wrong target, cluster or key
never earns a reply. A local certificate/configured-node mismatch must fail closed
at the receiving peer's same binding checks; this slice claims no startup-ready
local credential audit or new certificate parser.

Pin reply semantics to the request actually sent. As this driver sends the whole
request without prefix splitting, an Ok acknowledgment must equal the last sent
entry's full LogId, or prev_log_id when there are no entries (including None for
an empty initial heartbeat). Do not turn None into a position or accept an
unsent/different-term/different-leader position. Conflict must carry exactly the
sent non-None prev_log_id and decode with hint None; Conflict for prev=None is
invalid. HigherVote must be strictly higher than the sent vote using the pinned
Vote comparison (partial_cmp == Some(Greater)), not term-only arithmetic. Never
substitute canned Success, the requested commit position, or a local maximum.
The receiver also validates a dispatcher's result before encoding a reply, so a
buggy seam cannot emit a syntactically valid unrelated acknowledgment.

Map busy/closed/not-ready/address/TLS/identity/framing/deadline/dispatch failure
to `RPCError::Unreachable(Unreachable::new(&static_error))` in this slice.
AppendError remains a nested semantic result, never an RPC transport success
fabrication. Errors/logs use static labels and bounded counts, without raw
addresses, frame bodies, credentials, keys or certificate dumps. Loss/cancellation
following possible dispatch is an unknown outcome, not rollback or proof of no
write. The driver never retries it; later OpenRaft retry policy stays unchanged.

## New transport admission and time policies

These are proposed new limits, not existing engine limits:

| Policy | Exact meaning and reason |
| --- | --- |
| 32 admitted connection/RPC tasks per transport owner | One shared inbound/outbound pool, including connecting, handshaking, framing, waiting for dispatch and terminating. Bounds sockets/tasks/buffers; 16-member durable membership does not justify an unbounded connection fanout. No guaranteed reserved heartbeat share in this first slice. |
| 4 active handshakes per owner | Shared inbound/outbound pool. Bounds concurrent crypto/handshake state separately from established exchanges; acquired without waiting before initiating handshake or outbound dial. |
| 5 seconds absolute inbound budget | Starts when accept returns a socket, before admission/handshake; covers handshake, header, body, terminator, dispatch, reply, flush and TLS shutdown. Prevents silent/slow pre-request peers occupying capacity indefinitely. Inbound has no RPCOption to supply this policy. |

Acquire capacity with try_acquire_owned, never a semaphore wait/queue. When full,
drop the newly accepted inbound socket immediately or refuse outbound before dial.
The OS listen backlog is not an admitted application queue or part of a claim of
32 total kernel sockets. Use one accept loop, at most one just-accepted socket
outside the admitted set. Acquire a connection and handshake unit atomically
with task registration under the short ownership gate; release partial acquisition
on refusal. Handshake units last until handshake completion/failure; connection
units stay with the owned task through all subsequent work and destruction.

Acquire the connection/task admission before encoding/copying a bounded frame;
move the caller-owned request into the task rather than leaving frame allocations
outside the pool. Bound/validate target metadata before copying it into clients.
Caller-supplied request memory and OS TCP buffers are not this pool's allocation.

Outbound deadline is method entry plus `RPCOption::soft_ttl()`, using checked
Instant addition. Zero/overflow/expired budget refuses before dial. Include local
validation/encoding/admission/connect/handshake/request/termination/reply in that
same budget, with no per-stage reset. Do not use hard_ttl, change its ratio,
borrow read/proposal units, or reinterpret the inbound five seconds as an engine
election/RPC setting. Every phase checks the original deadline before dispatch
and before accepting a result. Tokio timeouts cannot preempt synchronous bounded
crypto/parsing or scheduler suspension; no hard real-time wall-clock guarantee
follows. The registry avoids mutex waits but proves no SQL-store health.

Use canonical numeric SocketAddr strings (<=200 bytes, exact parse/to_string
roundtrip, nonzero port), no hostnames/URLs/DNS. Refuse unspecified, multicast,
loopback, IPv4 broadcast, IPv4-mapped IPv6, and scoped IPv6 endpoints in the live
constructor; accept ordinary LAN/Tailscale unicast addresses. Membership's current
syntax checker alone does not enforce this: this is a new transport boundary,
not a membership codec migration. A private cfg(test)-only loopback constructor
may admit only explicit numeric loopback endpoints for local socket tests;
production cannot select it or a generic permissive dialer. These local tests do
not prove LAN/Tailscale reachability or container network isolation.

## Ownership, startup and shutdown fencing

Use one private short mutex for the closed state, completed-task reaping and
JoinSet registration. The closed check and registration happen in the same
critical section, with no await, I/O, registry callback or task join inside it.
Reap completed records before admission so JoinSet bookkeeping cannot grow
without bound even when callers disappear. A closure racing admission either
contains the registered task in its set or refuses it; no detached spawn path.
The task, not the caller future/oneshot receiver, owns permits, socket/TLS state,
buffers, authenticated tuple and dispatcher future until its real completion.
Dropping a caller does not release its connection unit early. Observe caller
closure before dispatch when it is reached; already dispatched work has an
unknown outcome. No new task/connection may be admitted after close. Check observed closure and
original deadline again at the pre-dispatch gate; work already past that gate
can have an unknown outcome when close races it.

ADR 0027's Registry captures Weak<Mutex<StateStore>>, but a temporary upgrade can
become the last store owner. The transport owner therefore retains a legitimate
SmAdapter keeper while any TLS callback/dispatch task can run. It must never
release that keeper on caller cancellation, close admission, timeout, or merely
issuing abort_all. Configs/Registry clones alone must continue not to retain the
store. There are no store-lock waits or persistence calls on TLS runtime workers;
the keeper prevents the callback from doing final store destruction there.

Close is explicit and observed: mark admission closed, stop and join the accept
loop, drain registered tasks against one transport close deadline of five seconds
from close entry, then abort unfinished async tasks and **join every one** before
claiming closed or releasing keeper/configs/dispatcher references. This is a new
transport drain policy. If deadline, panic or join failure occurs, report it as an
unclean transport close even if subsequent joins finish. Abort/drop is not a
join acknowledgment. Aborted tasks remain owned/observed until joined. Do not
report a successful bounded shutdown while unjoined work exists. No timeout can
preempt a synchronous poll; the post-abort joins are required cleanup, not a
promise that total close is <=five seconds. An unjoined owner must retain the
keeper and prevent a clean-release claim.

Keep the accept-loop handle, JoinSet, close phase/original deadline and optional
SmAdapter keeper in persistent owner state, not moved into the close() future.
Cancelling that future must leave all unjoined work and the keeper owned there;
it neither renews admission nor drops/forgets an aborted-but-unjoined task. A
later close() resumes the same phase and original deadline, with no renewed grace
budget. If the original deadline elapsed, resume abort-and-join cleanup and
retain the unclean outcome. Cancellation is not a close acknowledgment.

After all transport work is joined, take/drop the owner's optional keeper and
sever stored dispatcher/config/engine references as part of close completion.
Retained AppendClient, shared-owner, dispatcher or configuration handles must
not retain an SmAdapter through another strong path after successful close.
Registry callbacks remain weak; a closed shared handle is inert and store-free.

The slice is private and instantiated only by its focused tests; no production
startup hook is added. Each fixture keeps a separate legitimate store owner
until explicit transport close/join finishes, including panic/error cleanup.
Transport Drop/runtime teardown is emergency abort only, never graceful-shutdown
or store-release evidence. Full production Drop/runtime-teardown ordering remains
a reviewed integration prerequisite; do not treat the test fixture's lifetime as
proof that arbitrary service teardown is safe.

Later full-RPC integration must solve the actual start cycle: start needs the
factory before SmAdapter/Engine exist. A future factory must be lazy/NotReady,
then attach the actual registry, TLS material, legitimate store keeper and Engine
once after successful start; refuse before attach and after close. It cannot call
an eager dial or invent a startup-ready registry snapshot. This contract adds no
attach API or edits to ConsensusNode to disguise the missing integration.

That future coordinator must keep ConsensusNode alive while closing and joining
transport, release every transport-held Engine/SmAdapter reference, then call the
unchanged `ConsensusNode::shutdown`. Awaiting its entire shutdown while retaining
an extra transport SmAdapter makes its Released/store-release check fail. Do not
weaken that proof, change the existing five-second node shutdown or claim a
composite five-second shutdown; a phased shared-budget API would require its own
review. Released observes preceding destructor completion, not verified OS/SQLite
close success. Store write-poison and node health/readiness remain separate.

## Meaningful real TLS/TCP tests required before credit

All cases use actual Tokio loopback TCP, actual TLS 1.3 configs/verifier and
synthetic disposable CA/leaf keys plus actual disposable StateStore applied
Found/Remove where authorization is tested. Generate fixtures without the real
engine/founder workflow; that is no founder diagnostic or acceptance substitute.
The existing PKI leaf/test_ticket helper fixes cluster [1;16]/node 4 and is not
a general two-node issuer. Reuse its TimeProvider/node URI/config conventions,
but issue distinct synthetic client/server leaf identities with a test-local
rcgen CA/Issuer in the new test module where needed. Use real certificate-chain,
SPKI and URI verification against actual StateStore bindings. Do not unseal
LeafSubject/IssueTicket or extend production issuance merely to create fixtures.

No pump-only handshake, in-memory duplex, AuthenticatedPeer::assume, direct
initialization of a cluster or mem_net earns transport acceptance. A recording
dispatcher returns explicit AppendResults and observes exact calls, not a fake
network success. Assert dispatch count zero on every pre-dispatch refusal.

Use channels/barriers and bounded event observations rather than sleeps or
retry-to-green. Test gates are private cfg(test) points in real socket phases,
not replacements for socket/TLS work. Always release gates, close peers and join
all helpers before potentially panicking assertions. Small test-only observation
budgets detect hangs; they do not rewrite the production policy. Every test
closes its transport and proves all permits/tasks recovered before dropping the
fixture keeper. Record expected deadline/refusal separately from cleanup failure.

| Cases | Required observable result |
| --- | --- |
| Empty heartbeat and entries/membership payload; fragmented header/body; maximum legal frame larger than the 64 KiB send buffer | Real TLS half-close exchange completes without deadlock, exact request dispatched once, exact None/last LogId returned, all tasks/units reclaimed. |
| Conflict and HigherVote | Exact non-None prev conflict with hint None and genuinely higher vote survive a real exchange; neither becomes Ok. |
| Valid request with client TLS half-close while retaining read side | Server sees clean EOF, sends reply, client reads it and validates clean reply EOF. This positive proves the specific adapter's half-close behavior. |
| Wrong root/cluster, missing client cert, expired cert, same node/different SPKI key, wrong expected server node | Real handshake/identity refusal, zero request dispatch. Use validly signed distinct fixtures for key/cluster cases. |
| Valid certificate/authenticated member, matching route and leader, but vote.committed=false | Zero dispatch over real TLS; positive Append fixtures use committed=true. The refusal must be the request-shape fence, not handshake failure. |
| Nonconsecutive indices, prev=None with first index 1, overflow, nonincreasing full LogIds | No dispatch; include valid initial index 0 and old-leader entries as positive controls. |
| Valid TLS peer but substituted header source, wrong configured local target, wrong header cluster or request vote leader | Receiver refuses before dispatch; do not rely on handshake failing for these application-binding tests. |
| Actual Remove after completed handshake, before final dispatch gate; held/poisoned actual store mutex at reauthorization | Removed/Unavailable refusal before dispatch using the retained actual tuple. Hold/release mutex with cleanup that cannot deadlock on an assertion. No retry inside driver. |
| Request header-only oversize, unsupported method/version/wrong direction; malformed body; header/body truncation with clean TLS close | No body allocation from the oversize length; no dispatch, no success. At least one request independently encoded by the test, not the production encoder. |
| Extra request byte before close_notify; complete request with no terminator; raw TCP write-close without close_notify; clean TLS close in header/body | Extra refuses immediately; missing terminator expires at original deadline without dispatch; raw EOF is UnexpectedEof; truncated clean close refuses. Test each distinct condition. |
| Extra reply byte, missing reply terminator, raw reply TCP EOF, truncated reply | Sender returns RPC failure even after receiving a valid success frame; dispatch may already have happened and must not be labelled rollback. |
| Wrong reply route/source/cluster, unsent or wrong-identity acknowledgment, conflict with wrong/absent prev, equal/lower HigherVote | Sender refuses each semantic violation; include same index with different term/leader. Never credit None as a sent nonempty acknowledgment. |
| Slow handshake then slow body/terminator; short outbound soft_ttl | One original deadline spans the phases; completing an earlier phase does not renew the budget. A silent inbound peer times out even before any RPC metadata exists. |
| Four real handshakes deliberately held; independent fifth inbound/outbound attempt | Shared handshake cap refuses without queued fifth handshake; established post-handshake tasks release handshake units but retain connection units. |
| 32 admitted tasks across inbound and outbound, already past handshake where necessary; 33rd attempt | Shared connection cap refuses without new dial/task queue, and completion allows a later explicit call. Account for test remote servers separately from this owner's pool. |
| Drop outbound caller while its real handshake or reply-terminator wait is gated | Permits/socket remain charged to the owned task until actual completion/deadline/observed abort-and-join; no premature release or detached owner. Cancellation noticed before dispatch does not dispatch. |
| Close/admission race, close during handshake/request/reply/dispatcher gates, task panic, deadline then abort | No registered task escapes the closed set; accept loop and each task are joined, panic/deadline is reported unclean, no new dispatch after observed close gate. Abort issuing alone cannot satisfy the test. |
| Cancel close() while handshake/dispatcher or abort-and-join is gated, then resume close | Persistent owner retains accept handle, JoinSet and keeper; no admission or grace-deadline reset. After releasing gates, resumed close joins all work and preserves an expired/unclean outcome. Cancellation alone never earns Released. |
| Keeper lifetime with client/shared-owner/dispatcher/config clones retained; explicit close/drop ordering | Weak callback cannot become final owner while transport work exists; after successful close severs its optional keeper and strong dispatcher/config references, fixture store Released occurs once other legitimate owners drop, even while the closed client/shared/config handles remain alive. No engine is started and no OS-close success is inferred. |
| Numeric address accepts/refusals; invalid/NotReady/closed client | Zero dial/dispatch on denial; test-only loopback exemption unavailable to live constructor. Lazy client construction consumes no task/socket/permits. |

After a positive baseline and independent source review, run only these two
semantic controls, once each on exact restored source:

1. Remove the final applied-registry reauthorization, retaining real handshake
   verification. The Remove-after-handshake socket test must fail because its
   spy observes an unauthorized dispatch/result; a compile error or unrelated red
   is not the intended kill.
2. Replace the receiver's required clean-EOF terminator check with immediate
   dispatch after its valid frame. The real extra-request-byte test must fail
   because dispatch occurred; this proves the application boundary, not merely
   rustls cryptography.

Restore exact bytes/hash after each, retain bounded logs, and never repeat a
surviving/unrelated-red control or convert it into engine diagnostics. These are
new transport test controls, not another founder fix attempt. Other controls
require a concrete additional claim and independent authorization.

## Credit and remaining hard prerequisites

Implementation validation is D-only locked/offline fmt, Clippy and the focused
new tests, with effective scratch paths verified first under docs/dev-scratch.md.
No command may silently reuse C: TMP, target or tool caches. Normal independent
Sol source review and required green exact-head checks remain; no first-failure
rerun or CI budget waiver. Real socket cases must run on Windows, Linux and macOS
through ordinary required checks before cross-platform claims. Preserve existing
engine tests/quarantine and exhausted counters unchanged.

This contract has run **none** of those checks/tests/controls. It only inspected
repository source and existing D: dependency sources. The independent reviewer
must settle this contract/ADR before implementation. Build/lockfile unification,
real adapter half-close behavior and the bounded owner proofs remain unverified.

Full Vote, explicit PreVote and snapshot vote/meta/blob/cancel/soft_ttl transport
are later reviewed slices. Preserve the existing <=16384-byte snapshot blob and
metadata/membership matching; use an explicitly reviewed complete RPC version,
not extra live ARXP/v1 tags without review. Factory heartbeat/snapshot separation,
production attach/Drop/teardown, private startup/config/path fences, key custody,
legitimate first-node bootstrap, actual join/self-admission/promotion, trusted
clock/expiry qualification and isolated-container network fences remain hard
prerequisites. Synthetic fixture credentials and dispatcher results cannot close
them. Only then can real networked engines, and subsequently real joined
three-container node-loss/write/expired-reuse acceptance, earn their own evidence.
Real multi-host paths remain **unverified on real hardware**; M2 OPEN/M3 UNCHECKED.
