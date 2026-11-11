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
