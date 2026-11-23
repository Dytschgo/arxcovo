# Dashboard HTTP API (proposed)

The dashboard's HTTP adapter (`ui/src/api/http.ts`) speaks this contract, and the
TypeScript types in `ui/src/api/types.ts` with the decoders in
`ui/src/api/decode.ts` are the precise shapes. A backend PR implements this
document; any change to it must update those three files in the same PR.

Dev and test builds use a deterministic stand-in
adapter (`ui/src/api/standin.ts`) and show a "Demo data" banner. Production
builds always use HTTP: the stand-in branch is compiled out, and the desktop
smoke test asserts that no fixture data reaches `ui/dist`.

## Principles

- Same origin only. Each host serves the dashboard and this API from one
  origin over HTTPS (or loopback HTTP for local use). No CORS headers.
- One owner account per cluster. Agents never use this API; they keep MCP.
- JSON in and out, UTF-8, `Content-Type: application/json`. Times are
  RFC 3339 / ISO 8601 UTC strings. Sizes are integer bytes.
- Identifiers match `^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$`. The client refuses
  anything else before sending, and the server must too.
- Every list is bounded (see each endpoint). The client rejects oversized
  responses: it counts body bytes as they stream in and aborts above 2 MiB,
  with or without `Content-Length`; per-field limits are in `decode.ts`.
- Errors use the HTTP status plus `{"error":{"code":"...","message":"..."}}`.
  `message` is short (≤ 200 chars), human readable and contains no secrets,
  paths or stack traces; the UI may show it.

| Status | `code` the UI expects | Meaning |
| --- | --- | --- |
| 400, 422 | `invalid_request` | Bad arguments |
| 401 | `unauthorized` | No or expired session; UI returns to login |
| 403 | `forbidden` | Bad CSRF token or Origin |
| 404 | `not_found` | Unknown id |
| 429 | `rate_limited` | Login throttled |
| 5xx | `unavailable` | arxd cannot serve this now |

A 401 ends only the session the request was sent under. If the client has
signed in again before a 401 for an earlier session arrives, it keeps the
newer session and token and reports that one request as failed
(`unavailable`) instead of returning to login.

## Authentication: local owner password

Chosen over a one-time pairing token because the dashboard is used daily from
several browsers and Electron; a pairing token would need its own long-lived
credential afterwards anyway. Pairing tokens remain the host *join* mechanism.

Provision the password with `arxd dashboard-password --state-dir DIRECTORY`
while the dashboard daemon is stopped. The command reads one UTF-8 line from
standard input and hides terminal input. Use the same private state directory
as the daemon's `state_dir`.

Set `dashboard.listen` and `dashboard.dist` in the foreground daemon's
private JSON config to enable the listener. The dist path is an absolute path
to an owner-controlled production build. For plain loopback HTTP, open
`http://localhost:<port>`. The Secure cookie uses the browser's localhost
exception. HTTPS uses `dashboard.tls.authority`, DER certificate paths in
`dashboard.tls.certificates` and a private DER key at
`dashboard.tls.private_key`. Use the configured authority in the browser.
The authority is a host with an optional port that must equal the
`dashboard.listen` port. The daemon compares `Host` and `Origin` exactly with
the browser's (WHATWG URL) serialization, so startup refuses any host not
already written that way: a lowercase ASCII DNS name without empty labels
(one trailing dot is kept), a dotted-quad IPv4 address (not `127.1` or
`0x7f.0.0.1`), or a bracketed IPv6 address in compressed lowercase hex with
no embedded IPv4 (`[::1]`, not `[0:0:0:0:0:0:0:1]`; `[::ffff:c000:201]`, not
`[::ffff:192.0.2.1]`). Uppercase, userinfo and zone identifiers are refused.
Only the port is normalized: the default 443 and leading zeros are dropped,
so `owner.example:443` is served as `https://owner.example` and
`owner.example:08443` as `https://owner.example:8443`.
`dashboard.vm_state` can point to the controller's private catalog directory
after the current controller has opened it. The listener and session details
are recorded in [ADR 0061](adr/0061-dashboard-owner-sessions.md).

Server requirements:

- The owner password is set on the host (CLI or first-run), stored only as an
  Argon2id hash (memory ≥ 64 MiB, t ≥ 3) in arxd's private state. No default
  password. Never logged.
