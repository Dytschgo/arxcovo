# 0008: Bounded opt-in local inventory MCP transport

Status: implemented candidate, pending independent review/integration.

The foreground inventory daemon accepts explicitly configured local IPC alongside
its existing stateless HTTP endpoint. The stdio process is a byte-frame bridge;
MCP protocol handling stays in official rmcp 3.9.0 on the daemon. It has no state,
execution, service-management or endpoint-spawn authority.

The client requires private bounded configuration with an explicit expected OS
server identity. Before credential writes, the connected pipe object's owner SID
(Windows) or socket peer UID (Unix) must match. Windows queries the pipe descriptor,
not a server process token: a standard client cannot assume TOKEN_QUERY access to
a future SYSTEM process. First-instance reservation, NETWORK denial, remote-pipe
rejection and specific client rights protect the pipe namespace. The configured
client never receives the pipe-instance creation bit through its client grant.

A fixed, timed auth preamble maps the credential to one immutable connection
principal; HTTP continues authenticating each request. SDK frames, connections,
handshake, partial-frame reads, writes, frame sizes and connection count are bounded. Inventory/activity
semantics and persistent host identity are shared without a second storage path.

This is foreground same-user development trust, not binary attestation or service
isolation. Same-user credential access is inherent. SYSTEM/root identity values
are supported but privileged paths have not been exercised. Unix separate-user
filesystem provisioning belongs to service integration; current socket/parent
permissions are owner-only. Existing endpoint objects are never reclaimed.

See crates/arxd/README.md for exact bounds, configuration and references. Services,
dedicated-account execution, Claude Code acceptance and their production security
review remain outstanding. There is no change to the settled execution design.

## PR #15 review correction decision (2026-11-11)

Keep strict current TokenUser ownership for Windows private client configuration
and its parent. Do not accept arbitrary group owners. The daemon already forces
its TokenUser as pipe owner; fixtures now obtain TokenUser directly and explicitly
assign it to file and parent, even when TokenOwner is BuiltinAdmins. Expected
server identity is independent of filesystem ownership. Standard/filtered tokens,
full/elevated administrator tokens, and SYSTEM can all designate their TokenUser
as owner of objects they provision. Only the filtered Windows runtime has been
exercised locally; full/elevated and SYSTEM runtime remain **unverified on real
hardware**. Service installer/cross-user provisioning and security approval belong
to the independent reviewer and service work, not this transport correction.

Rejected/disconnected peers are isolated from listener failures. On Unix, a failed
Tokio peer_cred query (macOS LOCAL_PEEREPID before getpeereid) drops only that stream.
The review's macOS early-close mechanism was source-checked, not runtime-observed
here. No credential is sent before kernel server identity matches. Fatal listener
errors return; rejected/busy peers are paced and Windows replenishment has three
bounded attempts. There is no indefinite error retry loop.

Replace the 300-second total lease with bounded slots and partial-frame deadlines:
eight initialized authenticated clients may stay idle indefinitely; unauthenticated
or uninitialized clients have five seconds each. A started frame has 300 seconds;
writes have five seconds, both directions have an 8192-byte including-newline cap.
This accepts same-user slot occupancy rather than silently ending normal Claude
Code idle sessions. Oversized responses terminate that connection and withhold
partial bytes; pagination is required before inventory exceeds the cap. HTTP
shutdown drains for at most five seconds. No M2 service/Claude Code acceptance is
claimed. Fresh independent review and green hosted checks are still required.
