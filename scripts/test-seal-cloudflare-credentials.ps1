#Requires -Version 7.0
<#
.SYNOPSIS
Deterministic, non-elevated tests for scripts/seal-cloudflare-credentials.ps1.

.DESCRIPTION
Every secret, account tag, tunnel ID and tunnel secret here is synthetic and generated
at run time; none is a real Cloudflare value. The suite creates one owner-private
directory D:\ArxcovoData\tmp-seal-test-<guid> (so the real D:\ArxcovoData fence is
exercised), runs the sealer as a child process with secrets on redirected stdin only,
and removes the directory in `finally`. Output names cases only; it never prints a
secret, SID or ciphertext.

Round trips use .NET ProtectedData (independent of the sealer's own P/Invoke), and the
leaf custody checks reimplement what arxd's read_secret requires: current-user owner,
protected DACL granting only the current user, a single hard link, a user-scope DPAPI
header (security.rs user_scope_dpapi) and ciphertext <= 16384 bytes.

Not exercised here: the interactive Read-Host prompt, the elevated-token refusal and the
transcription-policy registry refusal (they need a console, an elevated token or policy
changes). Live Cloudflare publishing stays unverified on real hardware.
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (-not $IsWindows) { throw 'Windows is required.' }
$sealer = Join-Path $PSScriptRoot 'seal-cloudflare-credentials.ps1'
$pwsh = (Get-Process -Id $PID).Path
$userSid = [Security.Principal.WindowsIdentity]::GetCurrent().User
$root = 'D:\ArxcovoData\tmp-seal-test-' + [guid]::NewGuid().ToString('N')

$script:passed = 0
$script:failed = 0
$script:skipped = 0

function Write-Case([string]$Name, [bool]$Condition) {
    if ($Condition) { $script:passed++; Write-Output "PASS  $Name" }
    else { $script:failed++; Write-Output "FAIL  $Name" }
}

function Skip-Case([string]$Name, [string]$Reason) {
    $script:skipped++
    Write-Output "SKIP  $Name ($Reason)"
}

function Get-RandomBytes([int]$Count) {
    $bytes = [byte[]]::new($Count)
    [Security.Cryptography.RandomNumberGenerator]::Fill($bytes)
    $bytes
}

function New-SyntheticHex { -join ((Get-RandomBytes 16) | ForEach-Object { $_.ToString('x2') }) }
function New-SyntheticTunnelId { [guid]::NewGuid().ToString('D') }
function New-SyntheticSecret { [Convert]::ToBase64String((Get-RandomBytes 32)) }
function Get-Ascii([string]$Value) { [Text.Encoding]::ASCII.GetBytes($Value) }

function New-TunnelJson([string]$Account, [string]$Secret, [string]$Tunnel, [string]$Tail = '') {
    '{"AccountTag":"' + $Account + '","TunnelSecret":"' + $Secret + '","TunnelID":"' + $Tunnel + '"' + $Tail + '}'
}

function New-PrivateDirectory([string]$Path, [string]$ExtraAces = '', [string]$DaclFlags = 'P', [string]$AceFlags = 'OICI') {
    $security = [System.Security.AccessControl.DirectorySecurity]::new()
    $sid = $userSid.Value
    $security.SetSecurityDescriptorSddlForm("O:${sid}D:$DaclFlags(A;$AceFlags;FA;;;$sid)$ExtraAces")
    $null = [System.IO.FileSystemAclExtensions]::Create([System.IO.DirectoryInfo]::new($Path), $security)
}

function Invoke-Sealer {
    param(
        [string[]]$Arguments = @(),
        [byte[]]$Stdin = [byte[]]::new(0),
        [string]$Command
    )
    $info = [System.Diagnostics.ProcessStartInfo]::new($pwsh)
    foreach ($item in @('-NoProfile', '-NonInteractive')) { $info.ArgumentList.Add($item) }
    if ($Command) {
        $info.ArgumentList.Add('-Command')
        $info.ArgumentList.Add($Command)
    } else {
        $info.ArgumentList.Add('-File')
        $info.ArgumentList.Add($sealer)
        foreach ($item in $Arguments) { $info.ArgumentList.Add($item) }
    }
    $info.UseShellExecute = $false
    $info.RedirectStandardInput = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [System.Diagnostics.Process]::Start($info)
    try {
        $out = $process.StandardOutput.ReadToEndAsync()
        $err = $process.StandardError.ReadToEndAsync()
        try {
            $process.StandardInput.BaseStream.Write($Stdin, 0, $Stdin.Length)
            $process.StandardInput.BaseStream.Flush()
        } catch [System.IO.IOException] {
            # The sealer may refuse and exit before reading stdin.
        }
        try { $process.StandardInput.Close() } catch [System.IO.IOException] { }
        if (-not $process.WaitForExit(180000)) {
            $process.Kill($true)
            throw 'sealer child timed out'
        }
        $process.WaitForExit()
        [pscustomobject]@{ Exit = $process.ExitCode; Out = $out.Result; Err = $err.Result }
    } finally {
        $process.Dispose()
    }
}

function Test-Clean($Result, [string[]]$Secrets) {
    foreach ($secret in $Secrets) {
        if ([string]::IsNullOrEmpty($secret)) { continue }
        if ($Result.Out.Contains($secret) -or $Result.Err.Contains($secret)) { return $false }
    }
    $true
}

function Get-Leaves([string]$Directory) {
    if (-not (Test-Path -LiteralPath $Directory)) { return @() }
    @(Get-ChildItem -LiteralPath $Directory -Force -File | Where-Object Name -like '*.dpapi')
}

function Test-Refusal {
    param(
        [string]$Name,
        [string[]]$Arguments,
        [byte[]]$Stdin = [byte[]]::new(0),
        [string]$Code,
        [string]$Directory,
        [string[]]$Secrets = @(),
        [int]$LeavesBefore = 0
    )
    $result = Invoke-Sealer -Arguments $Arguments -Stdin $Stdin
    $ok = $result.Exit -eq 2 `
        -and $result.Err.Trim() -eq "seal-cloudflare-credentials: refused: $Code" `
        -and $result.Out.Length -eq 0 `
        -and (Test-Clean $result $Secrets)
    if ($Directory) { $ok = $ok -and (@(Get-Leaves $Directory).Count -eq $LeavesBefore) }
    Write-Case "$Name -> $Code" $ok
    if (-not $ok) { Write-Output "      exit=$($result.Exit) stderr=$($result.Err.Trim())" }
}

# security.rs user_scope_dpapi, reimplemented independently.
function Test-UserScopeHeader([byte[]]$Blob) {
    $provider = [byte[]](0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2, 0x97, 0xeb)
    $Blob.Length -ge 44 `
        -and [BitConverter]::ToUInt32($Blob, 0) -eq 1 `
        -and [Linq.Enumerable]::SequenceEqual([byte[]]$Blob[4..19], $provider) `
        -and ([BitConverter]::ToUInt32($Blob, 40) -band 0x4) -eq 0
}

function Test-PrivateLeaf([string]$Path) {
    $acl = Get-Acl -LiteralPath $Path
    $rules = @($acl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    $links = @(fsutil hardlink list $Path)
    $item = Get-Item -LiteralPath $Path -Force
    $acl.GetOwner([Security.Principal.SecurityIdentifier]).Equals($userSid) `
        -and $acl.AreAccessRulesProtected `
        -and $rules.Count -eq 1 `
        -and $rules[0].IdentityReference.Equals($userSid) `
        -and -not $rules[0].IsInherited `
        -and $rules[0].AccessControlType -eq 'Allow' `
        -and $LASTEXITCODE -eq 0 -and $links.Count -eq 1 `
        -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0
}

function Test-Seal {
    param(
        [string]$Name,
        [string]$Directory,
        [string]$Kind,
        [string[]]$Arguments,
        [byte[]]$Payload,
        [string[]]$Secrets
    )
    $result = Invoke-Sealer -Arguments $Arguments -Stdin $Payload
    $leafName = if ($Kind -eq 'dns') { 'dns.dpapi' } else { 'tunnel.dpapi' }
    $path = Join-Path $Directory $leafName
    $ok = $result.Exit -eq 0 -and $result.Err.Length -eq 0 -and (Test-Path -LiteralPath $path)
    Write-Case "$Name exits 0 and writes $leafName" $ok
    if (-not $ok) {
        Write-Output "      exit=$($result.Exit) stderr=$($result.Err.Trim())"
        return
    }
    $cipher = [IO.File]::ReadAllBytes($path)
    $receipt = $result.Out | ConvertFrom-Json
    $digest = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($cipher)).ToLowerInvariant()
    Write-Case "$Name receipt carries only path, length and digest" (
        $receipt.schema -eq 'arxcovo.cloudflare-seal.v1' `
            -and $receipt.target_directory -eq $Directory `
            -and $receipt.daemon_config_modified -eq $false `
            -and $receipt.public_publishing -eq 'unchanged-default-off' `
            -and @($receipt.leaves).Count -eq 1 `
            -and $receipt.leaves[0].kind -eq $Kind `
            -and $receipt.leaves[0].path -eq $path `
            -and $receipt.leaves[0].ciphertext_bytes -eq $cipher.Length `
            -and $receipt.leaves[0].ciphertext_sha256 -eq $digest `
            -and (@($receipt.leaves[0].PSObject.Properties.Name) -join ',') -eq 'kind,path,ciphertext_bytes,ciphertext_sha256')
    Write-Case "$Name stdout/stderr contain no plaintext" (Test-Clean $result $Secrets)
    $plain = [Security.Cryptography.ProtectedData]::Unprotect($cipher, $null, 'CurrentUser')
    try {
        Write-Case "$Name DPAPI round trip returns the exact payload bytes" (
            [Linq.Enumerable]::SequenceEqual([byte[]]$plain, [byte[]]$Payload))
    } finally {
        [Array]::Clear($plain, 0, $plain.Length)
    }
    $cipherText = [Text.Encoding]::Latin1.GetString($cipher)
    $leak = $false
    foreach ($secret in $Secrets) {
        if ($cipherText.Contains($secret) -or $cipherText.Contains([Text.Encoding]::Latin1.GetString([Text.Encoding]::Unicode.GetBytes($secret)))) {
            $leak = $true
        }
    }
    Write-Case "$Name ciphertext file contains no plaintext" (-not $leak)
    Write-Case "$Name leaf meets read_secret custody (owner, protected DACL, 1 link, user-scope DPAPI, <=16384)" (
        (Test-PrivateLeaf $path) -and (Test-UserScopeHeader $cipher) -and $cipher.Length -le 16384)
    $others = @(Get-ChildItem -LiteralPath $Directory -Force | Where-Object Name -ne $leafName)
    Write-Case "$Name leaves no other files" ($others.Count -eq 0)
}

$junctions = [System.Collections.Generic.List[string]]::new()
try {
    $before = @(Get-ChildItem -LiteralPath 'D:\ArxcovoData' -Directory -Filter 'tmp-seal-test-*').Count
    New-PrivateDirectory $root

    $account = New-SyntheticHex
    $tunnel = New-SyntheticTunnelId
    $secret = New-SyntheticSecret
    $token = 'synthetic.' + [Convert]::ToBase64String((Get-RandomBytes 30)) + '-_~'
    $tunnelArgs = @('-AccountId', $account, '-TunnelId', $tunnel)
    $secrets = @($token, $secret, $account, $tunnel)

    # ---- positive DNS --------------------------------------------------------------------
    $dnsDir = Join-Path $root 'pos-dns'
    Test-Seal -Name 'dns (created dir)' -Directory $dnsDir -Kind 'dns' -Payload (Get-Ascii $token) -Secrets $secrets `
        -Arguments @('-Mode', 'Dns', '-TargetDirectory', $dnsDir, '-CreateDirectory', '-FromStdin')
    $createdAcl = Get-Acl -LiteralPath $dnsDir
    Write-Case 'dns -CreateDirectory makes an owner-only protected directory' (
        $createdAcl.GetOwner([Security.Principal.SecurityIdentifier]).Equals($userSid) -and $createdAcl.AreAccessRulesProtected `
            -and @($createdAcl.GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]) |
                Where-Object { -not $_.IdentityReference.Equals($userSid) }).Count -eq 0)

    $dnsMaxDir = Join-Path $root 'pos-dns-512'
    New-PrivateDirectory $dnsMaxDir
    $maxToken = 'synthetic-' + ('Ab9-_.~+/=' * 51)
    $maxToken = $maxToken.Substring(0, 512)
    Test-Seal -Name 'dns 512-byte boundary' -Directory $dnsMaxDir -Kind 'dns' -Payload (Get-Ascii $maxToken) -Secrets @($maxToken) `
        -Arguments @('-Mode', 'Dns', '-TargetDirectory', $dnsMaxDir, '-FromStdin')

    # ---- positive Tunnel -----------------------------------------------------------------
    $tunnelDir = Join-Path $root 'pos-tunnel'
    New-PrivateDirectory $tunnelDir
    $json = New-TunnelJson $account $secret $tunnel
    Test-Seal -Name 'tunnel without Endpoint' -Directory $tunnelDir -Kind 'tunnel' -Payload (Get-Ascii $json) -Secrets $secrets `
        -Arguments (@('-Mode', 'Tunnel', '-TargetDirectory', $tunnelDir, '-FromStdin') + $tunnelArgs)

    $endpointDir = Join-Path $root 'pos-tunnel-endpoint'
    New-PrivateDirectory $endpointDir
    $jsonEndpoint = New-TunnelJson $account $secret $tunnel ',"Endpoint":""'
    Test-Seal -Name 'tunnel with empty Endpoint' -Directory $endpointDir -Kind 'tunnel' -Payload (Get-Ascii $jsonEndpoint) -Secrets $secrets `
        -Arguments (@('-Mode', 'Tunnel', '-TargetDirectory', $endpointDir, '-FromStdin') + $tunnelArgs)

    $spacedDir = Join-Path $root 'pos-tunnel-whitespace'
    New-PrivateDirectory $spacedDir
    $jsonSpaced = "{`r`n  `"TunnelID`" : `"$tunnel`",`n`t`"AccountTag`": `"$account`",`n  `"TunnelSecret`":`"$secret`"`n}`r`n"
    Test-Seal -Name 'tunnel with JSON whitespace and trailing CRLF (serde accepts; bytes sealed verbatim)' -Directory $spacedDir `
        -Kind 'tunnel' -Payload (Get-Ascii $jsonSpaced) -Secrets $secrets `
        -Arguments (@('-Mode', 'Tunnel', '-TargetDirectory', $spacedDir, '-FromStdin') + $tunnelArgs)

    # ---- negative DNS --------------------------------------------------------------------
    $negDir = Join-Path $root 'neg'
    New-PrivateDirectory $negDir
    $dnsArgs = @('-Mode', 'Dns', '-TargetDirectory', $negDir, '-FromStdin')
    Test-Refusal -Name 'dns empty' -Arguments $dnsArgs -Stdin ([byte[]]::new(0)) -Code 'dns-token-empty' -Directory $negDir
    $long = 'synthetic' + ('x' * 504)
    Test-Refusal -Name 'dns 513 bytes' -Arguments $dnsArgs -Stdin (Get-Ascii $long) -Code 'dns-token-too-long' -Directory $negDir -Secrets @($long)
    foreach ($case in @(
            @{ Name = 'dns space'; Value = 'synthetic token' },
            @{ Name = 'dns trailing LF'; Value = "$token`n" },
            @{ Name = 'dns trailing CRLF'; Value = "$token`r`n" },
            @{ Name = 'dns colon'; Value = 'synthetic:token' },
            @{ Name = 'dns tab'; Value = "synthetic`ttoken" })) {
        Test-Refusal -Name $case.Name -Arguments $dnsArgs -Stdin (Get-Ascii $case.Value) -Code 'dns-token-forbidden-byte' -Directory $negDir -Secrets $secrets
    }
    Test-Refusal -Name 'dns non-ASCII' -Arguments $dnsArgs -Stdin ([Text.Encoding]::UTF8.GetBytes("synthetic$([char]0xE9)token")) `
        -Code 'dns-token-forbidden-byte' -Directory $negDir
    Test-Refusal -Name 'dns UTF-8 BOM' -Arguments $dnsArgs -Stdin ([byte[]](0xEF, 0xBB, 0xBF) + (Get-Ascii $token)) `
        -Code 'dns-token-forbidden-byte' -Directory $negDir -Secrets $secrets

    # ---- negative Tunnel -----------------------------------------------------------------
    $tArgs = @('-Mode', 'Tunnel', '-TargetDirectory', $negDir, '-FromStdin') + $tunnelArgs
    $badSecret42 = $secret.Substring(0, 42) + 'B='
    $tunnelCases = @(
        @{ Name = 'tunnel > 2048 bytes'; Value = $json + (' ' * 2100); Code = 'tunnel-json-too-long' },
        @{ Name = 'tunnel wrong-case tunnelId'; Value = '{"AccountTag":"' + $account + '","TunnelSecret":"' + $secret + '","tunnelId":"' + $tunnel + '"}'; Code = 'tunnel-json-unknown-field' },
        @{ Name = 'tunnel wrong-case accountTag'; Value = '{"accountTag":"' + $account + '","TunnelSecret":"' + $secret + '","TunnelID":"' + $tunnel + '"}'; Code = 'tunnel-json-unknown-field' },
        @{ Name = 'tunnel unknown field'; Value = (New-TunnelJson $account $secret $tunnel ',"Extra":"x"'); Code = 'tunnel-json-unknown-field' },
        @{ Name = 'tunnel duplicate key'; Value = (New-TunnelJson $account $secret $tunnel (',"TunnelID":"' + $tunnel + '"')); Code = 'tunnel-json-duplicate-field' },
        @{ Name = 'tunnel duplicate empty Endpoint'; Value = (New-TunnelJson $account $secret $tunnel ',"Endpoint":"","Endpoint":""'); Code = 'tunnel-json-duplicate-field' },
        @{ Name = 'tunnel non-empty Endpoint'; Value = (New-TunnelJson $account $secret $tunnel ',"Endpoint":"x"'); Code = 'tunnel-endpoint-not-empty' },
        @{ Name = 'tunnel null Endpoint'; Value = (New-TunnelJson $account $secret $tunnel ',"Endpoint":null'); Code = 'tunnel-json-non-string-value' },
        @{ Name = 'tunnel number value'; Value = '{"AccountTag":1,"TunnelSecret":"' + $secret + '","TunnelID":"' + $tunnel + '"}'; Code = 'tunnel-json-non-string-value' },
        @{ Name = 'tunnel escaped string'; Value = (New-TunnelJson $account $secret $tunnel ',"Endpoint":"\u0000"'); Code = 'tunnel-json-escape' },
        @{ Name = 'tunnel missing TunnelSecret'; Value = '{"AccountTag":"' + $account + '","TunnelID":"' + $tunnel + '"}'; Code = 'tunnel-json-missing-field' },
        @{ Name = 'tunnel trailing data'; Value = $json + '{}'; Code = 'tunnel-json-trailing-data' },
        @{ Name = 'tunnel trailing comma'; Value = (New-TunnelJson $account $secret $tunnel ','); Code = 'tunnel-json-malformed' },
        @{ Name = 'tunnel AccountTag mismatch'; Value = (New-TunnelJson (New-SyntheticHex) $secret $tunnel); Code = 'tunnel-account-tag-mismatch' },
        @{ Name = 'tunnel TunnelID mismatch'; Value = (New-TunnelJson $account $secret (New-SyntheticTunnelId)); Code = 'tunnel-id-mismatch' },
        @{ Name = 'tunnel uppercase AccountTag'; Value = (New-TunnelJson ('ABCDEF' + $account.Substring(6)) $secret $tunnel); Code = 'tunnel-account-tag-invalid' },
        @{ Name = 'tunnel uppercase TunnelID'; Value = (New-TunnelJson $account $secret $tunnel.ToUpperInvariant()); Code = 'tunnel-id-invalid' },
        @{ Name = 'tunnel secret bad 43rd char'; Value = (New-TunnelJson $account $badSecret42 $tunnel); Code = 'tunnel-secret-not-canonical' },
        @{ Name = 'tunnel secret 43 bytes'; Value = (New-TunnelJson $account $secret.Substring(0, 43) $tunnel); Code = 'tunnel-secret-not-canonical' },
        @{ Name = 'tunnel secret missing ='; Value = (New-TunnelJson $account ($secret.Substring(0, 43) + 'A') $tunnel); Code = 'tunnel-secret-not-canonical' },
        @{ Name = 'tunnel secret url-safe alphabet'; Value = (New-TunnelJson $account ('-' + $secret.Substring(1)) $tunnel); Code = 'tunnel-secret-not-canonical' }
    )
    foreach ($case in $tunnelCases) {
        Test-Refusal -Name $case.Name -Arguments $tArgs -Stdin (Get-Ascii $case.Value) -Code $case.Code -Directory $negDir -Secrets $secrets
    }
    Test-Refusal -Name 'tunnel UTF-8 BOM' -Arguments $tArgs -Stdin ([byte[]](0xEF, 0xBB, 0xBF) + (Get-Ascii $json)) `
        -Code 'tunnel-json-bom' -Directory $negDir -Secrets $secrets
    Test-Refusal -Name 'tunnel uppercase -AccountId' `
        -Arguments @('-Mode', 'Tunnel', '-TargetDirectory', $negDir, '-FromStdin', '-AccountId', $account.ToUpperInvariant(), '-TunnelId', $tunnel) `
        -Stdin (Get-Ascii $json) -Code 'account-id-invalid' -Directory $negDir -Secrets $secrets
    Test-Refusal -Name 'tunnel non-canonical -TunnelId' `
        -Arguments @('-Mode', 'Tunnel', '-TargetDirectory', $negDir, '-FromStdin', '-AccountId', $account, '-TunnelId', $tunnel.Replace('-', '')) `
        -Stdin (Get-Ascii $json) -Code 'tunnel-id-argument-invalid' -Directory $negDir -Secrets $secrets
    Test-Refusal -Name 'tunnel without identifiers' -Arguments @('-Mode', 'Tunnel', '-TargetDirectory', $negDir, '-FromStdin') `
        -Stdin (Get-Ascii $json) -Code 'tunnel-identifiers-required' -Directory $negDir -Secrets $secrets

    # ---- input-channel rules ---------------------------------------------------------------
    Test-Refusal -Name 'stdin with -Mode Both' -Arguments (@('-Mode', 'Both', '-TargetDirectory', $negDir, '-FromStdin') + $tunnelArgs) `
        -Stdin (Get-Ascii $token) -Code 'stdin-single-leaf-only' -Directory $negDir -Secrets $secrets
    Test-Refusal -Name 'redirected stdin without -FromStdin' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $negDir) `
        -Stdin (Get-Ascii $token) -Code 'stdin-redirected-without-fromstdin' -Directory $negDir -Secrets $secrets
    $paramNames = (Get-Command $sealer).Parameters.Keys
    Write-Case 'sealer exposes no secret-bearing parameter' (
        @($paramNames | Where-Object { $_ -match 'token|secret|credential|json|password|value' }).Count -eq 0)

    $transcriptDir = Join-Path $root 'transcript'
    New-PrivateDirectory $transcriptDir
    $transcriptFile = Join-Path $transcriptDir 'session.txt'
    $command = "Start-Transcript -LiteralPath '$transcriptFile' | Out-Null; " +
        "& '$sealer' -Mode Dns -TargetDirectory '$transcriptDir' -FromStdin; `$code = `$LASTEXITCODE; " +
        "Stop-Transcript | Out-Null; exit `$code"
    $result = Invoke-Sealer -Command $command -Stdin (Get-Ascii $token)
    $transcriptText = if (Test-Path -LiteralPath $transcriptFile) { [IO.File]::ReadAllText($transcriptFile) } else { '' }
    Write-Case 'active transcript -> transcript-active' (
        $result.Exit -eq 2 -and $result.Err.Contains('refused: transcript-active') -and @(Get-Leaves $transcriptDir).Count -eq 0 `
            -and -not $transcriptText.Contains($token) -and (Test-Clean $result $secrets))

    # ---- custody and path fence --------------------------------------------------------------
    $outsideTemp = 'D:\Temp\arx-cf-sealer\outside-' + [guid]::NewGuid().ToString('N')
    foreach ($case in @(
            @{ Name = 'path under D:\Temp'; Path = $outsideTemp; Code = 'path-outside-arxcovodata' },
            @{ Name = 'path is D:\ArxcovoData itself'; Path = 'D:\ArxcovoData'; Code = 'path-outside-arxcovodata' },
            @{ Name = 'path with lookalike root'; Path = 'D:\ArxcovoDataX\secrets'; Code = 'path-outside-arxcovodata' },
            @{ Name = 'path on C:'; Path = 'C:\ArxcovoData\secrets'; Code = 'path-outside-arxcovodata' },
            @{ Name = 'relative path'; Path = 'ArxcovoData\secrets'; Code = 'path-invalid' },
            @{ Name = 'UNC path'; Path = '\\localhost\D$\ArxcovoData\secrets'; Code = 'path-invalid' },
            @{ Name = 'traversal component'; Path = "$negDir\..\pos-dns"; Code = 'path-invalid' },
            @{ Name = 'ADS component'; Path = "${negDir}:stream"; Code = 'path-invalid' },
            @{ Name = 'ADS subdirectory'; Path = "$negDir\leaf:stream"; Code = 'path-invalid' },
            @{ Name = 'short-name component'; Path = 'D:\ArxcovoData\TMP-SE~1'; Code = 'path-invalid' },
            @{ Name = 'trailing separator'; Path = "$negDir\"; Code = 'path-invalid' },
            @{ Name = 'reserved device name'; Path = "$root\NUL"; Code = 'path-invalid' })) {
        Test-Refusal -Name $case.Name -Arguments @('-Mode', 'Dns', '-TargetDirectory', $case.Path, '-CreateDirectory', '-FromStdin') `
            -Stdin (Get-Ascii $token) -Code $case.Code -Secrets $secrets
    }
    Write-Case 'D:\Temp target was not created' (-not (Test-Path -LiteralPath $outsideTemp))

    Test-Refusal -Name 'missing target directory' -Arguments @('-Mode', 'Dns', '-TargetDirectory', (Join-Path $root 'absent'), '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'target-directory-missing' -Secrets $secrets
    Test-Refusal -Name 'missing parent with -CreateDirectory' `
        -Arguments @('-Mode', 'Dns', '-TargetDirectory', (Join-Path $root 'absent\child'), '-CreateDirectory', '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'ancestor-missing' -Secrets $secrets
    Write-Case 'missing parent was not created' (-not (Test-Path -LiteralPath (Join-Path $root 'absent')))

    $inherited = Join-Path $root 'inherited'
    $null = New-Item -ItemType Directory -Path $inherited
    Test-Refusal -Name 'inherited-ACL directory' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $inherited, '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'target-directory-not-private' -Directory $inherited -Secrets $secrets
    Test-Refusal -Name 'inherited-ACL directory with -CreateDirectory' `
        -Arguments @('-Mode', 'Dns', '-TargetDirectory', $inherited, '-CreateDirectory', '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'target-directory-not-private' -Directory $inherited -Secrets $secrets

    $shared = Join-Path $root 'shared'
    New-PrivateDirectory $shared '(A;OICI;FR;;;WD)'
    Test-Refusal -Name 'world-readable protected directory' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $shared, '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'target-directory-not-private' -Directory $shared -Secrets $secrets

    # A parent without inheritable entries, so the child's only entry is explicit and
    # owner-only; the child is refused solely because its DACL is not protected.
    $plainParent = Join-Path $root 'no-inherit-parent'
    New-PrivateDirectory $plainParent -AceFlags ''
    $unprotected = Join-Path $plainParent 'unprotected'
    New-PrivateDirectory $unprotected -DaclFlags ''
    $unprotectedRules = @((Get-Acl -LiteralPath $unprotected).GetAccessRules($true, $true, [Security.Principal.SecurityIdentifier]))
    Write-Case 'unprotected fixture has one explicit owner-only entry' (
        -not (Get-Acl -LiteralPath $unprotected).AreAccessRulesProtected -and $unprotectedRules.Count -eq 1 `
            -and $unprotectedRules[0].IdentityReference.Equals($userSid) -and -not $unprotectedRules[0].IsInherited)
    Test-Refusal -Name 'owner-only directory with inheritance enabled' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $unprotected, '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'target-directory-not-private' -Directory $unprotected -Secrets $secrets

    $junctionTarget = Join-Path $root 'junction-target'
    New-PrivateDirectory $junctionTarget
    New-PrivateDirectory (Join-Path $junctionTarget 'inner')
    $junction = Join-Path $root 'junction'
    $null = New-Item -ItemType Junction -Path $junction -Target $junctionTarget
    $junctions.Add($junction)
    Test-Refusal -Name 'junction ancestor' -Arguments @('-Mode', 'Dns', '-TargetDirectory', (Join-Path $junction 'inner'), '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'reparse-point-in-path' -Directory (Join-Path $junctionTarget 'inner') -Secrets $secrets
    Test-Refusal -Name 'junction target directory' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $junction, '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'reparse-point-in-path' -Directory $junctionTarget -Secrets $secrets

    $symlink = Join-Path $root 'symlink'
    $symlinkMade = $false
    try {
        $null = New-Item -ItemType SymbolicLink -Path $symlink -Target $junctionTarget -ErrorAction Stop
        $junctions.Add($symlink)
        $symlinkMade = $true
    } catch {
        Skip-Case 'directory symlink ancestor' 'creating symbolic links needs Developer Mode or SeCreateSymbolicLinkPrivilege'
    }
    if ($symlinkMade) {
        Test-Refusal -Name 'directory symlink ancestor' -Arguments @('-Mode', 'Dns', '-TargetDirectory', (Join-Path $symlink 'inner'), '-FromStdin') `
            -Stdin (Get-Ascii $token) -Code 'reparse-point-in-path' -Directory (Join-Path $junctionTarget 'inner') -Secrets $secrets
    }

    $existingHash = (Get-FileHash -LiteralPath (Join-Path $dnsDir 'dns.dpapi') -Algorithm SHA256).Hash
    Test-Refusal -Name 'existing dns.dpapi (no overwrite)' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $dnsDir, '-FromStdin') `
        -Stdin (Get-Ascii ('synthetic.other' + $token)) -Code 'target-file-exists' -Directory $dnsDir -LeavesBefore 1 -Secrets $secrets
    Write-Case 'existing dns.dpapi is unchanged' ((Get-FileHash -LiteralPath (Join-Path $dnsDir 'dns.dpapi') -Algorithm SHA256).Hash -eq $existingHash)
    Test-Refusal -Name 'existing tunnel.dpapi (no overwrite)' `
        -Arguments (@('-Mode', 'Tunnel', '-TargetDirectory', $tunnelDir, '-FromStdin') + $tunnelArgs) `
        -Stdin (Get-Ascii $json) -Code 'target-file-exists' -Directory $tunnelDir -LeavesBefore 1 -Secrets $secrets

    $linkDir = Join-Path $root 'hardlink'
    New-PrivateDirectory $linkDir
    $linkSource = Join-Path $linkDir 'source.bin'
    [IO.File]::WriteAllBytes($linkSource, [byte[]](1, 2, 3))
    $null = New-Item -ItemType HardLink -Path (Join-Path $linkDir 'dns.dpapi') -Target $linkSource
    Test-Refusal -Name 'pre-placed hard link at leaf name' -Arguments @('-Mode', 'Dns', '-TargetDirectory', $linkDir, '-FromStdin') `
        -Stdin (Get-Ascii $token) -Code 'target-file-exists' -Directory $linkDir -LeavesBefore 1 -Secrets $secrets
    Write-Case 'hard-link source untouched' ([Linq.Enumerable]::SequenceEqual([IO.File]::ReadAllBytes($linkSource), [byte[]](1, 2, 3)))

    Write-Case 'negative cases left no leaves in the shared negative directory' (@(Get-Leaves $negDir).Count -eq 0)
} finally {
    foreach ($link in $junctions) {
        if (Test-Path -LiteralPath $link) { [IO.Directory]::Delete($link, $false) }
    }
    if (Test-Path -LiteralPath $root) { Remove-Item -LiteralPath $root -Recurse -Force }
}

$after = @(Get-ChildItem -LiteralPath 'D:\ArxcovoData' -Directory -Filter 'tmp-seal-test-*').Count
Write-Case 'cleanup removed the test directory' (-not (Test-Path -LiteralPath $root) -and $after -eq $before)
Write-Output "tmp-seal-test-* directories under D:\ArxcovoData after cleanup: $after"
Write-Output "Summary: $script:passed passed, $script:failed failed, $script:skipped skipped"
if ($script:failed -ne 0) { exit 1 }
