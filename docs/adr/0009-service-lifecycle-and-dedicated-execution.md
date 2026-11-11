# 0009: Per-OS service lifecycle and dedicated-account execution

Status: settled implementation contract (Opus 5.5 high, architecture). Docs only.
Nothing here is implemented, and no acceptance is claimed. Every privileged
path below is **unverified on real hardware** until a disposable experiment or
the owner's install exercises it.

Owner steps: docs/service-install.md. Builds on: main `03adb434` (inventory, durable activity, host identity), proposed
ADR 0006 (`docs/early-hardware-and-raft-spikes`), and these unmerged candidates,
which are not yet independently accepted: PR14 Unix executor `a5777bb` (ADR 0007),
PR15 local transport `6f55eb2` (ADR 0008, first review failed), PR16 drive-prefix
path fix, PR17 Windows probe diagnostics and PR18 conditional `finish` (`97cea83`).
Where this ADR depends on one of them, it states the property it needs, not a
particular commit.

## Decision summary

1. `arxd` runs as one privileged OS service per host: Windows SCM `LocalSystem`,
   a Linux systemd system unit running as root, and a macOS launchd LaunchDaemon
   running as root. A separate dedicated *standard* OS account, which the owner
   creates, runs every agent command. The service never runs a command as
   itself, as the owner, or as any other fallback identity.
2. Service mode has no configuration-path argument. All paths are fixed per OS
   (see Layout). Install artifacts are constant files checked into the repo and
   embedded with `include_bytes!`. Nothing is templated or interpolated into
   unit files, plists, SCM command lines or shell text.
3. Owner-only actions run as explicit local subcommands with OS admin rights:
   `arxd service install|configure|start|stop|status|uninstall`. The owner runs
   `arxd client-config new` unelevated. No MCP or HTTP route can reach any of
   them.
4. Windows execution uses an MSV1_0 S4U batch logon as the pinned build SID,
   then a fixed `arxd exec-child` shim inside a kill-on-close Job. Linux uses
   ADR 0007's setresuid with cgroup v2, under a systemd-delegated subtree. macOS
   uses ADR 0007's setuid, then a build-UID process sweep after each run.
5. The service reads the account-scoped tool inventory by running the fixed
   `arxd tool-probe` subcommand *as the build account*. The service never
   resolves `PATH` and never opens workspace paths with its own privilege.
6. Activity schema v2 adds timestamps, an actor kind, a fixed tool vocabulary
   and the observed execution identity. Agents read only their own rows through
   `list_activity`, using keyset pagination. Output, argv, environment and paths
   are never persisted.
7. M2 does **not** need key material, so no keyfile exists. The OS-keychain
   design for M3 CA keys is a separate ADR. A root-only or SYSTEM-only keyfile
   is not an accepted substitute.

## Layout (fixed, never read from argv, environment or config)

