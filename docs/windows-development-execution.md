# Windows development execution candidate

This candidate (Sol start, Opus continuation) passes its local unit, policy
fixture and transport tests and needs a fresh independent Sol review before the
coordinator's owner acceptance. The original M2 dedicated-account/service
requirement remains open. The positive current-owner production path is
**unverified on real hardware**: tests use a unit-test-only policy fixture that
skips the token *policy* (hosted runners are elevated). The fixture keeps the
Job, the pre-resume child TokenUser comparison, the reparse checks and the bounds.

It depends on the PR16 `checked_path` change (17b6bc9 prefix skip plus 8c3856a
physical-root requirement). The same lines are carried here, so merging either
first is a textual no-op for that function.

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
