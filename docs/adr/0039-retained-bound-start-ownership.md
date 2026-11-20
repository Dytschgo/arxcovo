# ADR0039: Retained bound-start ownership

Date: 2026-11-20. Source-only step 5a; live acceptance remains step 5b.

Base: e9e44c912e1fa7c01fbd72428077ee46225951ed. Owner authority is
limited to the reviewed inspection-to-start transaction and outer custody.
Design SHA256 a357af504ff0a66f94daeb1871e045e8980b35e950a5eab81c0d34ee035f767f;
independent design review SHA256
00845c8668d139d2ca5fd8baedadc6ab5fe5e63b911b89ef9d5c275f7c6e4b81.

A new operational transaction spans one irreversible Begun period: authenticated
recovery identity and readonly validation, retirement of readonly connections and
leases under the original outer lock, writable acquisition with identity checks,
snapshot/membership comparison and unchanged preflight on the exact pair.
Existing offline preparation reuses factored authentication and inspection bodies.
Legacy terminal Inspect and existing start ordering remain unchanged.

Owned slots preserve partial resources and first failure after caught ordinary
unwind. Transfer validates completeness before infallible field moves; successful
Inspect, partial, failed and spent bundles cannot transfer. The unchanged adapters
receive the original SQLite connections and store leases.

The private bound-start sibling creates a leaked, process-retained resource cell
before preparation. The observer owns a watch receiver only. A driver joins real
blocking preparation and supervised startup. EngineMayExist is sticky before
Engine polling; returned nodes enter the retained cell before notification.
Unconfirmed startup retains strong adapter ownership and the original Owner.
The module-private original-Owner composition loan requires Running, an actual
retained node, no close request and original identity checks; no publication
capability or integration is exposed.

Close is latched before dispatch. Observers cannot abort its execution or dispatch
shutdown again. The actual node stays in an asynchronous mutex in the process
anchor while the single supervised close task polls unchanged shutdown. Extra
startup adapter clones are dropped first. Failure/unwind/lost execution never
becomes clean through an absent node or release flags. First close failure remains
retained. Successful close requires actual-node shutdown success, joined startup
and store release evidence. Deliberate pre-Engine disposal needs no Engine join.
Uncertain custody has no production disposal operation. The small result anchor
remains even after clean close; this deliberate process-lifetime allocation trades
availability/memory for retention and supports repeated observation.

Ordinary tests exercise real production preparation/transfer, SQLite failures,
unwind, real locks, cross-process probes, synchronization and the outer close
policy. The cfg(test) driver boundary returns before the Engine block on every
platform; releasing test barriers cannot continue into Engine. Policy tests are
not Engine acceptance. No service activation, Found, issuance, transport,
publication, Engine/shutdown/Owners repair, tuning or workflow change.

Local evidence is in the bounded builder report on D:, linked by the brief.
Default hosted CI remains unchanged and may execute its existing Engine/founder
inventory. First hosted CI costs are pending; no rerun-to-green is authorized.
Engine Sol 3/3 exhausted; historical Opus 0/2 unchanged. New-source budget is
separate. Runtime step 5b, publication composition and hardware acceptance remain
open. Native/multi-host paths are **unverified on real hardware**.

## Step 5b source authorization amendment (2026-11-20)

The owner granted a source-only extension to 5a for one ignored Windows library
case and its fixed launcher. “Running the observation is a SEPARATE owner
decision later on the exact reviewed head.” The sole witness constructor call
follows an exact argv check and bounded pipe read inside that case. The witness
can pass only its own job through the test-build stop. The production driver
awaits the same Engine call directly. An ordinary test still returns at
Prepared. Existing CI selection and quarantine apply as before.

The build and independent exact-head review are routed to separate Sol 6.2/high
agents under the November 20 assignment. The coordinator owns review,
required checks and merge. Sol Engine 3/3 remains exhausted and historical
Opus 0/2 remains recorded. Step 5b stays open and **unverified on real hardware**.
Sources are the design digest
0263040725758c661cf821ad0c29cbd4eb9baec7428952bd8c2860afdf78cd52 and independent
design review digest
49ff208d8e988e7b9b08cd5cef4e4d20dbfdbc7bb2288502563493f6e8987460.

### Qualified entry for a later owner-authorized run