| | Windows | Linux | macOS |
| --- | --- | --- | --- |
| binary | `{ProgramFiles}\Arxcovo\arxd.exe` | `/usr/local/libexec/arxcovo/arxd` | `/Library/Application Support/Arxcovo/libexec/arxd` |
| config | `{ProgramData}\Arxcovo\config\arxd.json` | `/etc/arxcovo/arxd.json` | `/Library/Application Support/Arxcovo/config/arxd.json` |
| state | `{ProgramData}\Arxcovo\state\` | `/var/lib/arxcovo/` | `/Library/Application Support/Arxcovo/state/` |
| endpoint | `\\.\pipe\arxcovo-arxd` | `/run/arxcovo/arxd.sock` | `/private/var/run/arxcovo/arxd.sock` |
| manager artifact | SCM service `arxd` | `/etc/systemd/system/arxd.service` (+ standard-rights drop-in) | `/Library/LaunchDaemons/com.arxcovo.arxd.plist` |

Windows resolves `{ProgramFiles}`/`{ProgramData}` only through
`SHGetKnownFolderPath` (native x64 Program Files). It then validates the result
with `checked_path`: a local disk, no reparse points, and drive-prefix ordering
per PR16. A `Layout::system()` constructor is the only one reachable from the
binary. Tests use `Layout::for_test(tempdir)`, which only library and
integration tests can construct. No CLI flag selects it.

Protection rules, checked by install and again on every service start:

- **Config and state** are confidential. Windows: a protected DACL with no
  inheritance, granting only SYSTEM and Administrators, owner `S-1-5-18`.
  Unix: `root:root`, `0700` directories and `0600` files.
- **Binary directory** is integrity-only. Only SYSTEM, Administrators or root
  (and TrustedInstaller on Windows) may write it. Read and execute stay open
  because the build account executes `arxd exec-child` and `arxd tool-probe`.
- **Endpoint directory** (Unix) is root-owned with mode `0755`. The socket is
  `chown`ed to the configured client UID with mode `0600`. Windows pipe security
  follows ADR 0008: the client SID gets specific rights, and NETWORK and remote
  clients are refused.
- **Ownership policy (strict current-user, aligned with the PR15 correction).**
  In service mode the service's *current user* is SYSTEM or root, and every
  service object must be owned by exactly that SID or UID. Group ownership is
  not accepted generally. On Windows the service calls
  `SetTokenInformation(TokenOwner = TokenUser)` at startup, before creating any
  object, so the DB, journal, lock and pipe get SYSTEM as owner instead of the
  full-admin default owner. Install runs as an elevated administrator. It
  enables `SeRestorePrivilege` only inside that process and sets owner
  `S-1-5-18` explicitly on each object it creates. Hand-edited config that
  changes the owner makes start fail with exit code 4. The documented fix is to
  rerun `arxd service configure`. Foreground development mode keeps ADR 0008's
  strict owner = foreground user rule. Test fixtures that need a different owner
  must use an explicit fixture policy, never a relaxed production check.
- **Squatting refusal.** Standard users can create folders under `C:\ProgramData`.
  If `{ProgramData}\Arxcovo` already exists, install refuses unless its owner and
  protected DACL already match the rule above. It never takes over or repairs a
  pre-existing tree. The same refusal applies to an existing Unix
  config/state/endpoint directory with the wrong owner or mode.

## Lifecycle contract

One platform-neutral core, `service::Host`, owns the state machine
`Starting → Running → Stopping → Stopped`, a shared `Shutdown` token (which also
feeds ADR 0007's `Runner::shutdown_handle`) and the fixed exit codes below.

Start order: load and validate the fixed config, then verify the layout
protections. Next, open the store, which runs restart reconciliation; storage
failure is fatal. Then write a daemon `service_start` row. Bind the local
endpoint, where "already in use" is fatal and existing endpoints are never
reclaimed. Next, run the execution preflight. A failure here is **not** fatal:
the service still starts, and execution reports `available=false` with a fixed
reason. After that, report ready. Startup tool discovery runs in the background
after ready.

Stop order:
1. Stop accepting connections.
2. Signal `Shutdown`, which cancels runs.
3. Wait up to 10 s for run threads to finish cleanup and persist outcome
   `shutdown`.
4. Write a daemon `service_stop` row.
5. Release the lock and exit 0.

Every stop path keeps the store transaction rules: no row is left `started`
except after a real crash, which restart reconciliation turns into
`interrupted`.

Exit and service-specific codes, the same on all OSes: `0` clean, `2` usage,
`3` config invalid, `4` config/state ownership or permissions, `5` store
unavailable, `6` endpoint in use or unsafe, `7` layout or binary integrity,
`10` not started by the service manager. Stderr carries only fixed category
text: no paths, values or OS error strings beyond numeric codes.

### Windows SCM host entry

- `arxd service run` requires exactly those two arguments and calls
  `StartServiceCtrlDispatcherW` for the service table entry `arxd`. If that
  returns `ERROR_FAILED_SERVICE_CONTROLLER_CONNECT`, `arxd` exits with code 10.
- `ServiceMain` registers `RegisterServiceCtrlHandlerExW` and reports
  `START_PENDING` (wait hint 10 s, with checkpoints), and then `RUNNING`.
  `RUNNING` accepts `SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_PRESHUTDOWN`.
- On `STOP` or `PRESHUTDOWN` the handler reports `STOP_PENDING` with wait hint
  5 s and signals the token. A checkpoint is reported at least every second.
  The final report is `STOPPED`, with `ERROR_SERVICE_SPECIFIC_ERROR` and one of
  the codes above on failure. `INTERROGATE` reports the current state.
- The tokio runtime is built on the ServiceMain thread.
- Install registers the service through the SCM API:
  - `CreateServiceW` with `SERVICE_WIN32_OWN_PROCESS`, auto start (delayed),
    `LocalSystem`, and an ImagePath of exactly the quoted fixed binary path
    followed by ` service run`.
  - `SERVICE_CONFIG_SERVICE_SID_INFO` set to unrestricted.
  - `SERVICE_CONFIG_REQUIRED_PRIVILEGES_INFO` limited to `SeTcbPrivilege`,
    `SeAssignPrimaryTokenPrivilege`, `SeIncreaseQuotaPrivilege` and
    `SeChangeNotifyPrivilege`. Sufficiency of this list is **unverified**: the
    hosted probe ran with the full SYSTEM token. If the service experiment shows
    a missing privilege, add exactly that one and record it here.
  - Preshutdown timeout 30 s.
  - Failure actions: restart at 5 s and 30 s, then none, with a 1-day reset,
    applied to crashes only (`fFailureActionsOnNonCrashFailures = FALSE`). A
    config error reported as `STOPPED` therefore does not restart in a loop.
- Dependency: only new `windows-sys` features (`Win32_System_Services`,
  `Win32_Security_Authentication_Identity`, `Win32_System_JobObjects`,
  `Win32_System_Pipes`, `Win32_NetworkManagement_NetManagement`,
  `Win32_System_StationsAndDesktops`, `Win32_UI_Shell`). Do **not** add
  `windows-service` or other crates.

### Linux systemd unit (constant `packaging/linux/arxd.service`)

```ini
[Unit]
Description=Arxcovo host service
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=notify
NotifyAccess=main
ExecStart=/usr/local/libexec/arxcovo/arxd service run
Restart=on-failure
RestartSec=5s
RestartPreventExitStatus=2 3 4 5 7 10
TimeoutStartSec=30s
TimeoutStopSec=30s
KillMode=mixed
Delegate=memory pids
DelegateSubgroup=supervisor
RuntimeDirectory=arxcovo
RuntimeDirectoryMode=0755
UMask=0077

