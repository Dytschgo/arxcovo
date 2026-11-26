# M2 owner-host acceptance

Acceptance is an MCP session from the owner that lists the host and runs
`cargo --version` under the selected execution identity. The action is in the
feed, and cleanup is truthful. The 2026-11-25 `current_user` check on the
owner's PC passed and is recorded in PROGRESS. That run's generated client
file launched `arxd mcp-stdio` directly, so the relay did not record
`m2-mcp.jsonl` or the derived fields. This runbook is the corrected client
route. A fresh owner run of this route is **unverified on real hardware**.

Paths below are the absolute paths supplied for the run. Quote them. The
prepare script writes those paths into `.mcp.json`. It does not embed a
credential. Keep the hash-pinned 2026-11-25 tree as the record of that run.
Point `-ArxRelay` at this repository's `tools/acceptance/m2/acceptance-stdio.py`.

Set `TMP`, `TEMP`, `CARGO_HOME`, and `CARGO_TARGET_DIR` in the same PowerShell 7
window before preparing the session. The window is unelevated.

## Client configuration

`tools/acceptance/m2/prepare-owner.ps1` writes one stdio server. The command is
the installed Python executable (3.11 or newer). The arguments are `-I`,
`-B`, the relay, and the owner-supplied arxd, client, and evidence paths:

```json
{
  "mcpServers": {
    "arxcovo-owner": {
      "type": "stdio",
      "command": "C:/Path/To/python.exe",
      "args": [
        "-I",
        "-B",
        "D:/Code/Arxcovo/tools/acceptance/m2/acceptance-stdio.py",
        "--arxd",
        "D:/acceptance/bin/arxd.exe",
        "--client-config",
        "D:/acceptance/private/client.json",
        "--evidence",
        "D:/acceptance/evidence/m2-mcp.jsonl"
      ]
    }
  }
}
```

The relay then launches `arxd mcp-stdio --client-config` with the client file
from those arguments, creates a fresh evidence file, and appends sanitized
results. Claude receives only the allowlisted fields from the acceptance
tools. The derived fields are `inventory_observed`, `rights_is_null`, and
`raw_identity_and_rights_are_null`.

## Prepare and connect

```powershell
& 'D:/Code/Arxcovo/tools/acceptance/m2/prepare-owner.ps1' `
  -ArxExe 'D:/acceptance/bin/arxd.exe' `
  -ArxRelay 'D:/Code/Arxcovo/tools/acceptance/m2/acceptance-stdio.py' `
  -ArxSession 'D:/acceptance/session' `
  -ArxDist 'D:/acceptance/ui/dist'
```

Expected: `Private owner session and M2/M6 configs created.` The script creates
an owner-only session, pins the named pipe to the current Windows SID, and
writes `m2-daemon.json`, `m6-daemon.json`, and `claude-work/.mcp.json`. It
prints none of the key material.

Start the foreground daemon and leave it running:

```powershell
& 'D:/acceptance/bin/arxd.exe' --config 'D:/acceptance/session/private/m2-daemon.json'
```

In a second unelevated PowerShell 7 window, use a fresh Claude profile and the
task's client file. Set `$claude` to the absolute path of the selected Claude
Code executable. Remove any ambient API-key or alternate-provider
variables for that window first. Keep `--strict-mcp-config` and `--tools ''`:

```powershell
$claude = 'C:/Path/To/claude.exe'
$env:CLAUDE_CONFIG_DIR = 'D:/acceptance/session/claude-config'
Set-Location -LiteralPath 'D:/acceptance/session/claude-work'
& $claude --strict-mcp-config --mcp-config 'D:/acceptance/session/claude-work/.mcp.json' --tools '' --permission-mode default --no-chrome
```

Accept workspace trust only for the session's `claude-work` directory. `/mcp`
must show `arxcovo-owner` connected. No persistent `claude mcp add`
registration is required.

Paste this prompt:

```text
Use only the arxcovo-owner MCP server for this acceptance check.
1. Call list_hosts with {}. Require exactly one Windows host with inventory_observed true and execution available=true, mode=current_user, matches_current_user=true, integrity=medium, elevated=false, dedicated=false, isolation=false, and rights_is_null=true. Stop if it differs.
2. Call run_command exactly once with {"tool":"cargo","args":["--version"],"timeout_s":10,"output_bytes":4096}. Require a cargo_version line, exit_code=0, stop_reason=exited, execution_mode=current_user, matches_current_user=true and cleanup_confirmed=true.
3. Call list_activity with {"limit":20}. Find this run_command row for cargo. Use its integer id to call list_activity again with {"activity_id":ID}. Require exactly that row, outcome=succeeded, identity_mode=current_user, program=cargo, argc=1, exit_code=0, integrity_rid=8192, elevation=false, observation_point=pre_exec, tree_boundary=windows_job, stop_reason=exited, identity_matches_pin=true, cleanup_confirmed=true and raw_identity_and_rights_are_null=true.
Report the cargo version, row ID, observed fields and any failed check. Use no shell, other tools, discovery, files or alternate execution.
```

Allow each of these MCP calls if Claude asks. Stop on any other call.

## Evidence

Exit Claude with `/exit`. Read the sanitized exchange:

```powershell
Get-Content -LiteralPath 'D:/acceptance/session/evidence/m2-mcp.jsonl' -TotalCount 12
```

Expected: four successful exchanges, including the page and the exact lookup
of the same action id. Treat a relay failure as a failed run even if the JSONL
contains successful exchanges. Use a fresh session directory for another run.
Keep the JSONL, UTC time, commit, binary SHA-256, Cargo version, and row id.
Do not export the Claude transcript, client config, auth output, or SQLite
database.

Stop the daemon with Ctrl+C, then check the stored row. `$rowId` is the
confirmed numeric id. Set `$python` to the absolute Python executable path
used in the generated `.mcp.json`:

```powershell
$python = 'C:/Path/To/python.exe'
& $python -I -B 'D:/Code/Arxcovo/tools/acceptance/m2/acceptance-stdio.py' --verify $rowId --database 'D:/acceptance/session/state/activity.sqlite3'
```

Expected: `PASS durable M2 row <ID>`. The verifier reads a snapshot of the
stopped database and requires that its journal sidecars are absent. Local
discovery, used before the dashboard terminal, takes the same relay and the
owner-supplied arxd and client paths:

```powershell
& $python -I -B 'D:/Code/Arxcovo/tools/acceptance/m2/acceptance-stdio.py' --discover --arxd 'D:/acceptance/bin/arxd.exe' --client-config 'D:/acceptance/session/private/client.json'
```

Expected: `Local discovery succeeded. Cargo found under current_user.`

The dashboard journey remains [M6 live-backend dashboard acceptance](m6-acceptance.md).
The localhost M6 check recorded in PROGRESS stands. Deployed HTTPS, native
execution beyond `current_user`, and this corrected relay route stay
**unverified on real hardware**.
