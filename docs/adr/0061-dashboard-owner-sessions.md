# ADR 0061 Dashboard owner sessions

## Decision

Serve the dashboard from an explicit `dashboard` listener in foreground
`arxd --config FILE`. The listener has its own authority check, connection
and request limits, route table and authentication state. The existing MCP
HTTP and local transports keep their agent credentials and principal rules.
The dashboard listener never constructs a `Principal`, and MCP never
authenticates a dashboard cookie.

When the owner supplies the server certificate chain and private key, the
dashboard uses TLS 1.3 and checks the configured authority. An incomplete or
invalid TLS configuration fails startup. Plain HTTP binds only to loopback
and uses `localhost` as its authority. The cookie remains
`__Host-arx_session` with Secure, HttpOnly, SameSite=Strict and Path=/ in
both modes. The application checks the configured origin on unsafe requests
and any request that supplies an Origin header. Login is the only unsafe
request exempt from the session's CSRF token.

The owner sets the password through `arxd dashboard-password --state-dir DIR`
using standard input. The private state directory holds a version 19 Argon2id
PHC record with 65,536 KiB memory, three iterations, one lane, a 128-bit random
salt and a 256-bit hash. The daemon holds the password lock while it serves.
Stop it before replacing the password. Every admitted login hashes the
submitted password, including attempts with a wrong username.

A single worker slot bounds concurrent Argon2 work. Failures start
exponential backoff after five attempts for a socket peer IP and twelve
attempts across the listener. Backoff starts at one second and caps at 256
seconds. A bucket forgets failures after 15 minutes of inactivity. At most
256 client buckets can be active. The server does not use forwarded client IP
headers.

A fresh login creates a random 256-bit session ID and an independent random
CSRF token. The in-memory registry holds only the session ID's SHA-256
digest and the CSRF token. The presented session is replaced under the same
registry lock. The lock also linearizes logout and session expiry. Session
expiry uses a monotonic clock, with a 12-hour idle limit and a seven-day
absolute limit. There are at most 256 active sessions. A restart discards
all sessions and rate limits. Successful logout revokes the session before
returning 204. There is no durable session material that can restore a
revoked ID after a crash.

The dashboard loads a bounded asset snapshot from the configured build
directory before accepting requests. It rejects links and reparse points,
validates opened files against the walked paths, and serves only in-memory
bytes. The owner installs the build in a directory controlled by their
installation account.

## Data and integration

The foreground listener projects the daemon's local inventory and owner-wide
activity feed. The VM endpoint reads a configured controller catalog through
a read-only connection. Schema v8 records CPU and memory at admission and
the timestamp of successful snapshot creation. The v7-to-v8 upgrade only adds
nullable columns and keeps the forward-only rule of ADR 0060: a v7 opener
refuses v8 state. Older rows preserve unknown
values as null in the API. The dashboard reports the test VM catalog's
recorded resources and does not probe guests as part of an HTTP read.

The foreground configuration has no connection to the installed service's
cluster membership process. Remote inventory and static VM catalog
integration require data sources outside this foreground reader. Terminal,
console and WebSocket handling belong to B2.
