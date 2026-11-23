# Dashboard HTTP API (proposed)

Status: **proposed**. Nothing here is served by `arxd` yet. The dashboard's
HTTP adapter (`ui/src/api/http.ts`) already speaks this contract, and the
TypeScript types in `ui/src/api/types.ts` with the decoders in
`ui/src/api/decode.ts` are the precise shapes. A backend PR implements this
document; any change to it must update those three files in the same PR.

Until the backend lands, dev and test builds use a deterministic stand-in
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
  responses (2 MiB JSON body, per-field limits in `decode.ts`).
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

## Authentication: local owner password

Chosen over a one-time pairing token because the dashboard is used daily from
several browsers and Electron; a pairing token would need its own long-lived
credential afterwards anyway. Pairing tokens remain the host *join* mechanism.

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

Requires `X-Arx-CSRF`. Revokes the session server-side and clears the cookie.
`204`.

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

- `os`: `windows | macos | linux`. `arch`: `x86_64 | arm64`.
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
- `snapshots` ≤ 256. The UI computes host compatibility from `arch`,
  `guestOs` and host reachability; the server remains the authority.

## Streams: terminal and console

Both use a short-lived ticket so no credential appears in a URL:

1. `POST` the stream endpoint with `X-Arx-CSRF`. Response
   `{"streamPath":"/api/v1/streams/<id>","ticket":"<16..256 chars>"}`.
   The ticket is single use, bound to the session, valid for 30 s.
2. Open `wss://<same origin><streamPath>`. The server checks the session
   cookie and `Origin` on upgrade.
3. The first client message is `{"type":"auth","ticket":"..."}`. Anything
   else, a reused or expired ticket, or no auth within 5 s closes the socket.

The client rejects a `streamPath` outside `/api/v1/streams/`.

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

Any malformed server message makes the client close the session.

### Console: `POST /api/v1/vms/{id}/console`

View-only frames for v1, from QEMU screendumps (or Screen Sharing capture for
macOS guests). Interactive VNC (noVNC-style RFB over this same socket) can
replace it later without changing the ticket flow.

Server → client, repeating pairs:

1. Text `{"type":"frame","seq":7,"width":1024,"height":768,"mime":"image/png","capturedAt":"..."}`
   (`mime` is `image/png` or `image/jpeg`; the client refuses others,
   including SVG).
2. One binary message with the image bytes (≤ 8 MiB).

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
`/api/*` with 503 `unavailable` until host connection is designed (proposal:
a host picker that loads the chosen host's served dashboard, with navigation
locked to that host's origin).
