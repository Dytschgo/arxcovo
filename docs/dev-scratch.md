# Agent scratch on D:

Before running a command, set its scratch environment explicitly. Each delegated
task must carry the same requirement and its own D: paths; parent intent does not
configure a child process. Builds, worktrees, observer output and downloaded tool
data belong on D:. Existing owner caches remain untouched.

Use a task-specific root, for example:

```powershell
$ErrorActionPreference = 'Stop'
$taskRoot = 'D:/Temp/arx-TASK'
$env:TMP = "$taskRoot/tmp"
$env:TEMP = $env:TMP
$env:CARGO_TARGET_DIR = "$taskRoot/target"
$env:CARGO_HOME = "$taskRoot/cargo-home"
New-Item -ItemType Directory -Force -Path $env:TMP,$env:CARGO_TARGET_DIR,$env:CARGO_HOME | Out-Null
[pscustomobject]@{
    TMP = $env:TMP
    TEMP = $env:TEMP
    CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR
    CARGO_HOME = $env:CARGO_HOME
    CFreeBytes = (Get-PSDrive C).Free
}
```

Replace `TASK` with the task identifier. An existing task-owned D: Cargo directory
may be reused explicitly. Set any tool-specific scratch/output flags too; setting
`TMP` does not override an explicit observer path. When using npm or Playwright,
set `npm_config_cache` and `PLAYWRIGHT_BROWSERS_PATH` under the task root on D:.

Check the printed paths before launching the tool and record the output location
in the task evidence. Check C: free space again before a large build: below
20,000,000,000 bytes, pause large builds and report the measured space. Continue
small documentation, review or remote CI work that needs no large local build.

For cleanup, inventory only attributable agent outputs. Resolve absolute paths,
verify they remain within the intended root, and reject reparse points. Preserve
logs/evidence on D: with hash verification before deleting their C: copies. Keep
caches, user data, Lab Manager state, VM disks and backups intact. Record measured
volume space separately from logical bytes deleted; concurrent disk activity can
change free space independently.
