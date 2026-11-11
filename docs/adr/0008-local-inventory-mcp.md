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
handshake, frame reads/writes and connection lease are bounded. Inventory/activity
semantics and persistent host identity are shared without a second storage path.

This is foreground same-user development trust, not binary attestation or service
isolation. Same-user credential access is inherent. SYSTEM/root identity values
are supported but privileged paths have not been exercised. Unix separate-user
filesystem provisioning belongs to service integration; current socket/parent
permissions are owner-only. Existing endpoint objects are never reclaimed.

See crates/arxd/README.md for exact bounds, configuration and references. Services,
dedicated-account execution, Claude Code acceptance and their production security
review remain outstanding. There is no change to the settled execution design.