- Compare in constant time. Throttle failed logins per client and globally
  (for example 5 failures → exponential backoff, answered with 429).
  The error for a wrong username and a wrong password is identical.
- On success, issue a fresh random session id (≥ 128 bits) and set
  `Set-Cookie: __Host-arx_session=<id>; Path=/; Secure; HttpOnly; SameSite=Strict`.
  Store only a hash of the id server-side. Rotate on every login.
  Idle timeout 12 h, absolute timeout 7 days (proposed defaults).
- Return a CSRF synchronizer token bound to the session in the JSON body.
  The client keeps it in memory only (never localStorage/sessionStorage)
  and sends it as `X-Arx-CSRF` on every `POST`/`PUT`/`PATCH`/`DELETE`.
  The server rejects unsafe requests whose token or `Origin` header does not
  match (403).
- Every response sets `Cache-Control: no-store`, `X-Content-Type-Options: nosniff`,
  `Referrer-Policy: no-referrer`, and HTML responses set the CSP below.

### `GET /api/v1/session`

Returns the current session so a reload can recover the CSRF token.
`200 Session` or `401`.

```json
{ "user": { "name": "owner" }, "csrfToken": "<16..256 chars>", "expiresAt": "2026-11-24T00:00:00Z" }
```

### `POST /api/v1/session`

Body `{"username":"owner","password":"..."}` (password ≤ 1024 chars).
`200 Session` with the cookie, `401` for bad credentials, `429` when
throttled. This is the only unsafe request without `X-Arx-CSRF`; the server
must still check `Origin`.

### `DELETE /api/v1/session`

Requires `X-Arx-CSRF`. Revokes the session server-side, clears the cookie,
and closes every open stream of that session (see "Stream lifetime"). `204`
only after the revocation is durable; `401` if the session was already gone.

