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

The November 20 replacement builder assignment is limited to the review findings.
The stopped builder stays at 3/3. The last merged PR is #201 at
990761343e6529c42d3d90da4870d97ee0f5bc49. Stopped candidate
183a4ff4de4aeadfc329c71f6d8cb4d3ae84e18d remains in history. Its HOLD review
SHA256 is 4b6b8592bf365c22bad163467a3a1c20c95a7a4cc4ac5c1cb4eef54a54ddd803.
The coordinator records checked-head evidence and task outcomes in PROGRESS.
The replacement head needs independent review and the normal green gates.

### Qualified entry for a later owner-authorized run

The coordinator stages the exact reviewed source, the checked test executable at
`bin/arxd-step5b.exe`, and the provenance files in the fixed observation root.
`source-manifest.json` covers the immutable source tree at the reviewed commit.
`loader.json` records the qualified toolchain, Windows loader, and absolute
CPython installation, its Windows loader dependencies and the DLL search
environment. Use a qualified flat CPython runtime with bundled standard-library
ZIP and a complete externally qualified non-system runtime inventory. The
coordinator qualifies immutable staged source against the reviewed commit and
the runtime inventory against the interpreter before entry. Stage `pins.json` with keys
`head`, `test`, `binary_bytes`, `binary_sha256`, `python` (native absolute
interpreter path), `python_sha256`, `python_runtime` (1–64 runtime leaf
names and their SHA256 digests, up to 64 MiB each) and `sha256`. The latter maps `launcher`,
`support`, `manifest`, `loader`, `lockfile`, `driver` and `case` to the hashes for
the fixed paths in the launcher. The coordinator verifies the source, loader and
binary qualification and records the trusted pins digest and interpreter path
with the owner grant. All observation inputs and directories must already be
private and staged, with the fixture ACL precondition satisfied under `tmp`.
Qualification includes the shell runtime and supports Windows PowerShell 7 on
.NET through version 10, where `Environment.TickCount64` uses the same Win32
clock as the launcher. See the
[clock compatibility record](https://learn.microsoft.com/en-us/dotnet/core/compatibility/core-libraries/11/environment-tickcount-windows-behavior).

As part of shell qualification, preload the managed deadline controller in the
same PowerShell session. This declaration requires no observation inputs and
creates no runspace, Job or child. The entry below takes the clock reading as
it calls that controller. The controller arms its watchdog before any native
compilation, runspace creation, Job creation or access to the staged inputs.

The two exclusive files mark entry and launcher admission. A refusal at any
point in the one-shot entry spends the authorization. Verified read handles
inhibit writes and replacement through use. CPython runs with isolated imports,
site initialization disabled and an explicit environment.

```powershell
Add-Type -TypeDefinition @'
using System;
using System.Management.Automation;
using System.Threading;
using System.Threading.Tasks;

public static class D6Entry {
    static int Remaining(long deadline) {
        return (int)Math.Max(0, deadline - Environment.TickCount64);
    }

    public static bool WithinWall(long cutoff, long wall, Func<CancellationToken, bool> setup) {
        var deadline = new CancellationTokenSource();
        var completed = new ManualResetEventSlim(false);
        // Arm a dedicated watchdog before creating the setup worker or runspace.
        new Thread(() => {
            if (!completed.Wait(Remaining(cutoff))) deadline.Cancel();
        }) { IsBackground = true }.Start();

        var worker = Task.Factory.StartNew(() => {
            try {
                deadline.Token.ThrowIfCancellationRequested();
                return setup(deadline.Token);
            } catch (Exception) {
                return false;
            }
        }, CancellationToken.None, TaskCreationOptions.LongRunning, TaskScheduler.Default);
        if (!worker.Wait(Remaining(cutoff))) {
            worker.Wait(Remaining(wall));
            return false;
        }
        if (Environment.TickCount64 >= cutoff) return false;
        completed.Set();
        return worker.Result;
    }

    public static bool Run(long entry, string source) {
        return WithinWall(entry + 89750, entry + 90000, deadline => {
            using var shell = PowerShell.Create();
            var output = shell.AddScript(source).AddArgument(entry).AddArgument(deadline).Invoke();
            return !shell.HadErrors && output.Count == 1 &&
                output[0].BaseObject is bool success && success;
        });
    }
}
'@
```

The qualified one-shot entry follows. Fill the two values from the grant
before entry. Use this block only with the separate exact-head grant.

```powershell
& {
    if (![D6Entry]::Run([Environment]::TickCount64, @'
param([long]$entry, [Threading.CancellationToken]$deadline)
$deadline.ThrowIfCancellationRequested()
if ([Environment]::TickCount64 -ge $entry + 82000) { throw 'Deadline' }
$ErrorActionPreference = 'Stop'
$root = 'D:\Temp\arx-m3-step5b\observation'
$qualifiedPython = '<qualified absolute interpreter from the grant>'
$trustedDigest = '<trusted pins SHA256 from the grant>'
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

public sealed class D6Custody : IDisposable {
    [StructLayout(LayoutKind.Sequential)]
    struct FileInfo {
        public uint Attributes, CtLow, CtHigh, AtLow, AtHigh, WtLow, WtHigh;
        public uint Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct Startup {
        public uint Size;
        public IntPtr Reserved, Desktop, Title;
        public uint X, Y, Xsize, Ysize, Xcount, Ycount, Fill, Flags;
        public ushort Show, ReservedSize;
        public IntPtr Reserved2, Input, Output, Error, Attributes;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct ProcessInfo {
        public IntPtr Process, Thread;
        public uint Id, ThreadId;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct BasicLimits {
        public long ProcessTime, JobTime;
        public uint Flags;
        public UIntPtr MinWorkingSet, MaxWorkingSet;
        public uint ActiveLimit;
        public UIntPtr Affinity;
        public uint Priority, Scheduling;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct JobLimits {
        public BasicLimits Basic;
        public ulong Reads, Writes, Other, ReadBytes, WriteBytes, OtherBytes;
        public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
    }
    [StructLayout(LayoutKind.Sequential)]
    struct Accounting {
        public long User, Kernel, PeriodUser, PeriodKernel;
        public uint Faults, Total, Active, Terminated;
    }
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern SafeFileHandle CreateFile(string path, uint access, uint share,
        IntPtr security, uint mode, uint flags, IntPtr template);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern uint GetFinalPathNameByHandle(SafeFileHandle file, StringBuilder path,
        uint length, uint flags);
    [DllImport("kernel32.dll")]
    static extern bool GetFileInformationByHandle(SafeFileHandle file, out FileInfo info);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern SafeFileHandle CreateJobObject(IntPtr security, string name);
    [DllImport("kernel32.dll")]
    static extern bool SetInformationJobObject(SafeFileHandle job, int kind,
        ref JobLimits info, uint length);
    [DllImport("kernel32.dll")]
    static extern bool QueryInformationJobObject(SafeFileHandle job, int kind,
        out Accounting info, uint length, IntPtr returned);
    [DllImport("kernel32.dll")]
    static extern bool InitializeProcThreadAttributeList(IntPtr attrs, int count,
        int flags, ref IntPtr size);
    [DllImport("kernel32.dll")]
    static extern bool UpdateProcThreadAttribute(IntPtr attrs, uint flags, IntPtr key,
        IntPtr value, UIntPtr size, IntPtr previous, IntPtr returned);
    [DllImport("kernel32.dll")]
    static extern void DeleteProcThreadAttributeList(IntPtr attrs);
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
    static extern bool CreateProcess(string app, StringBuilder command,
        IntPtr processSecurity, IntPtr threadSecurity, bool inherit, uint flags,
        IntPtr env, string cwd, ref Startup startup, out ProcessInfo process);
    [DllImport("kernel32.dll")]
    static extern bool IsProcessInJob(IntPtr process, SafeFileHandle job, out bool member);
    [DllImport("kernel32.dll")]
    static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll")]
    static extern uint WaitForSingleObject(IntPtr process, uint ms);
    [DllImport("kernel32.dll")]
    static extern bool GetExitCodeProcess(IntPtr process, out uint code);
    [DllImport("kernel32.dll")]
    static extern bool TerminateJobObject(SafeFileHandle job, uint code);
    [DllImport("kernel32.dll")]
    static extern bool CloseHandle(IntPtr handle);

    static void Require(bool ok) {
        if (!ok) throw new InvalidOperationException("D6 native refusal");
    }
    public static SafeFileHandle Hold(string path, bool directory) {
        uint flags = directory ? 0x02200000u : 0x00200000u;
        uint share = directory ? 3u : 1u;
        var handle = CreateFile(path, directory ? 0x80u : 0x80000000u, share,
            IntPtr.Zero, 3, flags, IntPtr.Zero);
        try {
            var name = new StringBuilder(1024);
            uint size = GetFinalPathNameByHandle(handle, name, 1024, 0);
            Require(!handle.IsInvalid && size > 0 && size < 1024
                && name.ToString() == @"\\?\" + path);
            Require(GetFileInformationByHandle(handle, out FileInfo first)
                && (first.Attributes & 0x400) == 0
                && ((first.Attributes & 0x10) != 0) == directory
                && (directory || first.Links == 1));
            using (var again = CreateFile(path, 0x80, share, IntPtr.Zero, 3, flags, IntPtr.Zero)) {
                Require(!again.IsInvalid && GetFileInformationByHandle(again, out FileInfo second)
                    && first.Attributes == second.Attributes && first.Links == second.Links
                    && first.Volume == second.Volume && first.IndexHigh == second.IndexHigh
                    && first.IndexLow == second.IndexLow);
            }
            return handle;
        } catch {
            handle.Dispose();
            throw;
        }
    }

    readonly SafeFileHandle job;

    public D6Custody() {
        job = CreateJobObject(IntPtr.Zero, null);
        var limits = new JobLimits();
        limits.Basic.Flags = 0x2000; // Kill on close. No breakaway.
        if (job.IsInvalid || !SetInformationJobObject(job, 9, ref limits,
            (uint)Marshal.SizeOf<JobLimits>())) {
            job.Dispose();
            Require(false);
        }
    }
    public static int Remaining(long deadline, long now) {
        return (int)Math.Max(0, deadline - now);
    }
    public CancellationTokenRegistration Watch(CancellationToken deadline) {
        return deadline.Register(() => {
            try { TerminateJobObject(job, 1); }
            catch (ObjectDisposedException) {}
        });
    }
    bool Empty() {
        return QueryInformationJobObject(job, 1, out Accounting info,
            (uint)Marshal.SizeOf<Accounting>(), IntPtr.Zero) && info.Active == 0;
    }
    public bool Run(string exe, string script, string[] env, string cwd, long entry,
        CancellationToken deadline) {
        long work = entry + 82000, wall = entry + 90000;
        IntPtr size = IntPtr.Zero, attrs = IntPtr.Zero, environment = IntPtr.Zero, jobList = IntPtr.Zero;
        bool initialized = false, cleanupConfirmed = false;
        var child = new ProcessInfo();
        var command = new StringBuilder("\"" + exe + "\" -I -S -B \"" + script + "\"");
        try {
            InitializeProcThreadAttributeList(IntPtr.Zero, 1, 0, ref size);
            Require(size.ToInt64() > 0 && size.ToInt64() <= 4096);
            attrs = Marshal.AllocHGlobal(size);
            initialized = InitializeProcThreadAttributeList(attrs, 1, 0, ref size);
            Require(initialized);
            jobList = Marshal.AllocHGlobal(IntPtr.Size);
            Marshal.WriteIntPtr(jobList, job.DangerousGetHandle());
            Require(UpdateProcThreadAttribute(attrs, 0, (IntPtr)0x2000d, jobList,
                (UIntPtr)IntPtr.Size, IntPtr.Zero, IntPtr.Zero));
            Array.Sort(env, StringComparer.OrdinalIgnoreCase);
            environment = Marshal.StringToHGlobalUni(string.Join("\0", env) + "\0");
            var startup = new Startup { Size = (uint)Marshal.SizeOf<Startup>(), Attributes = attrs };
            Require(!deadline.IsCancellationRequested && Environment.TickCount64 < work);
            Require(CreateProcess(exe, command, IntPtr.Zero, IntPtr.Zero, false,
                0x08080404, environment, cwd, ref startup, out child));
            Require(IsProcessInJob(child.Process, job, out bool member) && member);
            Require(!deadline.IsCancellationRequested && Environment.TickCount64 < work);
            Require(ResumeThread(child.Thread) == 1);
            Require(WaitForSingleObject(child.Process,
                (uint)Remaining(wall, Environment.TickCount64)) == 0);
            Require(!deadline.IsCancellationRequested && Environment.TickCount64 < wall && Empty());
            cleanupConfirmed = true;
            return GetExitCodeProcess(child.Process, out uint code) && code == 0;
        } finally {
            if (!cleanupConfirmed) {
                bool stopped = TerminateJobObject(job, 1);
                bool reaped = child.Process == IntPtr.Zero || WaitForSingleObject(child.Process,
                    (uint)Remaining(wall, Environment.TickCount64)) == 0;
                cleanupConfirmed = stopped && reaped && Empty();
            }
            if (child.Process != IntPtr.Zero) CloseHandle(child.Process);
            if (child.Thread != IntPtr.Zero) CloseHandle(child.Thread);
            if (environment != IntPtr.Zero) Marshal.FreeHGlobal(environment);
            if (initialized) DeleteProcThreadAttributeList(attrs);
            if (attrs != IntPtr.Zero) Marshal.FreeHGlobal(attrs);
            if (jobList != IntPtr.Zero) Marshal.FreeHGlobal(jobList);
            Require(cleanupConfirmed);
        }
    }
    public void Dispose() { job.Dispose(); }
}
"@
$deadline.ThrowIfCancellationRequested()
if ([Environment]::TickCount64 -ge $entry + 82000) { throw 'Deadline' }
$custody = [D6Custody]::new()
$watch = $null
try {
    $watch = $custody.Watch($deadline)
    $deadline.ThrowIfCancellationRequested()
    $files = [Collections.Generic.List[IDisposable]]::new()
    $directories = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    try {
        $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
        $allowed = @($sid, 'S-1-5-18', 'S-1-5-32-544')
        function Assert-Work {
            $deadline.ThrowIfCancellationRequested()
            if ([Environment]::TickCount64 -ge $entry + 82000) { throw 'Deadline' }
        }
        function Hold-Path([string]$path, [bool]$directory) {
            Assert-Work
            if (![IO.Path]::IsPathFullyQualified($path) -or
                [IO.Path]::GetFullPath($path) -cne $path -or
                $path -notmatch '^[A-Za-z]:\\') { throw 'Path' }
            if ($directory -and $directories.Contains($path)) { return }
            $parent = [IO.Path]::GetDirectoryName($path)
            if ($parent) { Hold-Path $parent $true }
            $handle = [D6Custody]::Hold($path, $directory)
            $files.Add($handle)
            if ($path -ceq $root -or $path.StartsWith("$root\", 'Ordinal')) {
                $acl = Get-Acl -LiteralPath $path
                $rules = $acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])
                if (!$acl.AreAccessRulesCanonical -or $rules.Count -eq 0) { throw 'ACL' }
                foreach ($rule in $rules) {
                    if ($rule.AccessControlType -eq 'Allow' -and
                        $rule.IdentityReference.Value -notin $allowed) { throw 'ACL' }
                }
            }
            if ($directory) { [void]$directories.Add($path) } else { return $handle }
        }
        function Hold-Input([string]$path, [long]$cap) {
            $stream = [IO.FileStream]::new((Hold-Path $path $false), 'Read')
            $files.Add($stream)
            if ($stream.Length -le 0 -or $stream.Length -gt $cap) { throw 'Size' }
            return $stream
        }
        function Get-Digest([IO.Stream]$stream) {
            Assert-Work
            return [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()
        }

        Hold-Path $root $true
        $drive = [IO.DriveInfo]::new('D')
        if ($drive.DriveType -ne 'Fixed' -or $drive.DriveFormat -cne 'NTFS') { throw 'Volume' }
        Assert-Work
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
        $runtime = $pins.python_runtime
        if ($runtime -isnot [Collections.IDictionary] -or $runtime.Count -lt 1 -or
            $runtime.Count -gt 64) { throw 'Runtime inventory' }
        $pythonDir = [IO.Path]::GetDirectoryName($qualifiedPython)
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        [void]$names.Add([IO.Path]::GetFileName($qualifiedPython))
        Hold-Path $pythonDir $true
        foreach ($leaf in $runtime.Keys) {
            if ($leaf -cnotmatch '^[A-Za-z0-9_][A-Za-z0-9_.-]{0,95}$' -or $leaf.EndsWith('.') -or
                !$names.Add($leaf) -or $runtime[$leaf] -cnotmatch '^[0-9a-f]{64}$' -or
                (Get-Digest (Hold-Input "$pythonDir\$leaf" 64MB)) -cne $runtime[$leaf]) {
                throw 'Runtime'
            }
        }
        $seen = 0
        foreach ($item in [IO.Directory]::EnumerateFileSystemEntries($pythonDir)) {
            Assert-Work
            if (++$seen -gt 65 -or !$names.Contains([IO.Path]::GetFileName($item))) {
                throw 'Runtime inventory'
            }
        }
        if ($seen -ne $names.Count) { throw 'Runtime inventory' }

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
        foreach ($inputFile in @(
            @('manifest', 'source-manifest.json', 16MB),
            @('loader', 'loader.json', 64KB),
            @('lockfile', 'source\Cargo.lock', 1MB),
            @('driver', 'source\crates\arxd\src\cluster\consensus\bound_start.rs', 64KB),
            @('case', 'source\crates\arxd\src\cluster\consensus\bound_start\nominal_observation.rs', 64KB)
        )) {
            $key, $leaf, $cap = $inputFile
            if ((Get-Digest (Hold-Input "$root\$leaf" $cap)) -cne $pins.sha256[$key]) {
                throw 'Source'
            }
        }
        $cap = [long]$pins.binary_bytes
        if ($cap -le 0 -or $cap -gt 512MB) { throw 'Binary size' }
        $binary = Hold-Input "$root\bin\arxd-step5b.exe" $cap
        if ($binary.Length -ne $cap -or (Get-Digest $binary) -cne $pins.binary_sha256) {
            throw 'Binary'
        }
        $rootEntries = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($leaf in 'entry-claimed','pins.json','source-manifest.json','loader.json',
            'source','bin','tmp','target','cargo-home','rustup-home','evidence') {
            [void]$rootEntries.Add($leaf)
        }
        foreach ($item in [IO.Directory]::EnumerateFileSystemEntries($root)) {
            Assert-Work
            if (!$rootEntries.Remove([IO.Path]::GetFileName($item))) { throw 'Layout' }
        }
        if ($rootEntries.Count -ne 0) { throw 'Layout' }
        $binEntries = [IO.Directory]::EnumerateFileSystemEntries("$root\bin").GetEnumerator()
        try {
            if (!$binEntries.MoveNext() -or $binEntries.Current -cne "$root\bin\arxd-step5b.exe" -or
                $binEntries.MoveNext()) { throw 'Binary layout' }
        } finally {
            $binEntries.Dispose()
        }
        $windows = [IO.Directory]::GetParent([Environment]::SystemDirectory).FullName
        $environment = @(
            "SYSTEMROOT=$windows", "WINDIR=$windows", "ARX_SCRATCH_ROOT=$root",
            'CARGO_NET_OFFLINE=true', 'CARGO_PROFILE_DEV_DEBUG=0',
            "ARX_STEP5B_ENTRY_MS=$entry", "ARX_STEP5B_PINS_SHA256=$trustedDigest"
        )
        foreach ($key in $scratch.Keys) {
            $path = "$root\$($scratch[$key])"
            Hold-Path $path $true
            $environment += "$key=$path"
        }
        $tmpAcl = Get-Acl -LiteralPath "$root\tmp"
        $tmpRules = $tmpAcl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier])
        $inherit = [Security.AccessControl.InheritanceFlags]'ContainerInherit,ObjectInherit'
        $full = [Security.AccessControl.FileSystemRights]::FullControl
        $usable = @($tmpRules | Where-Object {
            $_.IdentityReference.Value -eq $sid -and $_.AccessControlType -eq 'Allow' -and
            ($_.InheritanceFlags -band $inherit) -eq $inherit -and
            ($_.FileSystemRights -band $full) -eq $full -and $_.PropagationFlags -eq 'None'
        })
        if ($usable.Count -eq 0) { throw 'Fixture ACL' }
        Assert-Work
        $custody.Run($qualifiedPython, $launcher, [string[]]$environment, $root, $entry, $deadline)
    } finally {
        foreach ($handle in $files) { $handle.Dispose() }
    }
} finally {
    if ($null -ne $watch) { $watch.Dispose() }
    $custody.Dispose()
}
'@)) {
        throw 'D6 entry refused or incomplete. Preserve evidence and stop.'
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
and writes one stdin byte. A separate custodian enforces the test deadline
independently of pipe draining and the bounded, ordered evidence writer.
The outer wait covers runspace startup, native compilation, Job setup,
preflight and launcher creation. Work that reaches the Job registers it with
the already-running watchdog before validation. The launcher enters that Job
suspended, with membership and the absolute work deadline checked before
resume. The watchdog signals the attributed Job at 89.75 seconds, leaving up
to 250 ms for signaled-handle and zero-active proof. If creation is still in flight, the resume fence and
kill-on-close remain in effect. Forced stop or missing cleanup proof is
uncertainty. A complete result requires an unforced exit,
pipe EOF, zero active members, ordered witness phases and successful in-process
release. See [the Windows process attribute contract](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).
