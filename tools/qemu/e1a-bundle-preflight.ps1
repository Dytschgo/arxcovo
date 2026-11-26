# Read-only owner check. The qualifier's held inventory remains authoritative.
[CmdletBinding()]
param([string]$Bundle)

function Get-E1aTree([string]$Root) {
    if ($Root -notmatch '^D:\\[A-Za-z0-9 _.-]+(?:\\[A-Za-z0-9 _.-]+)*$' -or
        $Root -cne [IO.Path]::GetFullPath($Root) -or
        ($Root.Substring(3).Split('\') | Where-Object { $_.EndsWith('.') -or $_.EndsWith(' ') })) {
        throw 'Expected an exact directory path on D:'
    }
    $cursor = 'D:\'
    foreach ($part in @('') + $Root.Substring(3).Split('\')) {
        if ($part) { $cursor = Join-Path $cursor $part }
        $item = Get-Item -LiteralPath $cursor -Force -ErrorAction Stop
        if ($item -isnot [IO.DirectoryInfo] -or
            ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Expected ordinary bundle ancestors'
        }
    }

    $nodes = [Collections.Generic.List[IO.FileSystemInfo]]::new()
    $nodes.Add($item)
    for ($i = 0; $i -lt $nodes.Count; $i++) {
        $node = $nodes[$i]
        if ($node.Attributes -band [IO.FileAttributes]::ReparsePoint) {
            throw 'Bundle contains a reparse point'
        }
        if ($node -is [IO.DirectoryInfo]) {
            foreach ($entry in $node.EnumerateFileSystemInfos()) {
                if ($nodes.Count -ge 512) { throw 'Bundle exceeds preflight bound' }
                $nodes.Add($entry)
            }
        }
    }
    $nodes.ToArray() | Sort-Object FullName
}

function Get-E1aAclViolation(
    [Security.AccessControl.RawSecurityDescriptor]$Descriptor,
    [Security.Principal.SecurityIdentifier]$Owner
) {
    $admins = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
    $system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
    if ($Owner -eq $admins -or $Owner -eq $system) { throw 'Expected a distinct owner login' }
    if ($Descriptor.Owner -ne $admins) { 'owner is not Administrators' }
    if (-not ($Descriptor.ControlFlags -band [Security.AccessControl.ControlFlags]::DiscretionaryAclProtected)) {
        'DACL inheritance is enabled'
    }
    if ($null -eq $Descriptor.DiscretionaryAcl) {
        'DACL missing or null'
        return
    }

    $seen = @{}
    $index = 0
    foreach ($ace in $Descriptor.DiscretionaryAcl) {
        $role = 'other'
        $mask = 'unavailable'
        $problems = @()
        if ($ace -is [Security.AccessControl.KnownAce]) {
            $mask = '0x{0:X8}' -f $ace.AccessMask
            if ($ace.SecurityIdentifier -eq $admins) { $role = 'Administrators' }
            elseif ($ace.SecurityIdentifier -eq $system) { $role = 'SYSTEM' }
            elseif ($ace.SecurityIdentifier -eq $Owner) { $role = 'owner login' }
        }
        if ($role -eq 'other') { $problems += 'unexpected trustee' }
        if ($ace.AceType -ne [Security.AccessControl.AceType]::AccessAllowed) {
            $problems += 'expected ordinary allow'
        }
        if ($ace.AceFlags -ne [Security.AccessControl.AceFlags]::None) {
            $problems += 'expected explicit non-inheritable ACE'
        }
        if ($role -ne 'other') {
            # .NET's explicit Allow rules include Synchronize.
            $expected = if ($role -eq 'owner login') { '0x001200A9' } else { '0x001F01FF' }
            if ($mask -ne $expected) { $problems += "expected mask $expected" }
            if ($seen.ContainsKey($role)) { $problems += 'duplicate trustee' }
            $seen[$role] = $true
        }
        if ($problems.Count) {
            "ACE[$index] trustee=$role type=$($ace.AceType) flags=$($ace.AceFlags) mask=$mask ($($problems -join ', '))"
        }
        $index++
    }
    foreach ($role in @('Administrators', 'SYSTEM', 'owner login')) {
        if (-not $seen.ContainsKey($role)) { "missing ACE for $role" }
    }
}

if ($MyInvocation.InvocationName -eq '.') { return }
$ErrorActionPreference = 'Stop'
$index = 0
try {
    $nodes = @(Get-E1aTree $Bundle)
    $owner = [Security.Principal.WindowsIdentity]::GetCurrent().User
    $failed = $false
    foreach ($node in $nodes) {
        # Recheck the selected leaf before requesting its security descriptor.
        $node.Refresh()
        if (-not $node.Exists -or ($node.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Bundle changed'
        }
        $role = if ($node.FullName -ceq $Bundle) { 'root' }
            elseif ($node.FullName -ceq (Join-Path $Bundle 'share')) { 'data' }
            else { 'payload' }
        $acl = Get-Acl -LiteralPath $node.FullName
        $raw = [Security.AccessControl.RawSecurityDescriptor]::new($acl.GetSecurityDescriptorBinaryForm(), 0)
        foreach ($problem in (Get-E1aAclViolation $raw $owner)) {
            Write-Output "Node[$index] $role $problem"
            $failed = $true
        }
        $index++
    }
    if ($failed) { exit 1 }
    Write-Output "E1a ACL preflight passed for $index objects"
    exit 0
} catch {
    [Console]::Error.WriteLine("E1a ACL preflight could not inspect the bounded ordinary tree at node index $index")
    exit 2
}