The client reports "signed out" only after `204` or `401`. On any other
answer or a network failure it keeps the session and its CSRF token, tells the
owner that sign-out failed and that the session may still be active, and
offers a retry that sends `DELETE` again. A client that holds no CSRF token
(after a 401, a stream's 4401 or an earlier sign-out) never assumes the cookie
is gone: it first sends `GET /api/v1/session`. A `401` there confirms there is
nothing to revoke; a returned session is revoked with `DELETE` as above; any
other failure is reported as a failed sign-out. Only an actual `401` response
confirms: a `DELETE` the client could not send is a failed sign-out too.

## Hosts

### `GET /api/v1/hosts` → `{"hosts": Host[]}` (≤ 256)
### `GET /api/v1/hosts/{id}` → `Host`

```json
{
  "id": "atlas", "name": "atlas",
  "os": "linux", "osVersion": "Ubuntu 24.04.3 LTS",
  "arch": "x86_64",
  "cpu": { "model": "AMD Ryzen 9 7950X", "cores": 16, "usedPercent": 38 },
  "memory": { "totalBytes": 137438953472, "usedBytes": 76235669504 },
  "disk": { "totalBytes": 4398046511104, "freeBytes": 2858730232217, "capBytes": 2199023255552 },
  "tools": [{ "name": "git", "version": "2.43.0" }],
  "reachability": "online", "reachabilityReason": null,
  "lastSeenAt": "2026-11-23T12:00:00Z",
  "mode": "always_on", "agentRights": "standard"
}
```

- `os`: `windows | linux` (v1; macOS joins with Mac host support on the roadmap). `arch`: `x86_64 | arm64`.
- `reachability`: `online | degraded | offline`; `reachabilityReason` is a
  short explanation when not online (for example the owner reserve is active).
- `usedPercent`, `usedBytes`, `capBytes` are `null` when unknown or unset.
- `tools` (≤ 128) is the `discover_tools` result for that host.
- `mode`: `always_on | opportunistic`. `agentRights`: `standard | admin`.

## Activity

### `GET /api/v1/activity` → `ActivityPage`

Query: `limit` (1–50, default 20), `before_id` (cursor, > 0), `outcome`,
`tool` (`^[a-z_]{1,64}$`), `host_id`, `include_reads` (`true` to include
read-only tools such as `list_hosts`). Newest first, ids strictly
descending; `nextBeforeId` is the last returned id when more exist, else
`null`. This mirrors the existing MCP `list_activity` paging.

```json
{
  "entries": [{
    "id": 64, "tool": "run_command", "outcome": "succeeded",
    "actor": { "kind": "agent", "name": "claude-code-1" },
    "hostId": "atlas", "vmId": null,
    "summary": "cargo test --workspace",
    "exitCode": 0,
    "startedAt": "2026-11-23T11:57:00Z", "finishedAt": "2026-11-23T11:58:00Z"
  }],
  "nextBeforeId": 44
}
```

- `outcome`: `started | succeeded | failed | rejected | refused | interrupted`.
- `actor.kind`: `agent | owner | system`; `actor.name` is the agent identity.
- `summary` (≤ 512 chars) is produced server-side from bounded metadata and
  must already be redacted: no secrets, tokens, environment values or full
  command output.

## VMs

### `GET /api/v1/vms` → `{"vms": Vm[]}` (≤ 1024)
### `GET /api/v1/vms/{id}` → `Vm`

```json
{
  "id": "ubuntu-build-02", "name": "ubuntu-build-02", "kind": "test",
  "template": { "id": "ubuntu-24-04-x64", "version": 7 },
  "guestOs": "linux", "guestOsVersion": "Ubuntu 24.04.3 LTS",
  "arch": "x86_64", "hostId": "atlas",
  "state": "running", "ready": true,
  "expiresAt": "2026-11-29T12:00:00Z",
  "cpus": 8, "memoryBytes": 17179869184, "diskBytes": 85899345920,
  "snapshots": [{ "id": "snap-deps", "name": "deps-installed", "createdAt": "2026-11-23T02:00:00Z", "sizeBytes": 2147483648 }]
}
```

- `kind`: `test | static`. A test VM always has `expiresAt`; a static VM has
  `null`.
- `state`: `creating | running | stopped | snapshotting | migrating | failed | destroying`.
- `ready`: the guest agent answered its readiness check.
- `cpus`, `memoryBytes` and snapshot `createdAt` are `null` when the VM
  catalog predates those observations. `diskBytes` is the VM's persistent
  disk allocation recorded by the controller.
- `snapshots` ≤ 256. The UI computes host compatibility from `arch`,
  `guestOs` and host reachability; the server remains the authority.

## Streams: terminal and console

Both use a short-lived ticket so no credential appears in a URL:

1. `POST` the stream endpoint with `X-Arx-CSRF`. Response
   `{"streamPath":"/api/v1/streams/<id>","ticket":"<16..256 chars>"}`.
   The stream id and the ticket are independent CSPRNG values of at least
   128 bits. The server stores only a hash of the ticket, bound to the
   session, the stream id, the action (`terminal` or `console`) and the
   target (host or VM id). It is valid for 30 s.
2. Open `wss://<same origin><streamPath>`. The server checks the session
   cookie and `Origin` on upgrade.
3. The first client message is `{"type":"auth","ticket":"..."}`. Redemption
   is atomic (compare-and-delete): the ticket must match this stream id,
   action, target and the cookie's live session, and it runs under the same
   session lock as revocation (see "Stream lifetime"), so a ticket cannot be
   redeemed after its session's generation changed. A reused, expired or
   mismatched ticket, any other first message, or no auth within 5 s closes
   the socket with **4408** ("ticket rejected": reconnectable, the session
   may still be live). Only a failed *session* check closes with 4401.

The client rejects a `streamPath` outside `/api/v1/streams/`.

### Stream lifetime

Authorization is not only checked at the upgrade. The server must:

- Close every stream of a session when that session is signed out, expires
  (idle or absolute) or is revoked, with WebSocket close code **4401**
  ("session ended"). Implement this with a per-session generation counter:
  the stream records the generation at redemption, and a session registry
  holds each open stream's cancellation handle so revocation reaches it
  immediately rather than at the next message.

**Ordering with revocation.** Revocation (sign-out, expiry, revocation) and
every authorization decision a stream makes are linearized by one
per-session lock:

1. Revocation takes the lock, bumps the generation, marks the session dead,
   then (still under the lock) triggers every registered stream's
   cancellation handle. `DELETE /api/v1/session` answers 204 only after this.
2. **Command admission.** A terminal `run` takes the same lock, checks the
   session is live and the generation unchanged, and registers the command
   with the executor's cancellation path *before* releasing the lock and
   spawning. The executor's existing pre-spawn cancellation check then
   refuses a command whose cancellation fired in between. Result: a command
   admitted before the bump may run to its executor cleanup, but its output
   stops at the close; a command not yet admitted is refused and never spawns.
3. **Frame delivery.** Producers (the executor's output, the console
   capturer) never write to the socket. They enqueue into a bounded
   per-connection outbound buffer, and only after taking the session lock
   and re-checking that the session is live and the generation unchanged;
   the lock is released before any I/O. **The server never holds the session
   lock across a blocking send.** The socket is drained by non-blocking or
   async writes outside the lock. A full buffer applies backpressure to the
   producer (for example pausing output reads), never to the lock.
   - At the revocation linearization point (step 1, under the lock) the
     server stops producing for the session's streams: every later enqueue
     fails the generation check and is refused, and data still waiting in the
     outbound buffer is discarded.
   - **Commit point.** A message is *committed for sending* only when the
     writer, holding the session lock, re-checks that the generation is
     unchanged and moves the message out of the outbound buffer into its
     single in-flight slot. The writer releases the lock and only then calls
     the transport. Revocation takes the same lock, so every message is
     either committed before the revocation linearization point (it may be
     written) or still in the buffer at that point (it is discarded and
     never written). A writer that dequeued nothing under the lock cannot
     start a write afterwards; there is no lock-free dequeue path.
   - Data the transport accepted before that point, meaning the socket send
     buffer plus the at most one message committed to the in-flight slot,
     may still be delivered. A WebSocket close cannot recall it
     ([RFC 6455 §5.5.1](https://www.rfc-editor.org/rfc/rfc6455.html#section-5.5.1)),
     and the contract makes no stronger delivery claim.
   - After revocation the server queues the 4401 close and sends it as soon
     as the transport accepts it. The 5 s deadline starts at revocation: if
     the connection cannot drain its buffer and send the close by then (a
     stalled peer), the server aborts the TCP connection.
   - A close frame's code is fixed when the server hands the frame to the
     transport. A writer that already took another close (for example 1001)
     and is waiting for the transport reads the pending close again under the
     session lock in the same step that hands the frame over. So a close
     handed over after revocation carries 4401. A close the transport had
     already accepted before revocation keeps its code and may still reach
     the peer after sign-out completes; a stalled connection may instead be
     aborted at the deadline above.
4. **Expiry wake-up.** At redemption the server arms a timer per stream for
   the session's absolute expiry, and re-arms an idle-expiry timer whenever
   the session's idle deadline moves. When a timer fires, its callback takes
   the session lock and re-reads the *live* deadlines: it revokes only if the
   absolute deadline has passed or the current idle deadline has passed.
   A callback scheduled for an idle deadline that has since been renewed
   does nothing except re-arm for the new deadline. A revoking callback then
   follows steps 1–3, closing with 4401 whether the stream is idle (no
   traffic) or busy (a command running or frames flowing).

Server integration tests (backend PR) must cover: revocation during command
admission; expiry during a running command; no code path holding the session
lock across a socket send (revocation completes while a send is
backpressured); an enqueue refused after revocation, and buffered but
unwritten data discarded; revocation on both sides of the commit point (a
message committed just before revocation may be written and is followed by
the 4401 close; a message still buffered at revocation is never written, and
4401 is the next frame the peer sees after any committed message); a stalled
drain aborted within 5 s of revocation;
ticket redemption racing revocation; and a session renewal racing a callback
for the old idle deadline (the session stays live).

The client already ignores everything that arrives after it observes a close,
so data the transport accepted before a 4401 is harmless to the UI.

- Re-validate the session (live, unexpired, same generation) before every
  terminal `run` and before enqueueing every console frame or output chunk,
  as above. A failed check closes the socket with 4401 and runs nothing.
- Close with **4403** when the target stops being allowed (for example the
  host is removed or the VM is deleted), and **1001** on shutdown. Close
  reasons are short (≤ 123 bytes) and contain no secrets.
- Bound cleanup: on any close, cancel the running terminal command through
  the executor's normal cancellation (same path as `{"type":"cancel"}`),
  record its activity entry as `interrupted`, and release the stream within
  the executor's kill timeout. A console stream stops capturing at once.

The client treats any close it did not start as server-initiated: it shows
the reason in the terminal or console view and offers Reconnect, and it
ignores any message that arrives after the close. On 4401 it also forgets the
in-memory CSRF token, ends the local session and returns to the login form,
even when an earlier closing notice (such as `VM stopped`) already closed the
view. A 4401 applies only to the session the stream was opened under: when a
newer sign-in has replaced that session, the client just marks that stream
closed and keeps the newer session and its token.
On 4408 it reports a ticket or handshake failure and offers Reconnect,
without ending the session.

### Terminal: `POST /api/v1/hosts/{id}/terminal`

A line-oriented session on the host's bounded `run_command` executor (same
policy, identity and limits as MCP; no shell, no PTY). Every command is an
activity entry with `actor.kind = "owner"`. 503 when the host is offline.

Client → server (JSON text frames, ≤ 128 KiB):

- `{"type":"run","tool":"git","args":["--version"],"timeoutS":60}`:
  `tool` must be a discovered tool; `args` ≤ 128 items of ≤ 8192 chars;
  `timeoutS` 1–3600, optional. Only one command runs at a time.
- `{"type":"cancel"}`: interrupt the running command.

Server → client:

- `{"type":"ready"}`: authenticated and idle.
- `{"type":"started","commandId":"c1","tool":"git","args":["--version"]}`
- `{"type":"output","commandId":"c1","stream":"stdout|stderr","data":"..."}`:
  UTF-8 text chunks; total output per command bounded by the executor
  (`output_bytes`).
- `{"type":"exit","commandId":"c1","exitCode":0,"outcome":"succeeded","truncated":false}`:
  `exitCode` may be `null` (refused, killed).
- `{"type":"error","code":"...","message":"..."}`

Any malformed server message makes the client close the session. Before
each `run` the server re-validates the session as described under "Stream
lifetime".

### Console: `POST /api/v1/vms/{id}/console`

View-only frames for v1, from QEMU screendumps. Interactive VNC (noVNC-style RFB over this same socket) can
replace it later without changing the ticket flow.

Server → client, repeating pairs:

1. Text `{"type":"frame","seq":7,"width":1024,"height":768,"mime":"image/png","capturedAt":"..."}`.
   `seq` is a safe integer that strictly increases per stream; `width` and
   `height` are 1–8192; `mime` is `image/png` or `image/jpeg` (the client
   refuses others, including SVG); `capturedAt` is an RFC 3339 instant.
2. One binary message with the image bytes (≤ 8 MiB).

The client treats any invalid metadata, binary data without metadata, or
metadata twice in a row as a protocol error: it shows an error state, emits
no frame and closes the stream.

`{"type":"closed","reason":"..."}` ends the stream (for example the VM
stopped). Suggested rate: 1–2 frames per second while the socket is open.

## Content Security Policy

Served on the dashboard HTML by arxd (and already set by the Electron shell):

```
default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' blob: data:;
font-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'self';
frame-ancestors 'none'; object-src 'none'
```

arxd's `connect-src` may be `'self'` only: modern browsers match same-origin
`wss:` under `'self'`.

## Desktop

The Electron shell serves `ui/dist` from `app://arxcovo` and answers
`/api/*` with 503 `unavailable`, for every method including the sign-in
`POST`, until host connection is designed (proposal: a host picker that loads
the chosen host's served dashboard, with navigation locked to that host's
origin).

Assets are served from an immutable in-memory table built once at startup,
never from the filesystem per request. Startup validates `ui/dist` (its real
path equals the requested path, every ancestor is a plain directory, device
and inode pinned), then walks the tree once with caps on file count, depth,
per-file bytes, total bytes and every enumerated directory entry. It refuses
symlinks and junctions (other Windows reparse tags, which Node reports as
plain files or directories, are a known limitation), reads each served file
through a handle whose identity must
match the walked file using a byte-bounded read, re-checks the directories
leading to it, and finally re-verifies the root identity. Any failure stops
the app from starting. Files changed on disk after startup are never served.
