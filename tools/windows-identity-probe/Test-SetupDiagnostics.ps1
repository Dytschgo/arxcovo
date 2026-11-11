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
$moduleAst=[Management.Automation.Language.Parser]::ParseFile("$PSScriptRoot/CiValidation.psm1",[ref]$tokens,[ref]$errors)
$shellGuard=@($moduleAst.FindAll({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq 'Assert-ProbeTrustedShellPath'},$true))
Check ($shellGuard.Count -eq 1)
# Isolate the exact-string layer from the path/reparse layer. A spy confirms
# the latter is called with the original spelling, without normalizing it.
& {
    param($Definition)
    $calls=[Collections.Generic.List[string]]::new()
    function Assert-ProbePath([string]$Path,[string]$Root) { $calls.Add($Path); $calls.Add($Root) }
    . ([scriptblock]::Create($Definition))
    $valid='C:\Program Files\PowerShell\7\pwsh.EXE'
    Assert-ProbeTrustedShellPath $valid
    Check ($calls.Count -eq 2 -and $calls[0] -ceq $valid -and $calls[1] -ceq $valid)
    $calls.Clear()
    Refused { Assert-ProbeTrustedShellPath 'C:\Program Files\PowerShell\7\..\7\pwsh.exe' }
    Check ($calls.Count -eq 0)
} $shellGuard[0].Extent.Text
$preflight=@($ast.FindAll({param($n) $n -is [Management.Automation.Language.TryStatementAst] -and $n.Body.Extent.Text.Contains('$context = Get-ProbeContext')},$true))
Check ($preflight.Count -eq 1)
# Extract only preflight. The provisioning body is never placed in the child.
$module=(Join-Path $PSScriptRoot 'CiValidation.psm1').Replace("'","''")
$scriptText=@('$ErrorActionPreference=''Stop''',"Import-Module '$module' -Force",$preflight[0].Extent.Text,"Write-Output 'AFTER-PREFLIGHT'") -join "`n"
$start=[Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
$start.UseShellExecute=$false
$start.RedirectStandardOutput=$true
$start.RedirectStandardError=$true
foreach ($arg in @('-NoProfile','-NonInteractive','-EncodedCommand',[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($scriptText)))) { $start.ArgumentList.Add($arg) }
$start.Environment.Clear()
$start.Environment['SystemRoot']=[Environment]::GetFolderPath('Windows')
$start.Environment['GITHUB_ACTIONS']='false'
function Read-BoundedTestOutput($Reader) {
    $buffer=[char[]]::new(4097); $count=0
    while ($count -lt $buffer.Length) {
        $read=$Reader.ReadAsync($buffer,$count,$buffer.Length-$count)
        if (!$read.Wait(5000)) { throw 'setup-regression-failed' }
        $n=$read.Result
        if ($n -eq 0) { break }
        $count+=$n
    }
    Check ($count -le 4096)
    return [string]::new($buffer,0,$count)
}
$child=[Diagnostics.Process]::Start($start)
try {
    $stdout=Read-BoundedTestOutput $child.StandardOutput
    $stderr=Read-BoundedTestOutput $child.StandardError
    Check ($child.WaitForExit(5000))
    Check ($child.ExitCode -eq 1)
    Check ($stderr.Length -eq 0 -and !$stdout.Contains('AFTER-PREFLIGHT'))
    $rows=@($stdout -split '\r?\n' | Where-Object { $_.Length -gt 0 })
    Check ($rows.Count -eq 2)
    $diag=$rows[0] | ConvertFrom-Json -AsHashtable
    Check (($diag.Keys | Sort-Object) -join ',' -ceq 'category,hresult,phase,stage')
    Check ($diag.stage -ceq 'setup-diagnostic' -and $diag.phase -ceq 'preflight')
    Check ($rows[1] -ceq '{"stage":"ci-experiment","ok":false}')
} finally { if (!$child.HasExited) { $child.Kill($true); [void]$child.WaitForExit(5000) }; $child.Dispose() }
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
Check (($phases | ForEach-Object { $_.Right.Extent.Text.Trim("'") }) -join ',' -ceq 'root,account,groups,directory-acls,payload,batch-right,task-definition,task-path,task-registration,task-run,task-wait,result-read,result-validation')
Check ($phases[7].Extent.EndOffset -lt $guards[0].Extent.StartOffset -and $guards[0].Extent.EndOffset -lt $phases[8].Extent.StartOffset)
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
# Real static method invocation models PowerShell's wrapper rather than plain throw.
$missing=Join-Path ([IO.Path]::GetTempPath()) ('PRIVATE-'+[guid]::NewGuid().ToString('N'))
try { [void][IO.File]::ReadAllText($missing); throw 'expected-method-failure' } catch {
    Check ($_.Exception -is [Management.Automation.MethodInvocationException])
    $expectedCode=$_.Exception.InnerException.HResult
    $text=Get-ProbeSetupDiagnostic 'result-read' $_
    $diag=$text | ConvertFrom-Json
    Check ($diag.hresult -eq $expectedCode -and $diag.category -ceq 'io')
    Check (!$text.Contains('PRIVATE') -and $text.Length -le 256)
}
Write-Output ('{"stage":"setup-self-test","ok":true,"assertions":'+$count+',"privilegedPathsRun":false}')
