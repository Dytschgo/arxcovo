# Windows development execution candidate

This candidate is incomplete, has a failing native fixture test, and requires
other-provider continuation and independent review before owner acceptance.
The original M2 dedicated-account/service requirement remains open. The
positive current-owner path is **unverified on real hardware**.

Commands in this mode share the owner's files, credentials and network access.
The bounds and Job controls do not create security isolation. Activity is not
tamper-evident against commands running as the owner. Redaction is best effort.

## Private configuration for acceptance after review

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

After review and completion, use the existing Claude Code MCP stdio bridge
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
Do not store raw output or owner paths/identity. This sequence has not been run
by the implementation agent.

## Current implementation limits and continuation

Only fixed tool-table `.exe` names are supported. Arguments are bounded to 128;
timeouts to 1–3600 seconds; retained output to 2–8192 bytes per stream with a
further JSON budget to fit the 64 KiB bridge response. No shell resolution,
whole-parent environment inheritance or product approval flow is introduced.

The failing junction fixture and missing execution-specific transport/fault
coverage are recorded with reproduction and three failed fixes in PROGRESS.md.
Do not treat passing inventory transport tests or the unit-only policy fixture
as evidence of production dedicated-account execution or owner acceptance.