[Install]
WantedBy=multi-user.target
```

The standard-rights hardening lives in the constant drop-in
`packaging/linux/50-arxcovo-standard-rights.conf`, installed at
`/etc/systemd/system/arxd.service.d/`:

```ini
[Service]
NoNewPrivileges=yes
CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN
ProtectSystem=full
ProtectHome=yes
PrivateTmp=yes
```

These settings also apply to build children, which is intended. For example,
`ProtectHome` hides the owner's home directories from builds. Admin rights need
`sudo` (set-ID), which is impossible under these settings, so admin mode
*removes* this drop-in. That keeps both artifacts constant and additive, with no
reset syntax. At startup the service reads its own `NoNewPrivs` and `CapBnd`
from `/proc/self/status` and reports `service_hardening: standard|none`. When
config says `admin` but the hardening is present, admin execution is refused
with reason `service_reconfiguration_required`.

Readiness is an in-tree `sd_notify` that sends `READY=1` and `STOPPING=1` over
`NOTIFY_SOCKET`, supporting abstract `@` names. It is about 30 lines on top of
`libc`/std, with no libsystemd crate. Requires systemd ≥ 254
(`DelegateSubgroup=`). On older systemd, execution reports
`mechanism_unavailable`; inventory still works. The service enables `+memory
+pids` in its unit cgroup's `subtree_control` and creates `runs/`, which it
passes as ADR 0007's `cgroup_parent`. At startup, the janitor kills
(`cgroup.kill`) and removes every leftover `runs/*` leaf before execution
becomes available.

### macOS launchd (constant `packaging/macos/com.arxcovo.arxd.plist`)

The plist sets:

- `Label com.arxcovo.arxd`
- `ProgramArguments` = [fixed binary, `service`, `run`]
- `RunAtLoad true`
- `KeepAlive {SuccessfulExit false}`
- `ThrottleInterval 10`
- `ExitTimeOut 30`
- `Umask 63`
- `StandardErrorPath /Library/Logs/Arxcovo/arxd.log` (the directory is
  root-owned with mode `0700` and created by install)

There is no `UserName` key (root) and no shell. Shutdown is on SIGTERM. The
service creates its endpoint directory itself and refuses an existing one with
the wrong owner or mode.

### Unix signals

`SIGTERM` and `SIGINT` mean graceful stop. `SIGHUP` is ignored: config changes
require a restart, and there is deliberately no live reload. `SIGPIPE` is
ignored.

### Owner subcommands

Each one validates first and then performs fixed OS calls. Tests inject an
`OsManager` trait that records calls instead of making them.

- `service install`: requires admin or root, and requires running from the fixed
  binary path. It verifies binary-directory integrity and creates the layout
  directories with the protections above. It then registers the service (SCM
  API, or writes the constant unit and drop-in or plist), but **does not start
  it**. It is idempotent only when the existing state is byte- and ACL-identical.
- `service configure`: requires admin or root. It writes the config atomically:
  a temp file in the same directory, explicit owner and DACL or mode, then
  rename. The config is built only through `serde` serialization from
  validated values (no string assembly). Name-to-identity resolution happens
  here, and the result is pinned. On Windows, configure grants
  `SeBatchLogonRight` to the pinned build SID only, using `LsaAddAccountRights`
  (as in the probe's `ProbeBatchRight`). If configure added the right, it
  records that in the config, and uninstall removes it. On Linux, configure adds
  or removes the standard-rights drop-in according to the configured rights,
  then runs `systemctl daemon-reload`.
- `service start|stop|status|uninstall`: Windows uses the SCM API. Unix runs
  exactly `/usr/bin/systemctl` or `/bin/launchctl` with a fixed argv vector,
  directly and without a shell. `status` prints one fixed token: `not-installed`,
  `stopped`, `starting`, `running`, `stopping` or `failed`. `uninstall` stops
  and deregisters the service and removes the manager artifacts. It **never**
  deletes state, config or the build account: the owner deletes those by hand.
- `client-config new --principal NAME --out PATH`: run unelevated by the owner.
  It writes a private client config: 32 random bytes from the OS RNG as hex, and
  the fixed expected server identity (`S-1-5-18` or UID `0`). It uses exclusive
  create and applies ADR 0008's owner rules. If the parent directory is missing,
  it creates it private and owned by the current user. It refuses an existing
  parent that is not private. It then prints only the principal and
  the token's SHA-256. The hash is not secret; the owner passes it to
  `configure`. This updates the current README statement that arxd generates no
  credentials: generation exists only in this owner-local subcommand.

## Service config (schema `arxd-service/1`, `deny_unknown_fields`, 16 KiB cap)

```json
{
  "schema": "arxd-service/1",
  "client": {"identity": "<owner SID | decimal UID>"},
  "build_account": {"name": "Arxcovo", "identity": "<SID | uid>", "gid": null},
  "rights": "standard",
  "workspace": "<absolute local path>",
  "environment": {"PATH": "...", "CARGO_HOME": "...", "RUSTUP_HOME": "..."},
  "limits": {"max_concurrent_runs": 1, "memory_bytes": 8589934592, "pids": 512},
  "agents": [{"principal": "claude-code", "token_sha256": "<64 hex>"}],
  "granted_batch_right": true
}
```

Validation rules:

- The build identity must not be the client, SYSTEM, LocalService,
  NetworkService, root or UID 0.
- In standard mode the build account must not belong to any administrative group
  (Windows Administrators, Linux `sudo`/`wheel`, macOS `admin`).
- The workspace must already exist. It must be on a local disk, free of reparse
  points and symlinks, and lexically disjoint from every layout path. Configure
  inspects it by metadata only and never creates it.
- Environment keys come from a per-OS allowlist. ADR 0007 defines the Unix list.
  The Windows list is `PATH`, `PATHEXT`, `SystemRoot`, `SystemDrive`, `TEMP`,
  `TMP`, `CARGO_HOME`, `RUSTUP_HOME`, `LANG` and `TZ`. At most 16 entries, each
  value at most 4096 bytes and without NUL.
- `max_concurrent_runs` is 1–4. On macOS it is exactly 1, because of the sweep.
- Agent principals must not equal the reserved daemon principal `arxd`.
- Service mode rejects ADR 0008 `listen` (HTTP) and `local` keys. For M2 the
  service exposes only the fixed local endpoint. HTTP remains available in
  foreground development mode, which has **no execution capability at all**.

ADR 0007's protected directories are derived from Layout (config and state),
never entered by the owner. The service, not the owner, reads its Linux
`cgroup_parent` from `/proc/self/cgroup`.

## Execution contract

### Preflight

Preflight runs at start and again before **every** run. Its failure reasons form
a fixed enum, reported in `list_hosts` and stored in the activity `reason`:

- `not_configured`, `service_mode_required`
- `account_missing`, `account_identity_changed` (name now resolves to a
  different SID or UID)
- `rights_mismatch` (the observed admin membership disagrees with `rights`)
- `batch_right_missing`
- `mechanism_unavailable` (with a numeric NTSTATUS or errno)
- `service_reconfiguration_required`
- `quarantined` (a previous cleanup is unconfirmed)
- `busy`
- `platform_unsupported`

No reason ever falls back to another identity.

### Windows

A blocking worker performs these steps:

1. Resolve the account and compare it with the pinned SID.
2. `LsaRegisterLogonProcess`, then `LsaLogonUser` with MSV1_0 S4U and logon
   type Batch, as in `docs/windows-identity-probe.md` steps 3–5. Read the raw
   `TokenGroups` with bounds checks.
3. Check the token against the configured rights:
   - Standard requires `TokenElevationTypeDefault` and Medium integrity, with no
     Administrators entry (including deny-only).
   - Admin requires Administrators membership. Use the token's
     `TokenLinkedToken` when the token comes back Limited, and require
     `TokenElevationTypeFull` and High integrity. This is the documented
     `TokenLinkedToken` behaviour for a SeTcb holder. It is unverified for S4U
     batch logons.
4. Create pipes for stdin (the spec), stdout and stderr. Pass only those three
   handles through `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`.
5. Create `arxd.exe exec-child` suspended, with these settings:
   - `lpApplicationName` is the fixed binary path.
   - `lpCurrentDirectory` is the binary directory, which is trusted, rather than
     the workspace.
   - `DETACHED_PROCESS | CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT`.
   - The configured environment block.
   - The desktop is a service-created non-interactive window station and desktop
     whose DACL grants only the pinned build SID the minimum rights. `WinSta0`
     and the service's own window station are never modified. This avoids the
     known session-0 `0xC0000142` DLL-init failure for user32-loading tools, and
     it is a permission choice on a *private* object only.
6. Assign the process to an unnamed Job with these limits:
   - `KILL_ON_JOB_CLOSE`
   - active-process limit from `pids`
   - Job memory limit
   - no breakaway
   - UI limits (desktop, handles, clipboard, global atoms)
7. Read the child's token SID, integrity and elevation type **before resume**.
   This is the persisted observed identity, with observation point
   `child_token_pre_resume`. Any mismatch kills the Job.
8. Resume the child. The shim reads a bounded JSON spec from stdin: executable
   or tool, argv, relative cwd and output quota. As the build account it opens
   the cwd beneath the workspace, refusing reparse points at every component. It
   then runs the target with `std::process::Command`: `env_clear` plus the
   config environment, stdin null, stdout and stderr inherited, and the absolute
   executable, so there is no PATH search. Rust std quoting and its batch-file
   argument refusal apply. Because the target inherits the token and the Job,
   the identity observed on the shim applies to it.
9. When the timeout expires, on cancel or at shutdown, call `TerminateJobObject`
   and require zero live Job processes, which sets `cleanup_confirmed`. If the
   service crashes, `KILL_ON_JOB_CLOSE` acts as the kernel backstop.

### Linux and macOS

These use ADR 0007's `Runner` unchanged in principle. The observed identity is
the child-reported identity after the ID drop and before exec, with observation
point `child_post_drop_pre_exec`.

Admin mode keeps exactly one supplementary group (`sudo`/`wheel` on Linux,
`admin` on macOS) and does not set NNP. The owner provisions a NOPASSWD sudoers
rule. Its *effect* is observed by the `tool-probe` result for `sudo -n true`
(`admin_effective`) rather than assumed.

macOS has no fence on the process tree. After each run, a child that has done
`setuid(build_uid)` calls `kill(-1, SIGKILL)`. The service then confirms through
`sysctl(KERN_PROC_UID)` that no build-UID process remains, which sets
`cleanup_confirmed`. The tree boundary is reported as `mac_uid_sweep`. This
needs a small ADR 0007 extension (a new `TreeBoundary` variant) and also kills
any of the build account's own interactive processes, which is acceptable for a
dedicated account and documented.

### Bounds and quarantine

Runs execute in `spawn_blocking`, joined on stop. MCP `notifications/cancelled`
maps to the run's `Cancellation`. An unconfirmed cleanup, or any executor error
after spawn, sets in-memory `quarantined`, which refuses further runs until
restart. The restart janitor then cleans up: cgroup leaves on Linux, a UID sweep
on macOS, and nothing on Windows because the Job dies with the process.

### Tool discovery

A fixed table lists `cargo`, `rustc`, `rustup`, `git`, `python3`/`python`,
`node`, `qemu-system-x86_64` and `qemu-img`. Running `arxd tool-probe` as the
build account, through the same executor path, resolves each name against the
configured `PATH` and runs `--version` with a 5 s timeout and a 4 KiB output
cap. It prints JSON lines `{name, found, version}`, where the version is at most
128 printable ASCII characters.

The service keeps the latest result **in memory** only, together with its
observed identity and timestamp. It is refreshed at start (daemon actor) and by
the `discover_tools` tool (agent actor). Absolute tool paths are not returned.
`run_command` accepts `tool: "<name>"` and re-resolves it inside the shim as the
build account.

## MCP surface (M2)

Every tool uses an exact JSON schema with `additionalProperties: false`. The
principal comes only from the authenticated transport: per request over HTTP,
and an immutable per-connection value on the local endpoint. No tool takes a
principal, identity, path override or config value.

- `list_hosts {}`: the current fields plus these:
  - `execution {available, reason, rights, service_hardening,
    service_state_protected}`. `service_state_protected` is `false` in admin
    mode, because admin builds can read service state; this is an honest
    limitation, not a fix.
  - `last_observed_identity`.
  - `tools [{name, found, version}]`, `tools_observed_at`.
- `run_command`: input `{tool | executable, args[≤128], cwd?, timeout_s
  1..=3600, output_bytes 2..=8192}`. ADR 0007 bounds the argv. `tool` and
  `executable` are mutually exclusive, and `executable` must be absolute. The
  result is:
  - `activity_id`, `outcome`, `exit_code`
  - `observed_identity {kind, id, gid?, integrity_rid?, elevation?,
    observation_point}`
  - `rights`, `tree_boundary`, `cleanup_confirmed`
  - `stdout`/`stderr` as `{head, tail, dropped_bytes}`. Head and tail are lossy
    UTF-8 in which every byte other than printable, TAB, LF and CR becomes
    U+FFFD. The worst case is therefore 3× the raw size with no JSON escapes,
    which bounds a whole response below 64 KiB. Output is untrusted text and is
    never persisted.
- `discover_tools {}`: runs discovery and returns the `tools` list.
- `list_activity {limit?: 1..=50 (default 20), before_id?: positive integer,
  include_reads?: bool (default false)}` (see below).

These are integration requirements on the local transport. PR15 corrections must
satisfy them; they do not mean this ADR reviewed PR15.

- A per-connection error must never terminate the listener.
- There is no absolute connection lifetime. An idle timeout of at most 15 min
  applies only while no request is in flight, so a Claude Code session and a
  3600 s run survive.
- Request frames stay at 8 KiB. Response frames may be up to 64 KiB.
- The bridge has the same idle rule on stdin.

## Activity schema v2 and audit semantics

Migration `user_version` 1 → 2 runs in one transaction with restart
reconciliation. Legacy rows keep their data and become `actor_kind='agent'`,
`class='read'`, with NULL timestamps and execution fields. If the migration
fails, nothing changes and the service exits with code 5.

| column | values |
| --- | --- |
| `id` | INTEGER PK AUTOINCREMENT (the only order and cursor) |
| `actor_kind` | `agent` \| `daemon` (`daemon` ⇒ principal `arxd`) |
| `principal` | validated principal (existing rules) |
| `tool` | `list_hosts` `list_activity` `unknown_tool` `run_command` `discover_tools` `service_start` `service_stop` |
| `class` | `read` (list_hosts, list_activity, rejected) \| `action` (run_command, discover_tools) \| `lifecycle` |
| `outcome` | `started` `succeeded` `failed` `rejected` `refused` `interrupted` |
| `reason` | NULL or fixed preflight/failure enum |
| `started_at_ms`, `finished_at_ms` | UTC wall clock; informational, may be non-monotonic |
| `program` | NULL, a discovery-table name, or `path` (absolute executable; the path itself is not stored) |
| `argc` | NULL or 0..=128 |
| `rights` | NULL \| `standard` \| `admin` |
| `observed_kind`, `observed_id`, `observed_gid`, `integrity_rid`, `elevation`, `observation_point`, `identity_matches_pin` | NULL or as observed; `observed_id` ≤ 184 chars, SID/decimal syntax checked |
| `stop_reason`, `exit_code`, `tree_boundary`, `cleanup_confirmed` | NULL or executor result |

Every text column has a `CHECK` against its fixed vocabulary or syntax. Rows
never hold argv, environment, cwd, paths, output, tool-call names supplied by
the caller, or error strings.

Ordering, extending the current store and PR18:

- `start` commits a `started` row before any dispatch, inventory read, query or
  spawn. If that commit fails, nothing runs.
- `finish(id, …)` is a single transaction that updates exactly one row where
  `outcome='started'`. Any other count rolls back and refuses, which is PR18's
  semantics. Execution fields are written in that same update.
- If `run_command` completion fails after the process ran, the caller gets an
  error with no result. The row stays `started`, and the next restart sets it to
  `interrupted`.
- Retention is two-class: keep the newest 1000 `action`+`lifecycle` rows and the
  newest 200 `read` rows. Pruning happens inside the `start` transaction.
  Authenticated read floods therefore cannot evict command records.
  Unauthenticated requests still create no rows.
- Page and size caps stay at 4096 × 1024 pages. The service rechecks the page
  count after the migration and refuses if the store is over the cap.

### `list_activity`

- **Visibility**: rows where `actor_kind='agent' AND principal = <authenticated
  principal>`. Daemon rows and other agents' rows are never returned. The owner
  view arrives with the M6 dashboard.
- **Audit recursion**: the call first commits its own `started` read row R.
  It then runs one keyset statement in the same connection, bounded above by R:

  ```sql
  SELECT <columns> FROM activity
   WHERE actor_kind = 'agent' AND principal = ?1
     AND id < ?2            -- ?2 = min(R, before_id) when before_id is supplied, else R
     AND (?3 = 1 OR class <> 'read')
   ORDER BY id DESC
   LIMIT ?4                 -- ?4 = limit + 1
  ```

  At most `limit` rows are returned. `next_before_id` is the id of the last
  returned row when row `limit + 1` exists, otherwise null. The call then
  finishes R as `succeeded`. If that finish fails, the results are withheld.
  The call never returns its own row R, and with the default filter it never
  returns other reads.
- **Output per row**: `id`, `tool`, `outcome`, `reason`, timestamps, `program`,
  `argc`, `rights`, the observed identity fields, `stop_reason`, `exit_code`,
  `tree_boundary` and `cleanup_confirmed`. `principal` is implied, so it is
  omitted. Each row serializes to at most about 600 bytes, so 50 rows fit in a
  64 KiB response.
- **Rejected arguments** (bad limit, unknown key): the call is recorded as
  `rejected` and returns no rows.

## Privilege boundary summary

- The service (SYSTEM or root) reads only fixed layout paths. It never opens,
  stats or canonicalizes the workspace or tool paths. The shim or ADR 0007's
  child does that after the identity switch.
- The build account cannot read config, tokens, hashes, state or activity, and
  cannot write the binary, layout or manager artifacts. In admin mode, the
  confidentiality of service state is lost, and inventory says so.
- The owner's client token sits in the owner's private file. The build account
  cannot connect to the endpoint (Unix permissions, Windows pipe DACL), and peer
  identity checks reject it anyway.
- M3 must not add a non-loopback listener to this privileged process without a
  new ADR. The preferred direction is an unprivileged network frontend with a
  local privileged broker.

## Build order for the implementer (Sol), one concern per PR

Each slice runs only meaningful tests that need no provisioning. None of them
creates accounts, services, rights or tasks on a developer machine or in default
CI.

1. **Lifecycle core and Unix entry.** `service::Host` state machine, `Shutdown`,
   exit codes, SIGTERM/SIGINT/SIGHUP handling, `sd_notify`, and `arxd service
   run` (on Unix it fails with code 7 when the fixed layout is absent). Tests:
   - transition table;
   - a signal leads to drain, `service_stop` and exit 0 against a `for_test`
     layout;
   - `sd_notify` frames received on a test `UnixDatagram`, including abstract
     names;
   - SIGHUP ignored;
   - a stuck fake run is bounded by the 10 s drain.
2. **Windows SCM entry.** Dispatcher, handler and status reporting. Tests:
   - pure control-code → event and status-sequence tests;
   - a real unprivileged call to `service run` outside SCM exits with code 10;
   - after `SetTokenInformation(TokenOwner = TokenUser)`, a newly created file's
     owner equals the user SID. This runs on admin-token hosted runners and
     reproduces the PR15 default-owner class.
3. **Fixed layout and install artifacts.** `Layout`, the files under
   `packaging/`, and `OsManager` with a recording fake. Tests:
   - embedded bytes equal the checked-in files;
   - artifacts contain no `$`, `%` or `{{` interpolation markers;
   - the argv vectors are exact;
   - squatted or mis-owned directories are refused;
   - `systemd-analyze verify` on an Ubuntu CI copy whose `ExecStart` alone is
     rewritten to the built binary;
   - `plutil -lint` on macOS CI.
4. **Service config and identity pinning.** `arxd-service/1`, `configure`
   (atomic serde write), `client-config new`, and the Windows LSA right
   add/remove behind a trait. Tests:
   - schema and bounds;
   - reserved principal;
   - client = build refused;
   - admin-group membership in standard mode refused, using fake resolvers;
   - atomic write with ownership on temp dirs;
   - generated tokens never reach stdout.
5. **Activity v2.** Migration, columns with CHECKs, two-class retention and
   `list_activity`. Tests:
   - v1 store with rows migrates losslessly;
   - failed migration leaves v1 intact;
   - keyset pages are exact at `limit`/`limit + 1` boundaries;
   - another principal's rows and daemon rows are invisible;
   - the call's own row is excluded;
   - read floods cannot evict actions;
   - PR18 duplicate or missing finish refuses;
   - secret-marker strings in args never appear in the DB file bytes.
6. **Executor trait and MCP tools.** `Executor { preflight, run }`, the
   unavailable implementation, `run_command`, `discover_tools` and the extended
   `list_hosts`, all over the local endpoint. Tests use a `#[cfg(test)]` fake
   executor that asserts its `started` row is committed before it is invoked,
   then:
   - store failure forbids the call;
   - completion failure withholds the result;
   - cancellation propagates;
   - `busy` and `quarantined` are refused;
   - the 64 KiB response bound holds for worst-case output.

   The fake cannot report a real identity kind. Its results are typed
   `FakeForTest`, so they can never satisfy acceptance.
7. **Unix executor integration** after PR14 merges: derive protected dirs and
   `cgroup_parent`, the janitor, the macOS UID sweep and admin group handling.
   Unprivileged tests run against the existing library. Privileged evidence
   comes only from the reviewed disposable experiment.
8. **Windows backend.** Port the probe mechanism to Rust, plus the `exec-child`
   and `tool-probe` shims and the private window station and desktop.
   Unprivileged tests:
   - raw `TokenGroups` classification over synthetic buffers;
   - environment block builder;
   - shim spec parsing and reparse refusal on temp junctions;
   - a real Job tree-kill of the current-user `exec-child`, which confirms zero
     live processes;
   - handle-list leakage: the child sees only the three handles.

   S4U itself requires the disposable experiment.
9. **Admin-rights verification** (both directions) on all OSes. Only the pure
   classification logic is tested; the real paths are unverified.
10. **Guarded disposable experiments**, in the pattern of PR12 and PR14. Install
    the service on a fresh hosted runner, configure a generated account, run
    `cargo --version` through `arxd mcp-stdio`, then uninstall. These need a
    separate review before any dispatch.

The coordinator may parallelize slices 1–2, 3–4 and 5–6. Slices 7 and 8 depend
on 6 and on their probe PRs.

## What can proceed now, and what is blocked

- **Can proceed now:** slices 1–6, the unprivileged parts of 7–9, and the guide.
- **Blocked on runtime evidence:**
  - Windows S4U feasibility, privilege-list sufficiency and the private desktop.
    The first experiment failed in an unidentified setup phase, and PR17
    diagnostics are unreviewed.
  - Linux privileged execution and delegation (the PR14 experiment has not run).
  - Any macOS privileged path.
- **Blocked on the owner:**
  - creating the `Arxcovo` standard account;
  - providing the tools (Rust) for it;
  - `service install` and `configure` on the development PC.
- **M2 acceptance (unchanged):** Claude Code, over `arxd mcp-stdio`, runs
  `list_hosts`, and then runs `run_command {tool:"cargo", args:["--version"]}`.
  The result and the persisted `list_activity` row must show the authenticated
  principal, `observed_kind=windows_sid` with the pinned build SID and
  `identity_matches_pin=1`, Medium integrity, `rights=standard`, and outcome
  `succeeded`. Hosted experiments prove software behaviour, not the owner's PC.

If S4U proves infeasible, that triggers a new ADR. The candidates are a
per-account service whose password the owner enters into the SCM, or an
owner-entered credential held in LSA private data. It is never an automatic
fallback, never a password in chat or config, and never the owner's identity.