The coordinator stages the exact reviewed source, the checked test executable at
`bin/arxd-step5b.exe`, and the provenance files in the fixed observation root.
`source-manifest.json` covers the immutable source tree at the reviewed commit.
`loader.json` records the qualified toolchain, Windows loader, and absolute
CPython installation and its runtime dependencies. Stage `pins.json` with keys
`head`, `test`, `binary_bytes`, `binary_sha256`, `python` (native absolute
interpreter path), `python_sha256` and `sha256`. The latter maps `launcher`,
`support`, `manifest`, `loader`, `lockfile`, `driver` and `case` to the hashes for
the fixed paths in the launcher. The coordinator verifies the source, loader and
binary qualification and records the trusted pins digest and interpreter path
with the owner grant. All observation inputs and directories must already be
private and staged, with the fixture ACL precondition satisfied under `tmp`.
Qualification includes the shell runtime and supports Windows PowerShell 7 on
.NET through version 10, where `Environment.TickCount64` uses the same Win32
clock as the launcher. See the
[clock compatibility record](https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/11/environment-tickcount-windows-behavior).

Use this block only with that separate one-shot grant. Fill the two values from
the grant before entry. The two exclusive files mark entry and launcher
admission. A refusal at any point spends the authorization. Verified read
handles inhibit writes and replacement through use. CPython runs with isolated
imports, site initialization disabled and an explicit environment.

