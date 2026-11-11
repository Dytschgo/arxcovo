# Each mutant runs the full evidence/parser suite on a private copy; nothing privileged is invoked.
$ErrorActionPreference='Stop'
$source=Get-Content -Raw "$PSScriptRoot/CiValidation.psm1"
$entrySource=Get-Content -Raw "$PSScriptRoot/Invoke-DisposableCi.ps1"
$mutants=[ordered]@{
    'stage-sid'='$j.observedSid -ceq $Sid -and'
    'integrity-rid'=' $j.integrityRid -eq 8192 -and'
    'medium-flag'=' -and $j.mediumIntegrity -ceq $true'
    'child-exit'='Assert-Probe ($j.exitCode -eq 42);'
    'reparse-ancestor'='Assert-Probe (((Get-Item -Force -LiteralPath $part.FullName).Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)'
    'trusted-shell-case'='[StringComparison]::OrdinalIgnoreCase'
    'setup-phase'='$Phase -cin $allowed'
    'setup-category'='category=$category'
    'setup-hresult'='hresult=[int]$exception.HResult'
    'setup-private-text'="stage='setup-diagnostic';"
    'setup-missing-diagnostic'='Write-Output (Get-ProbeSetupDiagnostic $phase $_); '
}
$tempBase=[IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$root=Join-Path $tempBase ('arx-mutations-'+[guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($root)
try {
    foreach ($file in @('Test-CiValidation.ps1','Test-SetupDiagnostics.ps1','Invoke-DisposableCi.ps1')) { Copy-Item -LiteralPath "$PSScriptRoot/$file" -Destination $root }
    foreach ($name in $mutants.Keys) {
        $guard=$mutants[$name]
        $targetSource=if ($name -eq 'setup-missing-diagnostic') { $entrySource } else { $source }
        if (($targetSource.Split([string[]]@($guard),[StringSplitOptions]::None)).Count -ne 2) { throw 'mutation-target-count' }
        $replacement=if ($name -eq 'stage-sid') { ' ' } elseif ($name -eq 'reparse-ancestor') { 'Assert-Probe $true' } else { '' }
        switch ($name) {
            'trusted-shell-case' { $replacement='[StringComparison]::Ordinal' }
            'setup-phase' { $replacement='$true' }
            'setup-category' { $replacement='category=$exception.Message' }
            'setup-hresult' { $replacement='hresult=0' }
            'setup-private-text' { $replacement="stage='setup-diagnostic';detail=`$exception.Message;" }
        }
        [IO.File]::WriteAllText("$root/CiValidation.psm1", $source)
        [IO.File]::WriteAllText("$root/Invoke-DisposableCi.ps1", $entrySource)
        $targetFile=if ($name -eq 'setup-missing-diagnostic') { 'Invoke-DisposableCi.ps1' } else { 'CiValidation.psm1' }
        [IO.File]::WriteAllText("$root/$targetFile", $targetSource.Replace($guard,$replacement))
        $output=& (Get-Process -Id $PID).Path -NoProfile -File "$root/Test-CiValidation.ps1" 2>&1 | Out-String
        $expected=if ($name -eq 'trusted-shell-case') { 'probe-validation-refused' } elseif ($name.StartsWith('setup-')) { 'setup-regression-failed' } else { 'expected-refusal-missing' }
        if ($LASTEXITCODE -eq 0 -or !$output.Contains($expected)) { throw "mutation-not-killed-as-expected: $name" }
        Write-Output (ConvertTo-Json @{stage='guard-mutation';guard=$name;killed=$true} -Compress)
    }
} finally {
    # Exact generated directory only; no recursive deletion or wildcard traversal.
    if (![IO.Path]::GetFullPath($root).StartsWith($tempBase,[StringComparison]::OrdinalIgnoreCase)) { throw 'cleanup-path-refused' }
    foreach ($file in @('Test-CiValidation.ps1','Test-SetupDiagnostics.ps1','Invoke-DisposableCi.ps1','CiValidation.psm1')) {
        if (Test-Path -LiteralPath "$root/$file") { Remove-Item -LiteralPath "$root/$file" }
    }
    [IO.Directory]::Delete($root)
}
