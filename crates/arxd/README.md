# arxd inventory development slice

This is a foreground, inventory-only developer server, using official rmcp
3.9.0 Streamable HTTP through axum. It does not complete milestone 2. The source
API was checked against the downloaded official rmcp crate (including
`streamable_http_server/tower.rs`, `handler/server.rs`, `model.rs`, and the
reqwest client implementation); `cargo info rmcp@3.9.0` reports minimum Rust
1.88. Workspace Rust remains pinned at 1.95.0. Primary upstream reference:
https://docs.rs/rmcp/3.9.0/rmcp/transport/streamable_http_server/.

Run `cargo run -p arxd --locked -- --config <absolute-private-config.json>`.
No config means exit without listening. Missing/null `listen` disables HTTP.
Only explicit numeric loopback addresses are accepted; `/mcp` is the endpoint.
Host must exactly match the bound numeric address and port. All Origin headers
are rejected: this slice supports non-browser clients only. Bodies are limited
to 8192 bytes, including chunked bodies, with a five-second read deadline and
at most eight concurrent admitted requests. Bearer headers are removed before
SDK dispatch. There is no application logging subscriber or request-body log.
Authentication failures create neither log records nor activity rows.

Configuration (unknown fields rejected, maximum 16 KiB, maximum 32 agents):

```json
{
  "listen": "127.0.0.1:48271",
  "state_dir": "<absolute-existing-private-state-directory>",
  "agents": [{"principal": "developer-agent", "token_sha256": "<SHA-256 hex digest of token text>"}]
}
```

Supply each client a distinct token: 32 cryptographically random bytes encoded
as 64 lowercase hex characters. Hash the encoded token text with SHA-256. Never
use a login/password as a token. Only hashes are retained in server configuration;
raw request tokens are used transiently for verification. Digest equality uses
`subtle` constant-time comparison, over every configured agent. No credentials
are generated or printed by arxd, and token administration has no HTTP endpoint.
Revocation takes effect by removing a hash and restarting the daemon. Clients
must protect their token files too. Do not pass tokens in shell command lines.

For example, the following owner-run Python fixture generator writes credentials
without printing them. First create a private directory and its `state` child
outside this repository. On Unix use owner-only 0700 directories. On Windows use
a protected DACL granting only the current user, SYSTEM and Administrators,
with inheritable permissions for children. No administrator action is required
for a private directory owned by the current foreground user. The generator
assumes those permissions are already correct; arxd verifies them on load.

```python
import hashlib, json, os, pathlib, secrets, sys
root = pathlib.Path(sys.argv[1]).resolve(strict=True)
assert (root / "state").is_dir()
token = secrets.token_hex(32)
def private_write(name, text):
    fd = os.open(root / name, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    with os.fdopen(fd, "w") as output:
        output.write(text)
private_write("agent.token", token)
private_write("config.json", json.dumps({
    "listen": "127.0.0.1:48271", "state_dir": str(root / "state"),
    "agents": [{"principal": "developer-agent", "token_sha256":
        hashlib.sha256(token.encode()).hexdigest()}]}))
```

Config and state paths must be absolute, normalized, existing and free of
symlinks/reparse points. Windows network/device paths and alternate data streams
are refused. Config/state DACLs reject other granted identities; Unix rejects
group/other access and requires the foreground effective UID. The private parent
directory is a trust boundary: this is not a privileged service hardened against
concurrent path replacement by the foreground owner. Never install this slice
with privilege or present it as dedicated-account execution isolation.

`list_hosts` accepts an empty argument object and reports observed OS, machine
architecture, logical CPU count, RAM and aggregate mounted-volume disk capacity
and availability, plus a persistent `host_id`. No process/tool probes execute. It explicitly reports execution
and tool discovery unavailable pending the dedicated-account backend. No hostname,
user name, mount paths, environment values or configured execution identity is
returned. Mounted volumes may overlap; disk totals are inventory facts, not a
VM storage allocation budget.

