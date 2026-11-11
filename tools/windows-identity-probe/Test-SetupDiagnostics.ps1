# Safe deterministic feedback loop: injected failures only; never invokes disposable entrypoint.
$ErrorActionPreference='Stop'
Import-Module "$PSScriptRoot/CiValidation.psm1" -Force
$count=0
function Check($Condition) { if (!$Condition) { throw 'setup-regression-failed' }; $script:count++ }
function Refused([scriptblock]$Action) {
    try { & $Action; throw 'accepted-unsafe-path' } catch { if ($_.Exception.Message -eq 'accepted-unsafe-path') { throw }; $script:count++ }
}
# This is the real scheduled-action path guard, exercised without COM or scheduling.
Assert-ProbeTrustedShellPath 'C:\Program Files\PowerShell\7\pwsh.EXE'
Check $true
foreach ($path in @('pwsh.exe','C:Program Files\PowerShell\7\pwsh.exe','\\server\share\pwsh.exe','C:\Program Files\PowerShell\7\..\7\pwsh.exe','C:\Program Files\PowerShell\7\pwsh.exe:stream','C:\Program Files\PowerShell\7\pwsh.exe.other','C:\untrusted\pwsh.exe')) { Refused { Assert-ProbeTrustedShellPath $path } }
# Use the production catch body, not a copied implementation, to replay the observed missing diagnostic.
$tokens=$null; $errors=$null
$ast=[Management.Automation.Language.Parser]::ParseFile("$PSScriptRoot/Invoke-DisposableCi.ps1",[ref]$tokens,[ref]$errors)
Check ($errors.Count -eq 0)
$guards=@($ast.FindAll({param($n) $n -is [Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Assert-ProbeTrustedShellPath'},$true))
Check ($guards.Count -eq 1)
$action=@{Path='C:\Program Files\PowerShell\7\pwsh.EXE'}
& ([scriptblock]::Create($guards[0].Extent.Text))
Check $true
$tries=@($ast.FindAll({param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $n.Body.Extent.Text.Contains('$success = $true')},$true))
Check ($tries.Count -eq 1)
$body=$tries[0].CatchClauses[0].Body.Extent.Text
$handler=[scriptblock]::Create($body.Substring(1,$body.Length-2))
$private='credential=PRIVATE; account=PRIVATE; C:\PRIVATE; InvocationInfo PRIVATE'
foreach ($case in @(
    @{phase='task-path';exception=[UnauthorizedAccessException]::new($private);category='access-denied'},
    @{phase='task-registration';exception=[Runtime.InteropServices.COMException]::new($private,-2147024891);category='com'},
    @{phase='root';exception=[IO.IOException]::new($private);category='io'},
    @{phase='payload';exception=[InvalidOperationException]::new($private);category='other'},
    @{phase=($private * 10000);exception=[Exception]::new($private);category='other'}
)) {
    $phase=$case.phase
    try { throw $case.exception } catch { $rows=@(& $handler) }
    Check ($rows.Count -eq 2)
    Check (($rows -join '').Length -le 300 -and !($rows -join '').Contains('PRIVATE'))
    $diag=$rows[0] | ConvertFrom-Json -AsHashtable
    Check (($diag.Keys | Sort-Object) -join ',' -ceq 'category,hresult,phase,stage')
    Check ($diag.stage -ceq 'setup-diagnostic' -and $diag.category -ceq $case.category)
    Check ($diag.hresult -is [long] -or $diag.hresult -is [int])
    Check ($diag.hresult -eq $case.exception.HResult)
    Check ($diag.phase -ceq $(if ($case.phase.Length -gt 100) { 'unknown' } else { $case.phase }))
    Check ($rows[1] -ceq '{"stage":"ci-experiment","ok":false}')
}
# Replay each actual phase assignment through the catch, so a missing/typo phase fails.
$phases=@($ast.FindAll({param($n) $n -is [Management.Automation.Language.AssignmentStatementAst] -and $n.Left.Extent.Text -ceq '$phase'},$true))
Check ($phases.Count -ge 13)
Check ($phases[0].Extent.Text.Contains("'root'"))
foreach ($assignment in $phases) {
    . ([scriptblock]::Create($assignment.Extent.Text))
    try { throw [Exception]::new($private) } catch { $rows=@(& $handler) }
    Check (($rows[0] | ConvertFrom-Json).phase -ceq $phase)
}
foreach ($code in @([int]::MinValue,0,[int]::MaxValue)) {
    $failure=[Management.Automation.ErrorRecord]::new([Runtime.InteropServices.COMException]::new($private,$code),$private,[Management.Automation.ErrorCategory]::NotSpecified,$private)
    $text=Get-ProbeSetupDiagnostic 'preflight' $failure
    Check ($text.Length -le 256 -and !$text.Contains('PRIVATE'))
    Check (($text | ConvertFrom-Json).hresult -eq $code)
}
Write-Output ('{"stage":"setup-self-test","ok":true,"assertions":'+$count+',"privilegedPathsRun":false}')
