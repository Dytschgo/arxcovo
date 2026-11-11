# 0010: Current-user Windows development execution

Status: owner-authorized temporary development deviation, 2026-11-11.
Contract settled (Opus 5.5 high, architecture). Docs only: nothing here is
implemented, and no acceptance is claimed.

The owner authorized using the existing Windows user for the development test
after the dedicated-account experiment failed, to avoid spending more time on
that blocker. The coordinator requested cancellation of further S4U probe
diagnosis. No third privileged experiment is planned; its dispatch gate is off.

The development path runs only as the current unelevated user, without account
switching, elevation, password collection, service installation or changes to
account rights. It requires an explicit owner-controlled development setting;
an agent cannot select it or turn it on through MCP arguments. It is never an
automatic fallback from a failed dedicated-account execution request.

Inventory, command results and activity must identify the development execution
mode honestly. They must not describe this user as a dedicated standard account
or claim isolation from the owner's files and credentials. Private owner names,
SIDs, home paths and credentials must not enter committed evidence or MCP output.

The production service and dedicated-account design remain in ADR 0009, which
stays unavailable until the owner configures it. This deviation unblocks
development verification. It does not prove the M2 dedicated-account
requirement or authorize owner-PC administration.

## Accepted risk (explicit, not mitigated)

Commands run as the owner and share its file, network and credential access:
the foreground config (token hashes), activity database, client token file, the
arxd process, and the pipe (reachable with a stolen token). Activity is not
tamper-evident against executed commands. The controls below bound arguments,
environment, cwd, process tree, time and output; they are not a security
boundary, and no output may call them one.

## Implementation contract

**Enablement.** Only the private foreground config of ADR 0008 can enable the
mode. It is loaded once from the owner-owned file, with PR15's strict owner
checks unchanged, and has no live reload. It gets one optional key, parsed with
`deny_unknown_fields`:

```json
"development_execution": {
  "mode": "development_current_user",
  "acknowledge": "shares_owner_user_files_and_credentials",
  "workspace": "<absolute local path>",
  "environment": {"PATH": "...", "SystemRoot": "...", "TEMP": "...", "TMP": "...",
                  "CARGO_HOME": "...", "RUSTUP_HOME": "..."},
  "limits": {"memory_bytes": 8589934592, "pids": 256}
}
```

- `mode` and `acknowledge` are single-variant enums; any other value is exit 3.
- Environment keys: ADR 0009's Windows allowlist (≤16, ≤4096 bytes, no NUL);
  `PATH` entries absolute and local. The workspace exists, is local and
  reparse-free, and is disjoint from config, state and client-config directories.
- Load fails on non-Windows, with `listen` (HTTP) set, or in service mode
  (`arxd-service/1` has no such key). Concurrency is fixed at 1. Absent key:
  `available=false`, `reason=not_configured`.

The executor is chosen once at startup, never per request: service config
selects the ADR 0009 dedicated executor; this key selects the development
executor; otherwise there is none. Preflight failure never switches to another
executor. No MCP argument names a mode, identity or config value.

**Identity verification (internal).** Preflight runs at startup and before every
run on the arxd process token. It refuses `TokenUser` S-1-5-18/19/20
(`service_identity`), nonzero `TokenIsElevated` (`elevated_identity`), a
mandatory label other than exactly `0x2000` (`integrity_not_medium`), and an
Administrators S-1-5-32-544 group without deny-only (`admin_group_enabled`, which
also catches UAC-off admin tokens).

The child is then checked before it resumes, with the same rules, and its
`TokenUser` must equal the daemon's `TokenUser`. On any mismatch the run calls
`TerminateJobObject` and records `refused` / `identity_mismatch`. The SID is
compared in memory only and is never logged, persisted, returned or put in an
error.

**Spawn (reuse ADR 0009 Windows steps 4–9 without the logon, AsUser or desktop
steps).** No `CommandJob` type exists on the inspected refs; port the probe's
Job code (`tools/windows-identity-probe/Native.cs`) to `windows-sys`, adding
only needed features.

