# Safe on owner PC: no import/invocation of the mutation entrypoint or native API.
$ErrorActionPreference = 'Stop'
Import-Module "$PSScriptRoot/CiValidation.psm1" -Force
$script:count = 0
function Accept([scriptblock]$Test) { & $Test; $script:count++ }
function Reject([scriptblock]$Test) {
    $refused = $false
    try { & $Test | Out-Null } catch { $refused=$true }
    if (!$refused) { throw 'expected-refusal-missing' }
    $script:count++
}
$c = @{
    GITHUB_ACTIONS='true'; RUNNER_ENVIRONMENT='github-hosted'; RUNNER_OS='Windows'
    ARXCOVO_DISPOSABLE_PROBE='reviewed-manual-ci-v1'; GITHUB_EVENT_NAME='workflow_dispatch'
    GITHUB_REPOSITORY='Dytschgo/arxcovo'; GITHUB_ACTOR='Dytschgo'; GITHUB_TRIGGERING_ACTOR='Dytschgo'
    GITHUB_REF='refs/heads/main'; GITHUB_RUN_ID='123'; GITHUB_RUN_ATTEMPT='1'
    GITHUB_SHA=('a'*40); GITHUB_WORKFLOW_SHA=('a'*40); REVIEWED_SHA=('a'*40)
}
Accept { Assert-ProbeContext $c }
foreach ($key in @($c.Keys)) {
    $copy=$c.Clone(); $copy[$key]=''
    Reject { Assert-ProbeContext $copy }
    $copy[$key]='unexpected'
    Reject { Assert-ProbeContext $copy }
}
foreach ($key in @('GITHUB_SHA','GITHUB_WORKFLOW_SHA')) { $copy=$c.Clone(); $copy[$key]='b'*40; Reject { Assert-ProbeContext $copy } }
Accept { $p=Get-ProbePlan '123' '1' 'axp0123abcd'; Assert-Probe ($p.Root -ceq 'C:\arxcovo-probe\123-1' -and $p.Task -ceq 'ArxcovoProbe-123-1') }
foreach ($bad in @('Administrator','axp0123abcD','axp0123abcd\x',"axp0123abcd`n",'')) { Reject { Get-ProbePlan '123' '1' $bad } }
Reject { Get-ProbePlan '../1' '1' 'axp0123abcd' }
Reject { Get-ProbePlan '1' ('1'*6) 'axp0123abcd' }
Reject { Assert-ProbePath 'C:\arxcovo-probe\123-1-other' 'C:\arxcovo-probe\123-1' }
Reject { Assert-ProbePath 'C:\arxcovo-probe\123-1\..\work' 'C:\arxcovo-probe\123-1' }
$s=@{ Sid='S-1-5-21-1-2-3-1001'; AccountCreated=$true; TaskCreated=$true; RightCreated=$true; PathCreated=$true }
Accept { Assert-Probe (((Get-ProbeCleanupPlan $s $s.Sid) -join ',') -ceq 'task,right,account,path') }
foreach ($flag in @('TaskCreated','RightCreated','PathCreated')) {
    $copy=$s.Clone(); $copy[$flag]=$false
    Accept { Assert-Probe (@(Get-ProbeCleanupPlan $copy $copy.Sid).Count -eq 3) }
}
Reject { Get-ProbeCleanupPlan $s 'S-1-5-21-1-2-3-1002' }
$copy=$s.Clone(); $copy.AccountCreated=$false; Reject { Get-ProbeCleanupPlan $copy $copy.Sid }
$copy=$s.Clone(); $copy.Sid='S-1-5-18'; Reject { Get-ProbeCleanupPlan $copy $copy.Sid }
$sid=$s.Sid
$rows=@(
    @{stage='preflight';ok=$true;expectedSid=$sid}
    @{stage='logon-token';observedSid=$sid;integrityRid=8192;mediumIntegrity=$true}
    @{stage='suspended-child';observedSid=$sid;integrityRid=8192;mediumIntegrity=$true}
    @{stage='descendant';observedSid=$sid;integrityRid=8192;mediumIntegrity=$true}
    @{stage='child-exit';exitCode=42}
    @{stage='bounded-tree-probe';ok=$true;descendantExitCode=124;liveJobProcesses=0;productionAcceptance=$false}
)
function Json($Rows) { return (($Rows | ForEach-Object { ConvertTo-Json $_ -Compress }) -join "`n") }
$json=Json $rows
Accept { Assert-Probe ((Read-ProbeEvidence $json $sid | ConvertFrom-Json).ok) }
foreach ($i in 0..5) { Reject { Read-ProbeEvidence (Json @($rows | Where-Object { $_ -ne $rows[$i] })) $sid } }
foreach ($change in @(@{descendantExitCode=44},@{liveJobProcesses=1},@{ok=$false},@{productionAcceptance=$true})) {
    $changed=$rows[-1].Clone(); foreach ($k in $change.Keys) { $changed[$k]=$change[$k] }
    Reject { Read-ProbeEvidence (Json (@($rows[0..4]) + @($changed))) $sid }
}
foreach ($field in @('ok','descendantExitCode','liveJobProcesses','productionAcceptance')) {
    foreach ($badValue in @('0','True',$null)) {
        $changed=$rows[-1].Clone(); $changed[$field]=$badValue
        Reject { Read-ProbeEvidence (Json (@($rows[0..4]) + @($changed))) $sid }
    }
}
Reject { Read-ProbeEvidence $json 'S-1-5-21-1-2-3-1002' }
foreach ($bad in @('', 'not-json', ('x'*262145), (("{} `n")*129), ('x'*2049), ($json+"`n"+'{"stage":"failure"}'),($json+"`n"+'{"stage":"untrusted"}'))) { Reject { Read-ProbeEvidence $bad $sid } }
# Isolate every stage/field while all other evidence remains valid.
foreach ($stageIndex in 1..3) {
    foreach ($change in @(@{observedSid='S-1-5-21-1-2-3-1002'}, @{integrityRid=4096}, @{integrityRid=12288}, @{integrityRid=16384}, @{mediumIntegrity=$false})) {
        $copy=@($rows | ForEach-Object { $_.Clone() })
        foreach ($key in $change.Keys) { $copy[$stageIndex][$key]=$change[$key] }
        Reject { Read-ProbeEvidence (Json $copy) $sid }
    }
}
foreach ($code in @(1,44)) {
    $copy=@($rows | ForEach-Object { $_.Clone() }); $copy[4].exitCode=$code
    Reject { Read-ProbeEvidence (Json $copy) $sid }
}
# Real unprivileged junction ancestor; remove link nonrecursively before target.
$tempRoot=Join-Path ([IO.Path]::GetTempPath()) ('arx-guards-'+[guid]::NewGuid().ToString('N'))
try {
    [void][IO.Directory]::CreateDirectory("$tempRoot\target\child")
    [void](New-Item -ItemType Junction -Path "$tempRoot\link" -Target "$tempRoot\target")
    Accept { Assert-ProbePath "$tempRoot\target\child" $tempRoot }
    Reject { Assert-ProbePath "$tempRoot\link\child" $tempRoot }
} finally {
    if (Test-Path -LiteralPath "$tempRoot\link") { [IO.Directory]::Delete("$tempRoot\link") }
    if (Test-Path -LiteralPath "$tempRoot\target\child") { [IO.Directory]::Delete("$tempRoot\target\child") }
    if (Test-Path -LiteralPath "$tempRoot\target") { [IO.Directory]::Delete("$tempRoot\target") }
    if (Test-Path -LiteralPath $tempRoot) { [IO.Directory]::Delete($tempRoot) }
}
Accept {
    $acl=Get-ProbeDirectoryAcl $sid 'ReadAttributes,Traverse' 'None'
    $rules=@($acl.GetAccessRules($true,$false,[Security.Principal.SecurityIdentifier]) | Where-Object { $_.IdentityReference.Value -ceq $sid })
    Assert-Probe ($acl.AreAccessRulesProtected -and $rules.Count -eq 1)
    Assert-Probe ($rules[0].InheritanceFlags -eq 0 -and $rules[0].FileSystemRights -eq ([Security.AccessControl.FileSystemRights]::ReadAttributes -bor [Security.AccessControl.FileSystemRights]::Traverse -bor [Security.AccessControl.FileSystemRights]::Synchronize))
}
$failedEvidence=Json @(@{stage='lsa';api='LsaLogonUser-MSV1_0-S4U-Batch';status=3221225581L;subStatus=3221225819L;win32=1326;reason='DO-NOT-PUBLISH'}, @{stage='child-exit';exitCode=3221225794L;payloadNativeError=1314}, @{stage='failure';reason='DO-NOT-PUBLISH'})
Accept {
    $diagnostics=@(Get-ProbeDiagnostics $failedEvidence)
    Assert-Probe ($diagnostics.Count -eq 2 -and !($diagnostics -join '').Contains('DO-NOT-PUBLISH'))
    $native=$diagnostics[0] | ConvertFrom-Json
    Assert-Probe ($native.status -eq 3221225581L -and $native.subStatus -eq 3221225819L -and $native.win32 -eq 1326)
    Assert-Probe (($diagnostics[1] | ConvertFrom-Json).exitCode -eq 3221225794L)
}
Reject { Read-ProbeEvidence $failedEvidence $sid }
foreach ($bad in @('{"stage":"win32","api":"private text","error":5}', '{"stage":"win32","api":"GetTokenInformation","error":"5"}', '{"stage":"win32","api":"GetTokenInformation","error":-1}', '{"stage":"win32","api":"GetTokenInformation","error":4294967296}', 'not-json', ('x'*262145))) { Reject { Get-ProbeDiagnostics $bad } }
# Exercise the exact task read block with ordinary fixed-output child processes.
$entry=Get-Content -Raw "$PSScriptRoot/Invoke-DisposableCi.ps1"
$begin=$entry.IndexOf('        $buffer = [char[]]::new(131073)')
$end=$entry.IndexOf('        exit $p.ExitCode', $begin)
Assert-Probe ($begin -ge 0 -and $end -gt $begin)
$readCode=$entry.Substring($begin,$end-$begin).Replace("'__ROOT__\result.jsonl'", '$resultFile').Replace('20000','1500')
$reader=[scriptblock]::Create($readCode)
foreach ($case in @(@{code='[Console]::Write("fixed-output")';pass=$true}, @{code='[Console]::Write("x" * 131074)';pass=$false;reason='bounded-output-or-time'}, @{code='Start-Sleep -Seconds 10';pass=$false;reason='output-timeout'})) {
    $start=[Diagnostics.ProcessStartInfo]::new((Get-Process -Id $PID).Path)
    $start.UseShellExecute=$false; $start.RedirectStandardOutput=$true; $start.CreateNoWindow=$true
    foreach ($arg in @('-NoProfile','-Command',$case.code)) { $start.ArgumentList.Add($arg) }
    $resultFile=Join-Path ([IO.Path]::GetTempPath()) ('arx-read-'+[guid]::NewGuid().ToString('N'))
    $p=[Diagnostics.Process]::Start($start)
    try {
        if ($case.pass) { Accept { & $reader; Assert-Probe ([IO.File]::ReadAllText($resultFile) -ceq 'fixed-output') } }
        else {
            $reason=''
            try { & $reader } catch { $reason=$_.Exception.Message }
            Accept { Assert-Probe ($reason -ceq $case.reason) }
        }
    } finally {
        if (!$p.HasExited) { $p.Kill($true); [void]$p.WaitForExit(5000) }; $p.Dispose()
        if (Test-Path -LiteralPath $resultFile) { Remove-Item -LiteralPath $resultFile }
    }
}
# Parse all scripts, including the nested protected-task template, without executing them.
foreach ($file in Get-ChildItem $PSScriptRoot -Include *.ps1,*.psm1 -File -Recurse | Where-Object { $_.FullName -notmatch '\\(bin|obj)\\' }) {
    $tokens=$null; $errors=$null
    $ast=[Management.Automation.Language.Parser]::ParseFile($file.FullName,[ref]$tokens,[ref]$errors)
    Assert-Probe ($errors.Count -eq 0)
    foreach ($node in $ast.FindAll({param($n) $n -is [Management.Automation.Language.StringConstantExpressionAst] -and $n.Value.Contains('$start = [Diagnostics.ProcessStartInfo]')},$true)) {
        [void][Management.Automation.Language.Parser]::ParseInput($node.Value,[ref]$tokens,[ref]$errors)
        Assert-Probe ($errors.Count -eq 0)
    }
    $script:count++
}
Write-Output ('{"stage":"helper-self-test","ok":true,"assertions":' + $count + ',"privilegedPathsRun":false}')
