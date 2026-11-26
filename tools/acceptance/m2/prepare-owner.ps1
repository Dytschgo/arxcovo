# Prepare the owner M2/M6 session. Paths are parameters supplied for that run.
# -ConfigOnly writes only the Claude client file and is what the unit test calls.
[CmdletBinding(DefaultParameterSetName = 'Prepare')]
param(
    [Parameter(Mandatory, ParameterSetName = 'Prepare')]
    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [string] $ArxExe,

    [Parameter(Mandatory, ParameterSetName = 'Prepare')]
    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [string] $ArxRelay,

    [Parameter(Mandatory, ParameterSetName = 'Prepare')]
    [string] $ArxSession,

    [Parameter(Mandatory, ParameterSetName = 'Prepare')]
    [string] $ArxDist,

    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [string] $Python,

    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [string] $ClientConfig,

    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [string] $Evidence,

    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [string] $OutFile,

    [Parameter(Mandatory, ParameterSetName = 'ConfigOnly')]
    [switch] $ConfigOnly
)

$ErrorActionPreference = 'Stop'

# Retain the checked directory chain through each create and through config
# generation. Leaf files are opened without following reparse points.
if (-not ('AcceptancePaths' -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.RegularExpressions;
using Microsoft.Win32.SafeHandles;

public sealed class AcceptancePaths : IDisposable
{
    [StructLayout(LayoutKind.Sequential)]
    private struct FileInfo
    {
        public uint Attributes, CreationLow, CreationHigh, AccessLow, AccessHigh,
            WriteLow, WriteHigh, Volume, SizeHigh, SizeLow, Links, IndexHigh, IndexLow;
    }

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern SafeFileHandle CreateFileW(string path, uint access, uint share,
        IntPtr security, uint disposition, uint flags, IntPtr template);

    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern bool GetFileInformationByHandle(SafeFileHandle handle, out FileInfo info);

    [DllImport("kernel32.dll")]
    private static extern uint GetFileType(SafeFileHandle handle);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CreateDirectoryW(string path, IntPtr security);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern uint GetFileAttributesW(string path);

    private const uint OpenReparsePoint = 0x00200000;
    private const uint BackupSemantics = 0x02000000;
    private const uint ReparsePoint = 0x400;
    private const uint DirectoryAttribute = 0x10;
    private const uint OpenExisting = 3;
    private const uint CreateNew = 1;
    private const uint ShareRead = 1;
    private const uint GenericRead = 0x80000000;
    private const uint GenericWrite = 0x40000000;
    private readonly List<SafeFileHandle> pins = new List<SafeFileHandle>();
    private readonly HashSet<string> directories = new HashSet<string>(StringComparer.OrdinalIgnoreCase);

    private static string Absolute(string path)
    {
        if (String.IsNullOrWhiteSpace(path) || path != path.Trim()
            || !Regex.IsMatch(path, @"^[A-Za-z]:[\\/]"))
            throw new IOException("Acceptance paths must be absolute local paths.");
        foreach (string part in path.Substring(3).Split('\\', '/'))
        {
            if (part == "." || part == "") continue;
            if (part.EndsWith(" ") || part.EndsWith(".") || part.IndexOfAny(Path.GetInvalidFileNameChars()) >= 0
                || Regex.IsMatch(part.Split('.')[0].TrimEnd(),
                    @"^(CON|PRN|AUX|NUL|CONIN\$|CONOUT\$|COM[1-9¹²³]|LPT[1-9¹²³])$", RegexOptions.IgnoreCase))
                throw new IOException("Acceptance paths must be absolute and use plain names.");
        }
        return Path.TrimEndingDirectorySeparator(Path.GetFullPath(path));
    }

    private static SafeFileHandle Open(string path, uint access, uint share, uint disposition, bool directory)
    {
        uint flags = OpenReparsePoint | (directory ? BackupSemantics : 0);
        SafeFileHandle handle = CreateFileW(path, access, share, IntPtr.Zero, disposition, flags, IntPtr.Zero);
        if (handle.IsInvalid || GetFileType(handle) != 1
            || !GetFileInformationByHandle(handle, out FileInfo info)
            || (info.Attributes & ReparsePoint) != 0
            || ((info.Attributes & DirectoryAttribute) != 0) != directory
            || (!directory && info.Links != 1))
        {
            handle.Dispose();
            throw new IOException("Could not pin a plain acceptance path.");
        }
        return handle;
    }

    private void PinDirectory(string path)
    {
        if (directories.Contains(path)) return;
        string parent = Path.GetDirectoryName(path);
        if (parent != null) PinDirectory(parent);
        pins.Add(Open(path, 1, ShareRead, OpenExisting, true));
        directories.Add(path);
    }

    private string Leaf(string path)
    {
        path = Absolute(path);
        string parent = Path.GetDirectoryName(path);
        if (parent == null) throw new IOException("Expected an acceptance path below the volume root.");
        PinDirectory(parent);
        return path;
    }

    public void HoldDirectory(string path) => PinDirectory(Absolute(path));

    public void HoldFile(string path) => pins.Add(Open(Leaf(path), GenericRead, ShareRead, OpenExisting, false));

    public void ExpectNewFile(string path)
    {
        uint attributes = GetFileAttributesW(Leaf(path));
        if (attributes != UInt32.MaxValue || Marshal.GetLastWin32Error() != 2)
            throw new IOException("The acceptance destination must be new.");
    }

    public void NewDirectory(string path)
    {
        path = Leaf(path);
        if (!CreateDirectoryW(path, IntPtr.Zero))
            throw new IOException("Could not create a new acceptance directory.");
        PinDirectory(path);
    }

    public void WriteNew(string path, string text)
    {
        using (SafeFileHandle handle = Open(Leaf(path), GenericWrite, 0, CreateNew, false))
        using (var stream = new FileStream(handle, FileAccess.Write))
        {
            byte[] bytes = Encoding.UTF8.GetBytes(text);
            stream.Write(bytes, 0, bytes.Length);
        }
    }

    public void Dispose()
    {
        for (int i = pins.Count - 1; i >= 0; i--) pins[i].Dispose();
    }
}
'@
}

function Assert-RelayCommand([string] $Command) {
    $name = [System.IO.Path]::GetFileName($Command)
    if ($name -match '^(?i)arxd(\.exe)?$') {
        throw 'The MCP command must be the acceptance relay.'
    }
}

function Join-OwnerPath([string] $Base, [string] $Child) {
    $separator = if ($Base.Contains('\') -and -not $Base.Contains('/')) { '\' } else { '/' }
    return $Base.TrimEnd('\', '/') + $separator + $Child
}

function New-AcceptanceMcpConfig {
    param(
        [Parameter(Mandatory)][string] $PythonExe,
        [Parameter(Mandatory)][string] $Relay,
        [Parameter(Mandatory)][string] $Arxd,
        [Parameter(Mandatory)][string] $ClientFile,
        [Parameter(Mandatory)][string] $EvidenceFile
    )
    Assert-RelayCommand $PythonExe
    if ($Relay -eq $Arxd) {
        throw 'The MCP command must be the acceptance relay.'
    }
    foreach ($path in @($PythonExe, $Relay, $Arxd, $ClientFile)) {
        $pins.HoldFile($path)
    }
    $pins.ExpectNewFile($EvidenceFile)
    $relayArgs = @(
        '-I', '-B', $Relay,
        '--arxd', $Arxd,
        '--client-config', $ClientFile,
        '--evidence', $EvidenceFile
    )
    [ordered]@{
        mcpServers = [ordered]@{
            'arxcovo-owner' = [ordered]@{
                type = 'stdio'
                command = $PythonExe
                args = $relayArgs
            }
        }
    }
}

function Write-JsonFile([string] $Path, $Value) {
    $text = $Value | ConvertTo-Json -Depth 10
    $pins.WriteNew($Path, $text)
}

$pins = [AcceptancePaths]::new()
try {
    if ($ConfigOnly) {
        $config = New-AcceptanceMcpConfig -PythonExe $Python -Relay $ArxRelay -Arxd $ArxExe -ClientFile $ClientConfig -EvidenceFile $Evidence
        Write-JsonFile $OutFile $config
        return
    }

    $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [System.Security.Principal.WindowsPrincipal]::new($identity)
    if ($principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw 'Open a new PowerShell 7 console without Run as administrator.'
    }
    if (-not $env:TMP -or -not $env:TEMP -or -not $env:CARGO_HOME -or -not $env:CARGO_TARGET_DIR) {
        throw 'Set TMP, TEMP, CARGO_HOME and CARGO_TARGET_DIR before preparing the owner session.'
    }
    foreach ($directory in @($env:TMP, $env:TEMP, $env:CARGO_HOME, $env:CARGO_TARGET_DIR, $ArxDist)) {
        $pins.HoldDirectory($directory)
    }
    foreach ($path in @($ArxExe, $ArxRelay, "$ArxDist/index.html")) {
        $pins.HoldFile($path)
    }

    $rustup = (Get-Command rustup.exe -CommandType Application | Select-Object -First 1).Source
    $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
    $cargo = & $rustup which --toolchain 1.95.0 cargo 2>$null
    if ($LASTEXITCODE -ne 0 -or @($cargo).Count -ne 1 -or !(Test-Path -LiteralPath $cargo -PathType Leaf)) {
        throw 'Rustup did not resolve the installed Rust 1.95.0 cargo.exe.'
    }
    $pins.HoldFile($cargo)
    $pins.HoldDirectory($env:SystemRoot)
    $pythonExe = (Get-Command python.exe -CommandType Application | Select-Object -First 1).Source

    $pins.HoldFile($pythonExe)
    $pins.NewDirectory($ArxSession)
    $acl = [System.Security.AccessControl.DirectorySecurity]::new()
    $acl.SetOwner($identity.User)
    $acl.SetAccessRuleProtection($true, $false)
    $inherit = [System.Security.AccessControl.InheritanceFlags]'ContainerInherit, ObjectInherit'
    $rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
        $identity.User, 'FullControl', $inherit, 'None', 'Allow')
    $acl.AddAccessRule($rule)
    Set-Acl -LiteralPath $ArxSession -AclObject $acl
    $applied = Get-Acl -LiteralPath $ArxSession
    $rules = @($applied.GetAccessRules($true, $true, [System.Security.Principal.SecurityIdentifier]))
    if (!$applied.AreAccessRulesProtected -or $applied.GetOwner([System.Security.Principal.SecurityIdentifier]) -ne $identity.User -or
        $rules.Count -ne 1 -or $rules[0].IdentityReference -ne $identity.User) {
        throw 'Owner-only session ACL could not be verified.'
    }
    $private = Join-OwnerPath $ArxSession 'private'
    $state = Join-OwnerPath $ArxSession 'state'
    $workspace = Join-OwnerPath $ArxSession 'work'
    foreach ($directory in @($private, $state, $workspace, (Join-OwnerPath $ArxSession 'claude-work'),
            (Join-OwnerPath $ArxSession 'claude-config'), (Join-OwnerPath $ArxSession 'evidence'))) {
        $pins.NewDirectory($directory)
    }

    $pipe = '\\.\pipe\arxcovo-owner-' + [guid]::NewGuid().ToString('N')
    $bytes = [System.Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
    try {
        $token = [Convert]::ToHexString($bytes).ToLowerInvariant()
        $hash = [Convert]::ToHexString(
            [System.Security.Cryptography.SHA256]::HashData([System.Text.Encoding]::ASCII.GetBytes($token))
        ).ToLowerInvariant()
        $clientFile = Join-OwnerPath $private 'client.json'
        Write-JsonFile $clientFile @{
            address = $pipe
            expected_server_identity = $identity.User.Value
            token = $token
        }
        $daemon = @{
            state_dir = $state
            local = @{ address = $pipe; client_identity = $identity.User.Value }
            agents = @(@{ principal = 'claude-code'; token_sha256 = $hash })
            execution = @{
                mode = 'current_user'
                workspace = $workspace
                environment = @{
                    PATH = Split-Path -LiteralPath $cargo
                    SystemRoot = $env:SystemRoot
                    TEMP = $env:TEMP
                    TMP = $env:TMP
                    CARGO_HOME = $env:CARGO_HOME
                    CARGO_TARGET_DIR = $env:CARGO_TARGET_DIR
                    RUSTUP_HOME = $rustupHome
                }
                limits = @{ memory_bytes = 2147483648; pids = 32 }
            }
        }
        Write-JsonFile (Join-OwnerPath $private 'm2-daemon.json') $daemon
        $daemon.dashboard = @{ listen = '127.0.0.1:47623'; dist = $ArxDist }
        Write-JsonFile (Join-OwnerPath $private 'm6-daemon.json') $daemon
        $evidenceFile = Join-OwnerPath (Join-OwnerPath $ArxSession 'evidence') 'm2-mcp.jsonl'
        $mcp = New-AcceptanceMcpConfig -PythonExe $pythonExe -Relay $ArxRelay -Arxd $ArxExe -ClientFile $clientFile -EvidenceFile $evidenceFile
        Write-JsonFile (Join-OwnerPath (Join-OwnerPath $ArxSession 'claude-work') '.mcp.json') $mcp
    } finally {
        [Array]::Clear($bytes, 0, $bytes.Length)
        Remove-Variable token,hash,daemon,clientFile,evidenceFile,mcp -ErrorAction SilentlyContinue
    }
    'Private owner session and M2/M6 configs created.'
} finally {
    $pins.Dispose()
}
