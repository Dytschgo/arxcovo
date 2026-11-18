# Windows execution account

Windows execution has two account choices. In a normal execution configuration,
omitting `mode` selects `current_user`: commands run as the non-elevated account
running the user-level Arxcovo daemon. This is the owner's requested default.

The mode shares that account's file and credential access. Windows Job limits,
timeouts, capture bounds and cleanup checks remain, but they do not isolate the
account from its own files. Service identities, session 0, elevation and enabled
administrator tokens are refused. A LocalSystem service cannot become this default.

Add `execution` to an otherwise valid private foreground configuration:

```json
"execution": {
  "workspace": "<existing absolute workspace>",
  "environment": {
    "PATH": "<existing absolute tool directories separated by semicolons>",
    "SystemRoot": "<Windows directory>",
    "TEMP": "<task temporary directory>",
    "TMP": "<task temporary directory>",
    "CARGO_HOME": "<Cargo home>",
    "CARGO_TARGET_DIR": "<build output directory>",
    "RUSTUP_HOME": "<installed Rust toolchain home>"
  },
  "limits": {"memory_bytes": 2147483648, "pids": 32}
}
```

`"mode":"current_user"` may be supplied explicitly. No acknowledgement field is
required for this normal mode. Keep configuration, client credentials and private
state outside the workspace. Execution requires authenticated local IPC; omit the
HTTP listener. Existing configurations with no execution block remain inventory-only.
The old `development_execution` block remains supported, with its original labels
and acknowledgement; configuring both forms is rejected.

For the optional account choice, set `"mode":"dedicated_account"` and
`"account":"Arxcovo"` in the execution block. The account is a bounded local
account name; this setting does not collect a password or create an account.
The dedicated executor currently reports `dedicated_account_setup_required` and
does not run commands. Selecting it never falls back to the current account.
Its protected-service setup and native qualification remain required; the
current-user default does not bypass those gates.

Start the user-level daemon with `arxd --config <private-daemon-config>`. Its MCP
stdio bridge is `arxd mcp-stdio --client-config <private-client-config>`. For this
mode, the client must pin the daemon's actual user identity, not the LocalSystem
identity used by the separate installed-service credential flow. Credentials
remain private and are never command-line tokens or committed examples.

Verify the account choice through `list_hosts`, then `run_command` with
`{"tool":"cargo","args":["--version"]}`, and `list_activity` using the returned
activity ID. Normal runs identify as `current_user`, dedicated false, isolation
false, and report actual current-user match and cleanup. Historical development
and dedicated activity retain their provenance. Restart persistence is a separate
check; a successful compile alone is not a completed owner journey.

For the owner's test PC, all workspace, state, scratch and build output stay on
D:. Owner-admin, Lab Manager, VM/static-data and privileged native-test fences
remain unchanged. Dedicated-account and cross-platform acceptance are not implied
by current-user execution. See [ADR0029](adr/0029-owner-selectable-windows-execution-account.md).
