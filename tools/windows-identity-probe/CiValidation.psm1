# Pure decisions only. Importing this module performs no account/task/native actions.
Set-StrictMode -Version Latest
function Assert-Probe($Condition) { if (!$Condition) { throw 'probe-validation-refused' } }
function Assert-ProbeTrustedShellPath([string]$Path) {
    # Exact absolute trusted location; only Windows casing may differ. No normalization.
    Assert-Probe ([string]::Equals($Path, 'C:\Program Files\PowerShell\7\pwsh.exe', [StringComparison]::OrdinalIgnoreCase))
    Assert-ProbePath $Path $Path
}
function Get-ProbeSetupDiagnostic([string]$Phase, [Management.Automation.ErrorRecord]$Failure) {
    # Never inspect message, stack, InvocationInfo, target, or arbitrary error data.
    $allowed=@('preflight','root','account','groups','directory-acls','payload','batch-right','task-definition','task-path','task-registration','task-run','task-wait','result-read','result-validation')
    $safePhase=if ($Phase -cin $allowed) { $Phase } else { 'unknown' }
    $exception=$Failure.Exception
    $category=if ($exception -is [UnauthorizedAccessException]) { 'access-denied' }
        elseif ($exception -is [Runtime.InteropServices.COMException]) { 'com' }
        elseif ($exception -is [IO.IOException]) { 'io' } else { 'other' }
    return (ConvertTo-Json @{stage='setup-diagnostic';phase=$safePhase;hresult=[int]$exception.HResult;category=$category} -Compress)
}
function Assert-ProbeContext([hashtable]$Context) {
    $expected = @{
        GITHUB_ACTIONS='true'; RUNNER_ENVIRONMENT='github-hosted'; RUNNER_OS='Windows'
        ARXCOVO_DISPOSABLE_PROBE='reviewed-manual-ci-v1'; GITHUB_EVENT_NAME='workflow_dispatch'
        GITHUB_REPOSITORY='Dytschgo/arxcovo'; GITHUB_ACTOR='Dytschgo'; GITHUB_TRIGGERING_ACTOR='Dytschgo'
        GITHUB_REF='refs/heads/main'
    }
    foreach ($key in $expected.Keys) { Assert-Probe ($Context[$key] -ceq $expected[$key]) }
    Assert-Probe ($Context.GITHUB_RUN_ID -cmatch '\A[0-9]{1,20}\z')
    Assert-Probe ($Context.GITHUB_RUN_ATTEMPT -cmatch '\A[0-9]{1,5}\z')
    Assert-Probe ($Context.REVIEWED_SHA -cmatch '\A[0-9a-f]{40}\z')
    Assert-Probe ($Context.GITHUB_SHA -ceq $Context.REVIEWED_SHA)
    Assert-Probe ($Context.GITHUB_WORKFLOW_SHA -ceq $Context.REVIEWED_SHA)
}
function Get-ProbeContext {
    $c = @{}
    foreach ($k in @('GITHUB_ACTIONS','RUNNER_ENVIRONMENT','RUNNER_OS','ARXCOVO_DISPOSABLE_PROBE','GITHUB_EVENT_NAME','GITHUB_REPOSITORY','GITHUB_ACTOR','GITHUB_TRIGGERING_ACTOR','GITHUB_REF','GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT','GITHUB_SHA','GITHUB_WORKFLOW_SHA','REVIEWED_SHA')) {
        $c[$k] = [Environment]::GetEnvironmentVariable($k)
    }
    return $c
}
function Get-ProbePlan([string]$Run, [string]$Attempt, [string]$Account) {
    Assert-Probe ($Run -cmatch '\A[0-9]{1,20}\z' -and $Attempt -cmatch '\A[0-9]{1,5}\z')
    Assert-Probe ($Account -cmatch '\Aaxp[0-9a-f]{8}\z')
    return @{ Root="C:\arxcovo-probe\$Run-$Attempt"; Account=$Account; Task="ArxcovoProbe-$Run-$Attempt" }
}
function Assert-ProbePath([string]$Path, [string]$Root) {
    Assert-Probe ($Path -ceq $Root -or $Path.StartsWith($Root + '\', [StringComparison]::Ordinal))
    Assert-Probe ([IO.Path]::GetFullPath($Path) -ceq $Path)
    $part = [IO.DirectoryInfo]::new($Path)
    while ($null -ne $part) {
        if (Test-Path -LiteralPath $part.FullName) {
            Assert-Probe (((Get-Item -Force -LiteralPath $part.FullName).Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)
        }
        $part = $part.Parent
    }
}
function Get-ProbeDirectoryAcl([string]$Sid='', [string]$Rights='', [string]$Inheritance='None') {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($admin in @('S-1-5-18','S-1-5-32-544')) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($admin), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    }
    if ($Rights) {
        Assert-Probe ($Sid -cmatch '\AS-1-5-21-(?:[0-9]+-){3}[0-9]+\z')
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($Sid), $Rights, $Inheritance, 'None', 'Allow'))
    }
    return $acl
}
function Get-ProbeCleanupPlan($State, [string]$ObservedSid) {
    Assert-Probe ($State.Sid -cmatch '\AS-1-5-21-(?:[0-9]+-){3}[0-9]+\z')
    Assert-Probe ($ObservedSid -ceq $State.Sid)
    Assert-Probe ($State.AccountCreated)
    $steps = @()
    if ($State.TaskCreated) { $steps += 'task' }
    if ($State.RightCreated) { $steps += 'right' }
    $steps += 'account'
    if ($State.PathCreated) { $steps += 'path' }
    return $steps
}
function Get-ProbeDiagnostics([string]$Text) {
    Assert-Probe ([Text.Encoding]::UTF8.GetByteCount($Text) -le 262144)
    $lines=@($Text.TrimEnd("`r","`n") -split '\r?\n')
    Assert-Probe ($lines.Count -le 128)
    $apis=@('NetUserGetInfo','NetUserGetLocalGroups','LsaRegisterLogonProcess','LsaLookupAuthenticationPackage','LsaLogonUser-MSV1_0-S4U-Batch','CreateProcessW-leaf','CreateProcessAsUserW-suspended','QueryInformationJobObject','GetTokenInformation','OpenProcessToken','AllocateLocallyUniqueId','CreateJobObjectW','SetInformationJobObject','AssignProcessToJobObject','ResumeThread','ResumeThread-leaf','WaitForSingleObject-child','GetExitCodeProcess','OpenProcess-descendant','IsProcessInJob','TerminateJobObject','GetExitCodeProcess-descendant','cleanup-TerminateJobObject','cleanup-TerminateProcess','LookupPrivilegeValueW','PrivilegeCheck','LookupPrivilegeNameW')
    # Validate the complete bounded JSON stream before publishing any sanitized row.
    $out=@()
    foreach ($line in $lines) {
        Assert-Probe ($line.Length -le 2048)
        $row=ConvertFrom-Json -InputObject $line -AsHashtable -ErrorAction Stop
        Assert-Probe ($row -is [hashtable] -and $row.stage -is [string])
        if ($row.stage -cnotin @('win32','lsa','account','child-exit')) { continue }
        $safe=@{stage='probe-diagnostic';sourceStage=$row.stage}
        if ($row.stage -cne 'child-exit') {
            Assert-Probe ($row.api -cin $apis)
            $safe.api=$row.api
        }
        foreach ($key in @('status','subStatus','win32','subWin32','error','exitCode','payloadNativeError')) {
            if ($row.ContainsKey($key) -and $null -ne $row[$key]) {
                Assert-Probe (($row[$key] -is [int] -or $row[$key] -is [long]) -and $row[$key] -ge 0 -and $row[$key] -le 4294967295)
                $safe[$key]=$row[$key]
            }
        }
        $out+=ConvertTo-Json $safe -Compress
    }
    return $out
}
function Read-ProbeEvidence([string]$Text, [string]$Sid) {
    Assert-Probe ([Text.Encoding]::UTF8.GetByteCount($Text) -le 262144)
    $lines = @($Text.TrimEnd("`r","`n") -split '\r?\n')
    Assert-Probe ($lines.Count -ge 1 -and $lines.Count -le 128)
    $seen = @{}
    foreach ($line in $lines) {
        Assert-Probe ($line.Length -le 2048)
        $j = $line | ConvertFrom-Json -AsHashtable -ErrorAction Stop
        Assert-Probe ($j -is [hashtable] -and $j.stage -is [string])
        foreach ($field in @('ok','mediumIntegrity','productionAcceptance')) {
            if ($j.ContainsKey($field)) { Assert-Probe ($j[$field] -is [bool]) }
        }
        foreach ($field in @('integrityRid','exitCode','descendantExitCode','liveJobProcesses')) {
            if ($j.ContainsKey($field)) { Assert-Probe ($j[$field] -is [long] -or $j[$field] -is [int]) }
        }
        # Never publish arbitrary text from a compromised payload. Only selected typed fields.
        switch -CaseSensitive ($j.stage) {
            'preflight' { Assert-Probe ($j.ok -ceq $true -and $j.expectedSid -ceq $Sid); $seen.preflight=$true }
            { $_ -cin @('logon-token','suspended-child','descendant') } {
                Assert-Probe ($j.observedSid -ceq $Sid -and $j.integrityRid -eq 8192 -and $j.mediumIntegrity -ceq $true)
                $seen[$j.stage]=$true
            }
            'child-exit' { Assert-Probe ($j.exitCode -eq 42); $seen.child=$true }
            'bounded-tree-probe' {
                Assert-Probe ($j.ok -ceq $true -and $j.descendantExitCode -eq 124 -and $j.liveJobProcesses -eq 0 -and $j.productionAcceptance -ceq $false)
                $seen.tree=$true
            }
            'failure' { throw 'probe-reported-failure' }
            default { Assert-Probe ($j.stage -cin @('win32','lsa','privilege','account','child-wait')) }
        }
    }
    foreach ($s in @('preflight','logon-token','suspended-child','descendant','child','tree')) { Assert-Probe ($seen[$s] -ceq $true) }
    Assert-Probe (($lines[-1] | ConvertFrom-Json).stage -ceq 'bounded-tree-probe' -or ($lines[-1] | ConvertFrom-Json).stage -ceq 'win32')
    return '{"stage":"ci-evidence","ok":true,"rootExitCode":42,"descendantExitCode":124,"liveJobProcesses":0,"productionAcceptance":false}'
}
Export-ModuleMember -Function *-Probe*