- `CreateProcessW` with `lpApplicationName` = resolved absolute `.exe`, an
  MSVC-rules command-line quoter, `CREATE_SUSPENDED | CREATE_NO_WINDOW |
  CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT`, a sorted
  config-only environment block, and a handle list of exactly stdin (NUL),
  stdout and stderr.
- Unnamed Job assigned before resume: `KILL_ON_JOB_CLOSE`, `pids` process
  limit, Job memory limit, `DIE_ON_UNHANDLED_EXCEPTION`, no breakaway, all UI
  limits. Timeout, cancel, shutdown and main-process exit call
  `TerminateJobObject`; `cleanup_confirmed` needs `ActiveProcesses == 0`.
  Unconfirmed cleanup or any post-spawn error quarantines until restart.
- Reuse ADR 0007's `RunSpec`, `Cancellation` and `CapturedOutput`; if not on
  main, land those types alone, unchanged.

**Arguments.** Development mode accepts `tool` only, from ADR 0009's table.
`executable` is rejected (`invalid_arguments`). `args` ≤128 with no NUL (ADR 0007 bounds); `timeout_s` 1..=3600 and
`output_bytes` 2..=8192. `cwd` is relative under the workspace with no `..`,
drive, UNC or `:`; each component is opened with `FILE_FLAG_OPEN_REPARSE_POINT`
and any reparse attribute refuses the run.

**Tool discovery.** arxd resolves names itself against the configured `PATH`
only, as `<dir>\<name>.exe`. It accepts only reparse-free regular files: no
`.bat`, `.cmd` or `PATHEXT` search, and no inherited `PATH`. It runs
`--version` through the same executor (5 s, 4 KiB) and returns
`{name, found, version}` without paths.

**Output hygiene.** Returned output replaces (case-insensitive, either slash)
the workspace, `CARGO_HOME`, `RUSTUP_HOME`, `TEMP`/`TMP` and the token's profile
directory with `<workspace>`, `<cargo_home>`, `<rustup_home>`, `<temp>`,
`<profile>`: best effort, not a guarantee. arxd errors are fixed category text.
Output is never persisted.

**Activity and MCP.** Add the column `identity_mode` with values
`dedicated_account` and `development_current_user`. Fold it into v2 if that is
unmerged; otherwise add a 2→3 migration with ADR 0009's transaction rules, where
legacy rows stay NULL.

- A `CHECK` forces `observed_kind`, `observed_id` and `rights` NULL in
  development rows. `identity_matches_pin` means *child TokenUser equals the
  daemon's current user*; rows also store `integrity_rid`, `elevation=0`,
  `observation_point='pre_exec'`, `tree_boundary='windows_job'`.
- Audit order unchanged: `started` commits before resolution or spawn (failure
  runs nothing); PR18-conditional `finish`; failed completion withholds the
  result; post-start rejections finish `refused` with a fixed reason.
- `list_hosts.execution` reports `{available, reason, mode:
  "development_current_user", isolation: "none_shares_owner_user", rights: null,
  matches_current_user, integrity: "medium", elevated: false}`.
- `run_command` results, `list_activity` rows and every test or evidence record
  carry `execution_mode` and `claim: "development_only_not_dedicated_account"`.
  None of them contains a SID, username or path.

**Tests.** Token-policy tests on injected token facts (every refusal); quoter
round trip through `CommandLineToArgvW`; environment block; config rejections
(wrong or missing `acknowledge`, unknown key, HTTP, non-Windows, service mode);
MCP schema rejection of mode or identity fields; store `CHECK` rejection of a
SID or rights in development rows; redaction. On hosted Windows, preflight must
refuse a token that an independent query reports elevated. Job, timeout,
kill-tree, handle list, cwd junction and output run through a test-only
policy-fixture constructor the binary cannot reach. The positive path is
**unverified on real hardware** until the owner-PC run below.

**Acceptance (development only).** From Claude Code over the authenticated local
MCP endpoint: `list_hosts`, then `run_command {tool: "cargo", args:
["--version"]}`, then `list_activity`, which shows the persisted row with
`identity_mode=development_current_user`. M2's "as the Arxcovo account"
criterion stays open in docs/PROGRESS.md.
