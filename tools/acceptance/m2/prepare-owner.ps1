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

function Assert-AbsolutePath([string] $Path) {
    if ([string]::IsNullOrWhiteSpace($Path) -or $Path -ne $Path.Trim() -or $Path.StartsWith('-')) {
        throw 'Acceptance paths must be absolute.'
    }
    if (-not [System.IO.Path]::IsPathFullyQualified($Path)) {
        throw 'Acceptance paths must be absolute.'
    }
    if ($Path -match '(^|[\\/])\.\.([\\/]|$)') {
        throw 'Acceptance paths must be absolute.'
    }
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
    foreach ($path in @($PythonExe, $Relay, $Arxd, $ClientFile, $EvidenceFile)) {
        Assert-AbsolutePath $path
    }
    Assert-RelayCommand $PythonExe
    if ($Relay -eq $Arxd) {
        throw 'The MCP command must be the acceptance relay.'
    }
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
    [System.IO.File]::WriteAllText($Path, $text, [System.Text.UTF8Encoding]::new($false))
}

function Assert-PlainPath([string] $Path) {
    $item = Get-Item -LiteralPath $Path -Force
    while ($null -ne $item) {
        if ($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) {
            throw 'A required path has a reparse point.'
        }
        $item = if ($item -is [System.IO.DirectoryInfo]) { $item.Parent } else { $item.Directory }
    }
}

if ($PSVersionTable.PSVersion.Major -lt 7) { throw 'Open a non-elevated PowerShell 7 console.' }

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
Assert-PlainPath $env:TMP
Assert-PlainPath $ArxExe
Assert-PlainPath $ArxRelay
Assert-PlainPath "$ArxDist/index.html"
if (Test-Path -LiteralPath $ArxSession) { throw 'Owner session already exists. Preserve it and use its existing configs if setup completed.' }

$rustup = (Get-Command rustup.exe -CommandType Application | Select-Object -First 1).Source
$rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $env:USERPROFILE '.rustup' }
$cargo = & $rustup which --toolchain 1.95.0 cargo 2>$null
if ($LASTEXITCODE -ne 0 -or @($cargo).Count -ne 1 -or !(Test-Path -LiteralPath $cargo -PathType Leaf)) {
    throw 'Rustup did not resolve the installed Rust 1.95.0 cargo.exe.'
}
Assert-PlainPath $cargo
Assert-PlainPath $env:SystemRoot
$pythonExe = (Get-Command python.exe -CommandType Application | Select-Object -First 1).Source

New-Item -ItemType Directory -Path $ArxSession | Out-Null
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
New-Item -ItemType Directory -Path $private,$state,$workspace,(Join-OwnerPath $ArxSession 'claude-work'),(Join-OwnerPath $ArxSession 'claude-config'),(Join-OwnerPath $ArxSession 'evidence') | Out-Null

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