The application `host_id` is 32 bytes from the OS cryptographic random source,
encoded as 64 lowercase hex characters. It contains no hostname, account, MAC,
machine GUID or hardware fingerprint. A separate singleton metadata table in the
private activity database keeps it outside activity retention. Initialization and
the schema-version marker commit in one transaction with activity recovery;
pre-identity stores migrate without discarding their activity. The version marker
distinguishes those stores from initialized stores missing their identity table.
Missing, malformed, duplicate or unsupported-version metadata fails closed;
inventory revalidates metadata on each call and withholds results on failure.
Restore valid private state when metadata is corrupt; restarting does not repair
or replace the identity. Copying the entire state copies its identity, so separate
hosts require fresh state. This is local application identity for future routing,
not a future Raft numeric node ID or certificate identity; no Raft design changes.
Using the existing transactional store avoids another private file or write path.

Activity schema v2 is durable M2 groundwork, not M2 acceptance. Version 0
identity initialization and version 1 migration, preservation of host ID/events,
and restart reconciliation commit atomically. Legacy rows become agent/read rows
with NULL timestamps and execution metadata. Unsupported versions and malformed
columns fail closed. Timestamps are nullable bounded UTC milliseconds
(0..=253402300799999), informational rather than ordering keys.

`list_activity` accepts only `limit` (integer 1–50, default 20), `before_id`
(positive i64) and `include_reads` (boolean, default false). It returns
`{activities, next_before_id}` in descending ID order. Only the transport's
immutable principal's agent rows are visible; principal and daemon rows are
omitted. Its own committed started row bounds the SQL query, together with the
optional cursor. SQL reads at most limit+1 rows, with bounded text columns; all
returned values are validated against typed vocabulary and numeric/identity
bounds. The last returned ID becomes the next cursor only if an extra row exists.
Completion commits before results are exposed. Invalid arguments are rejected
before querying, but still audited. Unknown calls persist only `unknown_tool`.

Start commits before dispatch. `finish_typed(id, Outcome, &Metadata)` updates
exactly one started row in a transaction; missing, duplicate or already completed
rows refuse and roll back. Future execution metadata is nullable and typed, with
fixed reason/program/rights/observation/stop/tree vocabulary. No argv, environment,
workspace, executable path, output, caller tool name or arbitrary error is stored.
`Program::Path` records only the literal `path`. An observed identity is optional;
when present its SID/decimal syntax and 184-byte maximum are validated.

Cancellation, timeout and shutdown map to **Outcome::Failed plus
StopReason::Cancelled / Timeout / Shutdown**, respectively. They are not outcome
strings; `finish` rejects them. Stop reason and outcome persist in the same update.
A clean service lifecycle event may finish succeeded. `start_lifecycle(Start|Stop)`
creates only daemon/arxd lifecycle records; service Host integration belongs to the
service builder. No executor, command tool or discovery route is added here.

Restart marks unfinished rows interrupted. Retention independently keeps the
newest 200 reads and 1000 actions+lifecycle rows, pruned transactionally. Rejected
calls become reads, so read floods cannot evict action history. A file lock prevents
concurrent instances. Page size/count stay capped at 4096/1024 (4 MiB, plus bounded
rollback journal); checks run after the whole migration/start/finish transaction's
work and before commit. Completion storage failure withholds results.

