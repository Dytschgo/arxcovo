# Windows development execution

PR #22 merged as `8fafbe70451807cf419c8e47706259b24a49cf68` after independent
Sol review at `59293a8` and all three hosted platforms and status gates passed.
The private production-path acceptance below was then exercised successfully on
the existing Windows host with a non-test binary. It used source `72a5e5f`, whose
production code, Cargo manifests/lock and toolchain are byte-identical to the
reviewed integrated head. The build hash and sanitized proof are in
[evidence/windows-development-acceptance.json](evidence/windows-development-acceptance.json).

Claude Code listed the host, ran `cargo --version` through local MCP and read its
successful activity row. After restarting the daemon, a fresh Claude Code session
read the same row. Independent database checks confirmed the development mode,
Medium integrity, current-user match, null raw identity/rights fields and Job
cleanup. The harness stopped its processes, a native process query found no
remaining acceptance arxd instance, and all temporary configuration/credential
files were removed. No raw CLI stream, private paths, token or owner SID is saved
in the public evidence.

This verifies the explicitly authorized current-user development path only.
Original M2 dedicated-account and installed-service acceptance remain open.
S4U, cross-account service execution and all VM/hypervisor paths remain
**unverified on real hardware**. The PR16 path correction is merged.

Commands in this mode share the owner's files, credentials and network access.
The bounds and Job controls do not create security isolation. Activity is not
tamper-evident against commands running as the owner. Redaction is best effort.

## Private configuration for repeating development acceptance

The coordinator is already authorized to provision this setting without asking
again. Do not commit the configuration, token, account identity, tool paths or
raw output. Use the existing foreground local endpoint configuration and strict
owner-only ACL/peer checks. Keep config, client-config and state directories
outside the workspace; place the client config alongside the private daemon
config so the config-directory disjointness check covers both. There is no
server field declaring separately located client-config directories, so those
must be kept outside the workspace by the coordinator.

Add this key to the private daemon JSON, replacing placeholders locally:

```json
"development_execution": {
  "mode": "development_current_user",
  "acknowledge": "shares_owner_user_files_and_credentials",
  "workspace": "<absolute local existing reparse-free directory>",
  "environment": {
    "PATH": "<absolute local existing tool directories separated by semicolons>",
    "SystemRoot": "<Windows directory>",
    "TEMP": "<private temporary directory>",
    "TMP": "<private temporary directory>",
    "CARGO_HOME": "<existing cargo home>",
    "RUSTUP_HOME": "<existing rustup home>"
  },
  "limits": {"memory_bytes": 8589934592, "pids": 256}
}
```

Omit `listen`; HTTP is forbidden in this mode. Run the foreground daemon from
an unelevated Medium-integrity token, using its existing `--config` argument.
The development key is not accepted in service configuration and no MCP argument
can enable or select this mode. There is no automatic fallback. A startup config
or token refusal exits 3 with fixed `server-config` text.

For a reviewed build, use the Claude Code MCP stdio bridge
`arxd mcp-stdio --client-config <private-client-config>` and call:

1. `list_hosts {}`: expect execution mode `development_current_user`, isolation
   `none_shares_owner_user`, null rights and dedicated false.
2. `run_command {"tool":"cargo","args":["--version"]}`: expect successful
   exit, current-user match, confirmed cleanup, execution mode and claim
   `development_only_not_dedicated_account`.
3. `list_activity {}`: expect the caller's persisted run row with identity_mode
   `development_current_user`, no observed SID/kind/rights, pre_exec observation,
   integrity 8192, elevation false, windows_job boundary and confirmed cleanup.
4. Restart the daemon and read the same principal's history to confirm durability.

Retain only sanitized booleans, fixed labels and outcomes as acceptance evidence.
Do not store raw output or owner paths/identity. The coordinator exercised all
four calls after review and green hosted checks. The harness checked host
execution mode but did not assert the host response's isolation or null rights.
It matched the activity row's ID, tool and outcome over MCP; the identity mode,
Medium integrity, current-user match, null raw identity/rights and cleanup fields
were asserted directly in SQLite. The implementation agent did not perform the
private acceptance.

## Current implementation limits

- Only fixed tool-table `.exe` names are supported, resolved from the configured
  `PATH` only. Arguments are bounded to 128, timeouts to 1–3600 seconds, and
  returned output to 2–8192 bytes per stream, with a further JSON budget so the
  result fits the 64 KiB bridge response. Results report
  `stdout_dropped_bytes`/`stderr_dropped_bytes` (bytes read past the limit;
  reading stops once the Job is terminated, so this is a lower bound).
- Concurrency is one run. A second request is refused `busy`. A post-spawn error
  (identity mismatch, unconfirmed cleanup, pipe or wait failure) quarantines the
  executor until restart. Pre-spawn refusals (arguments, reparse, missing tool)
  never quarantine.
- Redaction is best effort. It replaces the workspace, `CARGO_HOME`,
  `RUSTUP_HOME`, `TEMP`/`TMP`, the token profile directory, `USERPROFILE` and
  `USERNAME` (case-insensitive, either slash), plus SID-shaped runs. It works on
  up to 4 KiB of raw output beyond the limit before cutting. A path a command
  prints in another form (8.3 short names, encoded or split output) can still
  appear.
- Reparse checks inspect each component by name (`checked_path`) and again
  through the pinned handle. Both run before the suspended child is created. The
  pins deny write/delete sharing until the run ends. This is not isolation: the
  child can still reach anything the owner can.
- The service-config rejection named in ADR 0010 is not applicable on this
  branch, because no service configuration exists here yet. When the lifecycle
  candidate lands, its service schema must not accept `development_execution`.
- When execution is configured, every audited request (including `list_hosts`)
  starts with `identity_mode=development_current_user`. Completion cannot relabel
  a row.

Do not treat passing inventory transport tests or the unit-only policy fixture
as evidence of production dedicated-account execution or owner acceptance.