```powershell
& {
    $entry = [Environment]::TickCount64
    $ErrorActionPreference = 'Stop'
    $root = 'D:\Temp\arx-m3-step5b\observation'
    $qualifiedPython = '<qualified absolute interpreter from the grant>'
    $trustedDigest = '<trusted pins SHA256 from the grant>'
    $files = [Collections.Generic.List[IO.FileStream]]::new()
    $process = $null
    try {
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $allowed = @($sid, 'S-1-5-18', 'S-1-5-32-544')
        function Assert-Path([string]$path, [bool]$directory) {
            if (![IO.Path]::IsPathFullyQualified($path) -or
                [IO.Path]::GetFullPath($path) -ne $path) { throw 'Path' }
            $item = Get-Item -LiteralPath $path -Force
            if ($item.FullName -ne $path -or $item.PSIsContainer -ne $directory -or
                ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -or
                (!$directory -and $item.LinkType)) { throw 'Path' }
            if ($path -eq $root -or $path.StartsWith("$root\", 'OrdinalIgnoreCase')) {
                $acl = Get-Acl -LiteralPath $path
                $rules = $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])
                if (!$acl.AreAccessRulesCanonical -or $rules.Count -eq 0) { throw 'ACL' }
                foreach ($rule in $rules) {
                    if ($rule.AccessControlType -eq 'Allow' -and
                        $rule.IdentityReference.Value -notin $allowed) { throw 'ACL' }
                }
            }
            $parent = [IO.Path]::GetDirectoryName($path)
            if ($parent) { Assert-Path $parent $true }
        }
        function Hold-Input([string]$path, [long]$cap) {
            Assert-Path $path $false
            $stream = [IO.File]::Open($path, 'Open', 'Read', 'Read')
            $files.Add($stream)
            Assert-Path $path $false
            if ($stream.Length -le 0 -or $stream.Length -gt $cap) { throw 'Size' }
            return $stream
        }
        function Get-Digest([IO.Stream]$stream) {
            return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
        }

        Assert-Path $root $true
        [IO.File]::Open("$root\entry-claimed", 'CreateNew', 'Write', 'None').Dispose()
        if ([Environment]::Version.Major -gt 10 -or $trustedDigest -cnotmatch '^[0-9a-f]{64}$') {
            throw 'Qualification'
        }
        foreach ($name in [Environment]::GetEnvironmentVariables().Keys) {
            if ($name -match '^(LD_|DYLD_|LIBSQLITE3_|SQLITE3_|ARX_STEP5B_)') { throw 'Overrides' }
        }

        $pinsStream = Hold-Input "$root\pins.json" 16384
        if ((Get-Digest $pinsStream) -cne $trustedDigest) { throw 'Provenance' }
        $pinsStream.Position = 0
        $bytes = [byte[]]::new([int]$pinsStream.Length)
        $pinsStream.ReadExactly($bytes, 0, $bytes.Length)
        $pins = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json -AsHashtable -Depth 8
        if ($pins.python -cne $qualifiedPython) { throw 'Interpreter' }
        $pythonStream = Hold-Input $qualifiedPython 32MB
        if ((Get-Digest $pythonStream) -cne $pins.python_sha256) { throw 'Interpreter' }
        $launcher = "$root\source\tools\ci\observe_bound_start.py"
        $support = "$root\source\tools\ci\windows_diag_sample.py"
        $launcherStream = Hold-Input $launcher 128KB
        $supportStream = Hold-Input $support 128KB
        if ((Get-Digest $launcherStream) -cne $pins.sha256.launcher -or
            (Get-Digest $supportStream) -cne $pins.sha256.support) { throw 'Source' }

        $scratch = @{
            TMP = 'tmp'; TEMP = 'tmp'; CARGO_TARGET_DIR = 'target'; CARGO_HOME = 'cargo-home'
            RUSTUP_HOME = 'rustup-home'; ARX_OBSERVER_ROOT = 'evidence'
        }
        foreach ($leaf in $scratch.Values) { Assert-Path "$root\$leaf" $true }
        foreach ($file in 'source-manifest.json','loader.json','source\Cargo.lock',
            'bin\arxd-step5b.exe','source\crates\arxd\src\cluster\consensus\bound_start.rs',
            'source\crates\arxd\src\cluster\consensus\bound_start\nominal_observation.rs') {
            Assert-Path "$root\$file" $false
        }
        $windows = [IO.Directory]::GetParent([Environment]::SystemDirectory).FullName
        $launch = [Diagnostics.ProcessStartInfo]::new($qualifiedPython)
        $launch.UseShellExecute = $false
        $launch.CreateNoWindow = $true
        $launch.WorkingDirectory = $root
        $launch.RedirectStandardInput = $true
        $launch.RedirectStandardOutput = $true
        $launch.RedirectStandardError = $true
        $launch.Environment.Clear()
        foreach ($key in $scratch.Keys) { $launch.Environment[$key] = "$root\$($scratch[$key])" }
        $launch.Environment['SYSTEMROOT'] = $windows
        $launch.Environment['WINDIR'] = $windows
        $launch.Environment['ARX_SCRATCH_ROOT'] = $root
        $launch.Environment['CARGO_NET_OFFLINE'] = 'true'
        $launch.Environment['CARGO_PROFILE_DEV_DEBUG'] = '0'
        $launch.Environment['ARX_STEP5B_ENTRY_MS'] = [string]$entry
        $launch.Environment['ARX_STEP5B_PINS_SHA256'] = $trustedDigest
        foreach ($arg in '-I', '-S', '-B', $launcher) { $launch.ArgumentList.Add($arg) }

        if ([Environment]::TickCount64 -ge $entry + 82000) { throw 'Admission deadline' }
        $process = [Diagnostics.Process]::Start($launch)
        $process.StandardInput.Close()
        do {
            $remaining = $entry + 90000 - [Environment]::TickCount64
            if ($remaining -le 0) { throw 'Wall deadline' }
            $exited = $process.WaitForExit([int][Math]::Min(250, $remaining))
        } until ($exited)
        if ([Environment]::TickCount64 -ge $entry + 90000 -or $process.ExitCode -ne 0) {
            throw 'Observation incomplete'
        }
    } catch {
        if ($process) {
            try { if (!$process.HasExited) { $process.Kill() } } catch {}
        }
        throw 'D6 entry refused or incomplete. Preserve evidence and stop.'
    } finally {
        if ($process) { $process.Dispose() }
        foreach ($file in $files) { $file.Dispose() }
    }
}
```

The launcher uses the shared absolute tick from shell entry, including
interpreter startup and admission. Test work stops by the earlier of entry plus
82 seconds and the spawn attempt plus 48 seconds. The last eight seconds of the
90-second wall budget are reserved for containment, drain, reap and evidence.
The named executable is created suspended, with an explicit pipe-handle list
and atomically assigned to the unnamed kill-on-close Job through
`PROC_THREAD_ATTRIBUTE_JOB_LIST`. The launcher verifies membership before resume
and writes one stdin byte before closing the writer. This keeps custody across
interruption during process creation. The outer wait targets only the launcher
and a forced stop is uncertainty. A complete result requires an unforced exit,
pipe EOF, zero active members, ordered witness phases and successful in-process
release. See [the Windows process attribute contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