Verification: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
--locked -- -D warnings`, `cargo nextest run --workspace --locked`. Tests use the
actual official SDK client over TCP for initialize/list/call, actual machine
inventory, persisted restart/revocation, and insertion/completion storage failures.
Boundary tests cover missing/wrong credentials, foreign Host/Origin, fixed and
chunked oversized bodies, config/metadata bounds, secret-marker exclusion and
private permissions. SQLite trigger failures are fault injection, not disk-failure
hardware evidence. Windows foreground inventory and DACL checks were exercised
locally. Unix permission/symlink tests exist but Linux/macOS are **unverified on
real hardware**. Windows junction rejection is **unverified on real hardware**.

Still required for M2: architecture/security review; real dedicated-account
command backend and identity evidence; rights enforcement; per-OS services;
reviewed local transport and privileged identity evidence; tool discovery as that account;
Claude Code acceptance listing the host and running `cargo --version` with
persisted principal, observed execution identity and outcome. S4U, root service,
CA keys and owner setup were not implemented or performed. The coordinator must
integrate only after M1 and obtain the required different-provider review and
required checks. No CI gates are changed by this slice.

## Opt-in local stdio bridge

The inventory daemon now also accepts an optional `local` object in its private
configuration. Omit it (or set null) to disable local IPC. HTTP remains optional;
`arxd --config <absolute-private-config.json>` can serve either or both transports.
No configured listeners exits without listening. Unknown fields, including inside
`local`, are rejected. Existing configuration still works.

Windows configuration addition:

```json
"local": {
  "address": "\\\\.\\pipe\\arxcovo-development",
  "client_identity": "<explicit allowed client SID>"
}
```

Unix configuration addition (under an existing private 0700 directory):

```json
"local": {
  "address": "/absolute/private-directory/mcp.sock",
  "client_identity": "<decimal client UID>"
}
```

The client command is `arxd mcp-stdio --client-config <absolute-private-file>`.
The private client JSON contains exactly:

```json
{
  "address": "<same local endpoint>",
  "expected_server_identity": "<explicit foreground server SID or decimal UID>",
  "token": "<64 lowercase hex token characters>"
}
```

Provision this file outside the repository with the private permissions described
above. On Windows explicitly set **both file and parent owner to the bridge's
TokenUser SID**, alongside the private protected DACL. Do not use TokenOwner or
infer identity from a newly created directory: a full administrator token commonly
creates BUILTIN\Administrators-owned objects. Provision using the client account
(or its installer), setting OWNER_SECURITY_INFORMATION to that account's user SID.
A group-owned file is rejected even if its ACL is otherwise private. Standard and
filtered users usually default to their TokenUser; elevated/full admin users need
explicit ownership; SYSTEM needs S-1-5-18 ownership for its own client files. These
are provisioning requirements, not instructions for agents to elevate. Set
expected_server_identity independently to the daemon TokenUser (pipe owner), never
to a config directory's owner. Elevated/SYSTEM runtime remains unverified here. Its parent must also be private; file and parent must belong to the bridge
user. It is limited to 16 KiB. The raw credential resides only in this private
client configuration; the daemon retains only the hash. Never put it in argv,
environment, MCP arguments or shell output. Configure MCP clients to invoke this
binary with the three arguments above and an absolute client config path. The
bridge does not start the daemon, invoke an endpoint command or access state.

Before sending **any** authentication bytes, Windows opens the exact pipe and
queries its kernel-held owner SID using GetSecurityInfo on that connected handle.
It requires the configured expected SID; inability to inspect it fails closed.
There is no OpenProcessToken-on-SYSTEM fallback. The daemon explicitly sets its
own SID as pipe owner. Pipes use first-instance reservation, remote-client
rejection, a NETWORK deny ACE, and a protected DACL. The server owner gets full
rights; the configured client SID gets specific read/write/synchronize/READ_CONTROL
rights without FILE_CREATE_PIPE_INSTANCE. This permits a future SYSTEM-owned
pipe to grant a separate client SID connectivity without granting instance creation.
The bridge uses anonymous SQOS, preventing server impersonation of the client.
See Microsoft's [pipe security reference](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)
and [GetSecurityInfo reference](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-getsecurityinfo).

Unix uses Tokio 1.48 peer_cred (kernel SO_PEERCRED on Linux; LOCAL_PEEREPID
followed by getpeereid on macOS)
to compare the connected server UID before sending credentials, and to restrict
accepted client UIDs. The socket is 0600 under an owner-only parent. Bind refuses
every existing object, including stale sockets and symlinks. Normal shutdown
unlinks only the exact socket inode/device/owner created by this instance; a
replacement is left alone. After a crash, inspect and remove a stale endpoint
manually. No agent automatically steals or deletes it.

Expected SYSTEM SID and UID 0 are valid identity values; privileged and cross-user
cases remain **unverified on real hardware**. Unix's current owner-only filesystem
policy intentionally requires same-user development fixtures; separate root-service
and client filesystem provisioning remains a service-integration prerequisite.
The current server is an **unprivileged foreground inventory development server**.
Set expected identity explicitly to that foreground user. This authenticates an OS
identity, not an executable binary. Another process of the same OS user can already
read that user's client credentials. This is not production SYSTEM-service or
account-isolation evidence.

After identity verification, the bridge sends a fixed 70-byte `ARX1 <token>\n`
preamble and waits for `OK\n`, each within five seconds. Only then does it pump
newline-delimited MCP stdio frames unchanged. Official rmcp 3.9.0
AsyncRwTransport::new_server owns MCP initialize/list/tool dispatch on the daemon,
with an 8192-byte request line cap. The bridge forwards complete request frames up to 8192 bytes and response frames up to 65536 bytes
and rejects oversized or incomplete frames without forwarding their partial data.
The API and feature names were verified against the exact downloaded crate's
`transport/async_rw.rs` and Cargo.toml, not inferred from another SDK version.
There are at most eight admitted connections, a five-second authentication limit,
a five-second initialization limit and a five-second write limit. Authenticated
initialized sessions have no arbitrary total or between-frame idle deadline. Each
may retain one of the eight slots while idle; this deliberately accepts same-user
slot exhaustion to preserve long-lived Claude Code sessions. New excess clients
are closed; busy/rejected accepts are paced at 25 ms (at most 40/s per listener).
Once the first byte arrives, a frame must complete within five minutes. Both
server and bridge enforce 8192-byte requests and 65536-byte responses, including newline:
an oversized inventory response closes that connection without forwarding the
partial frame. Activity pages contain at most 50 validated rows.
Buffers are capped; no frame accumulation across idle periods is permitted.
Unrecoverable accept/reset errors stop the listener. Windows replenishment keeps
the reservation and attempts creation at most three times with 50 ms spacing;
persistent failure stops the listener. Windows busy connects retry every 10 ms
within the five-second connect deadline. Peer credential lookup failure always
rejects that peer; it never substitutes an identity. The first rejected peer emits
one fixed diagnostic per listener lifetime, preventing log flooding. Dropping the listener aborts admitted connection tasks. The
binary emits only fixed client-config, peer-connect/identity, peer-authentication,
server-config/state/bind/listener or transport failure categories (no source error
chains, paths or tokens). HTTP Ctrl-C shutdown again drains gracefully, bounded to
five seconds. The binary explicitly exits after bridge transport closure because Tokio stdin reads
cannot be cancelled; holding stdin open cannot orphan the bridge.

Each authenticated connection gets one immutable server-side principal. No
client metadata or tool argument chooses that principal. HTTP retains stateless
per-request principal binding. Both transports share the same Activity store and
persistent host identity, including start-before-inventory and withholding results
on activity/storage failure. `list_activity` shares those semantics. No configuration or
execution tools are exposed.

Local Windows evidence: real SDK client subprocess stdio initialize/list/call,
local-only daemon binary, concurrent principals and persisted identity, missing
daemon, wrong configured peer identity trap receiving zero bytes, malformed/missing
credentials, oversized authentication and configuration, nested unknown keys,
authentication deadline, storage-start failure, eight-connection capacity,
listener shutdown, and bridge exit with stdin held open. HTTP regression tests
remain green. Unix collision/replacement/symlink tests are compiled but **unverified
on real hardware**. Linux/macOS Rust type checks are distinct from native C builds:
normal cross-checks stop at unavailable SQLite C toolchains; a linked-SQLite
build-script override permits Rust-only checking without claiming a native link.

Still missing for M2 acceptance: independent review and hosted native checks,
services and account isolation, dedicated-account execution and rights evidence,
account-scoped tool discovery, and real Claude Code list-host plus cargo --version
acceptance with persisted execution identity/outcome. No account, service,
elevation or old Lab Manager operation was performed for this slice.
