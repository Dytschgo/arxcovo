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

The activity table stores only principal, fixed tool classification, numeric ID and outcome.
A durable transaction records `started` before inventory collection; storage
failure prevents dispatch/results. Completion failure withholds inventory and
leaves the durable started row. Restart marks unfinished rows `interrupted`.
A file lock prevents concurrent instances sharing state. Retention is 1000 rows;
database page size/count are capped at 4096/1024 (4 MiB, plus bounded rollback
journal). Older rows are pruned transactionally. Rejected calls use the fixed
inventory classification and `rejected`; attacker tool names/arguments are never
stored. There is no activity-reading tool or HTTP configuration authority.

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
stdio bridge with endpoint identity checks; tool discovery as that account;
Claude Code acceptance listing the host and running `cargo --version` with
persisted principal, observed execution identity and outcome. S4U, root service,
CA keys and owner setup were not implemented or performed. The coordinator must
integrate only after M1 and obtain the required different-provider review and
required checks. No CI gates are changed by this slice.
