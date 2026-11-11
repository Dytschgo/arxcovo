# Each mutant runs the full evidence/parser suite on a private copy; nothing privileged is invoked.
$ErrorActionPreference='Stop'
$source=Get-Content -Raw "$PSScriptRoot/CiValidation.psm1"
$mutants=[ordered]@{
    'stage-sid'='$j.observedSid -ceq $Sid -and'
    'integrity-rid'=' $j.integrityRid -eq 8192 -and'
    'medium-flag'=' -and $j.mediumIntegrity -ceq $true'
    'child-exit'='Assert-Probe ($j.exitCode -eq 42);'
    'reparse-ancestor'='Assert-Probe (((Get-Item -Force -LiteralPath $part.FullName).Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)'
}
$tempBase=[IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$root=Join-Path $tempBase ('arx-mutations-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
try {
    foreach ($file in @('Test-CiValidation.ps1','Invoke-DisposableCi.ps1')) { Copy-Item -LiteralPath "$PSScriptRoot/$file" -Destination $root }
    foreach ($name in $mutants.Keys) {
        $guard=$mutants[$name]
        if (($source.Split([string[]]@($guard),[StringSplitOptions]::None)).Count -ne 2) { throw 'mutation-target-count' }
        $replacement=if ($name -eq 'stage-sid') { ' ' } elseif ($name -eq 'reparse-ancestor') { 'Assert-Probe $true' } else { '' }
        [IO.File]::WriteAllText("$root/CiValidation.psm1", $source.Replace($guard,$replacement))
        $output=& (Get-Process -Id $PID).Path -NoProfile -File "$root/Test-CiValidation.ps1" 2>&1 | Out-String
        if ($LASTEXITCODE -eq 0 -or !$output.Contains('expected-refusal-missing')) { throw "mutation-not-killed-as-expected: $name" }
        Write-Output (ConvertTo-Json @{stage='guard-mutation';guard=$name;killed=$true} -Compress)
    }
} finally {
    # Exact generated directory only; no recursive deletion or wildcard traversal.
    if (![IO.Path]::GetFullPath($root).StartsWith($tempBase,[StringComparison]::OrdinalIgnoreCase)) { throw 'cleanup-path-refused' }
    foreach ($file in @('Test-CiValidation.ps1','Invoke-DisposableCi.ps1','CiValidation.psm1')) {
        if (Test-Path -LiteralPath "$root/$file") { Remove-Item -LiteralPath "$root/$file" }
    }
    [IO.Directory]::Delete($root)
}
