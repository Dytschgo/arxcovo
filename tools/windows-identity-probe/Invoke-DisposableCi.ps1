# Only mutation entrypoint. Never invoke on a developer/owner computer.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
Import-Module "$PSScriptRoot/CiValidation.psm1" -Force
try {
    $context = Get-ProbeContext
    Assert-ProbeContext $context
    Assert-Probe $IsWindows
    $id = [Security.Principal.WindowsIdentity]::GetCurrent()
    try { Assert-Probe ([Security.Principal.WindowsPrincipal]::new($id).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) } finally { $id.Dispose() }
    Assert-Probe ((& git -C $env:GITHUB_WORKSPACE rev-parse HEAD) -ceq $context.REVIEWED_SHA)
    Assert-Probe ([string]::IsNullOrEmpty((& git -C $env:GITHUB_WORKSPACE status --porcelain --untracked-files=no)))
} catch { Write-Output (Get-ProbeSetupDiagnostic 'preflight' $_); Write-Output '{"stage":"ci-experiment","ok":false}'; exit 1 }
$account = 'axp' + [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(4)).ToLowerInvariant()
$plan = Get-ProbePlan $context.GITHUB_RUN_ID $context.GITHUB_RUN_ATTEMPT $account
$state = @{ Sid=''; AccountCreated=$false; RightCreated=$false; TaskCreated=$false; PathCreated=$false; ParentCreated=$false }
$root = $plan.Root
$taskXml = $null
$success = $false
$cleanupOk = $true
function Assert-Lifetime {
    Assert-ProbeContext (Get-ProbeContext)
    Assert-ProbePath $root $root
    $user = Get-LocalUser -Name $account -ErrorAction Stop
    Assert-Probe ($user.SID.Value -ceq $state.Sid -and $user.Enabled)
    return $user
}
function Set-PrivateAcl([string]$Path, [string]$TestRights='') {
    Assert-ProbePath $Path 'C:\arxcovo-probe'
    $inheritance = if ($TestRights -eq 'ReadAttributes,Traverse') { 'None' } else { 'ContainerInherit,ObjectInherit' }
    $acl = Get-ProbeDirectoryAcl $state.Sid $TestRights $inheritance
    Set-Acl -LiteralPath $Path -AclObject $acl
}
$script:cleanupEntries = 0
function Remove-SafeTree([string]$Path, [int]$Depth = 0) {
    Assert-Probe ($Depth -le 32 -and $script:cleanupEntries -le 512)
    Assert-ProbePath $Path $root
    foreach ($name in [IO.Directory]::EnumerateFileSystemEntries($Path)) {
        $script:cleanupEntries++
        Assert-Probe ($script:cleanupEntries -le 512)
        $entry = Get-Item -Force -LiteralPath $name
        Assert-ProbePath $entry.FullName $root
        Assert-Probe (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)
        if ($entry.PSIsContainer) { Remove-SafeTree $entry.FullName ($Depth + 1) } else { Remove-Item -Force -LiteralPath $entry.FullName }
    }
    Remove-Item -Force -LiteralPath $Path
}
$phase = 'root'
try {
    Assert-ProbePath $root $root
    Assert-Probe (!(Test-Path -LiteralPath $root))
    Assert-Probe (!(Test-Path -LiteralPath 'C:\arxcovo-probe'))
    [IO.FileSystemAclExtensions]::Create([IO.DirectoryInfo]::new('C:\arxcovo-probe'), (Get-ProbeDirectoryAcl))
    $state.ParentCreated = $true
    Set-PrivateAcl 'C:\arxcovo-probe'
    [void][IO.Directory]::CreateDirectory($root)
    $state.PathCreated = $true
    Set-PrivateAcl $root
    $phase = 'account'
    Assert-Probe ($null -eq (Get-LocalUser -Name $account -ErrorAction SilentlyContinue))
    $password = [Security.SecureString]::new()
    try {
        foreach ($c in 'aA1!'.ToCharArray()) { $password.AppendChar($c) }
        $random = [Security.Cryptography.RandomNumberGenerator]::GetBytes(32)
        try { foreach ($b in $random) { $password.AppendChar([char](33 + ($b % 90))) } } finally { [Array]::Clear($random) }
        $password.MakeReadOnly()
        $user = New-LocalUser -Name $account -Password $password -AccountNeverExpires -UserMayNotChangePassword
        $state.Sid = $user.SID.Value
        $state.AccountCreated = $true
    } finally { $password.Dispose() }
    [void](Assert-Lifetime)
    $phase = 'groups'
    $users = Get-LocalGroup -SID 'S-1-5-32-545'
    $members = @(Get-LocalGroupMember -Group $users | ForEach-Object { $_.SID.Value })
    if ($state.Sid -cnotin $members) { Add-LocalGroupMember -Group $users -Member $user }
    foreach ($group in Get-LocalGroup) {
        $members = @(Get-LocalGroupMember -Group $group | ForEach-Object { $_.SID.Value })
        if ($state.Sid -cin $members) { Assert-Probe ($group.SID.Value -ceq 'S-1-5-32-545') }
    }
    $phase = 'directory-acls'
    Set-PrivateAcl 'C:\arxcovo-probe' 'ReadAttributes,Traverse'
    Set-PrivateAcl $root 'ReadAttributes,Traverse'
    foreach ($dir in @('bin','work')) { [void][IO.Directory]::CreateDirectory("$root\$dir") }
    $phase = 'payload'
    $publish = "$PSScriptRoot\bin\Release\net10.0-windows\win-x64\publish"
    Assert-ProbePath $publish $publish
    foreach ($file in Get-ChildItem -LiteralPath $publish -File) {
        Assert-Probe (($file.Attributes -band [IO.FileAttributes]::ReparsePoint) -eq 0)
        Copy-Item -LiteralPath $file.FullName -Destination "$root\bin"
    }
    Assert-Probe (Test-Path -LiteralPath "$root\bin\IdentityProbe.exe")
    Set-PrivateAcl "$root\bin" 'ReadAndExecute'
    Set-PrivateAcl "$root\work" 'Modify'
    $phase = 'batch-right'
    Add-Type -Path "$PSScriptRoot/ProbeBatchRight.cs"
    [void](Assert-Lifetime)
    [ProbeBatchRight]::Change($state.Sid, $true)
    $state.RightCreated = $true
    $taskCode = @'
$ErrorActionPreference = 'Stop'
try {
    $start = [Diagnostics.ProcessStartInfo]::new('__ROOT__\bin\IdentityProbe.exe')
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardOutput = $true
    $start.Environment.Clear()
    $start.Environment['SystemRoot'] = [Environment]::GetFolderPath('Windows')
    $start.Environment['GITHUB_ACTIONS'] = 'true'
    $start.Environment['RUNNER_ENVIRONMENT'] = 'github-hosted'
    $start.Environment['RUNNER_OS'] = 'Windows'
    $start.Environment['ARXCOVO_DISPOSABLE_PROBE'] = 'reviewed-manual-ci-v1'
    $start.Environment['GITHUB_RUN_ID'] = '__RUN__'
    $start.Environment['GITHUB_RUN_ATTEMPT'] = '__ATTEMPT__'
    $start.ArgumentList.Add('__ACCOUNT__')
    $start.ArgumentList.Add('__ROOT__\work')
    $p = [Diagnostics.Process]::Start($start)
    try {
        $buffer = [char[]]::new(131073)
        $count = 0
        $deadline = [Diagnostics.Stopwatch]::StartNew()
        while ($count -lt $buffer.Length) {
            $remaining = 20000 - [int]$deadline.ElapsedMilliseconds
            if ($remaining -le 0) { throw 'output-timeout' }
            $read = $p.StandardOutput.ReadAsync($buffer, $count, $buffer.Length - $count)
            if (!$read.Wait($remaining)) { throw 'output-timeout' }
            $n = $read.Result
            if ($n -eq 0) { break }
            $count += $n
        }
        if ($count -gt 131072 -or !$p.WaitForExit([Math]::Max(0, 20000 - [int]$deadline.ElapsedMilliseconds))) { throw 'bounded-output-or-time' }
        [IO.File]::WriteAllText('__ROOT__\result.jsonl', [string]::new($buffer, 0, $count))
        exit $p.ExitCode
    } finally { if (!$p.HasExited) { $p.Kill($true); [void]$p.WaitForExit(5000) }; $p.Dispose() }
} catch { exit 2 }
'@
    $taskCode = $taskCode.Replace('__ROOT__',$root).Replace('__RUN__',$context.GITHUB_RUN_ID).Replace('__ATTEMPT__',$context.GITHUB_RUN_ATTEMPT).Replace('__ACCOUNT__',$account)
    [IO.File]::WriteAllText("$root\task.ps1",$taskCode)
    $phase = 'task-definition'
    $scheduler = New-Object -ComObject 'Schedule.Service'
    $scheduler.Connect()
    $folder = $scheduler.GetFolder('\')
    $definition = $scheduler.NewTask(0)
    $definition.Principal.UserId = 'S-1-5-18'
    $definition.Principal.LogonType = 5
    $definition.Principal.RunLevel = 1
    $definition.Settings.ExecutionTimeLimit = 'PT1M'
    $definition.Settings.RestartCount = 0
    $definition.Settings.AllowDemandStart = $true
    $action = $definition.Actions.Create(0)
    $phase = 'task-path'
    $action.Path = (Get-Process -Id $PID).Path
    Assert-ProbeTrustedShellPath $action.Path
    $action.Arguments = '-NoLogo -NoProfile -NonInteractive -File "' + $root + '\task.ps1"'
    # TASK_CREATE (2) refuses existing names atomically. Protected SYSTEM/admin DACL.
    $phase = 'task-registration'
    $registered = $folder.RegisterTaskDefinition($plan.Task,$definition,2,'SYSTEM',$null,5,'D:P(A;;FA;;;SY)(A;;FA;;;BA)')
    $state.TaskCreated = $true
    $taskXml = $registered.Xml
    [void](Assert-Lifetime)
    $phase = 'task-run'
    $before = $registered.LastRunTime
    [void]$registered.Run($null)
    $phase = 'task-wait'
    $watch = [Diagnostics.Stopwatch]::StartNew()
    do {
        Start-Sleep -Milliseconds 250
        $current = $folder.GetTask($plan.Task)
        Assert-Probe ($current.Xml -ceq $taskXml)
        $done = $current.LastRunTime -gt $before -and $current.State -ne 4 -and $current.State -ne 2
    } while (!$done -and $watch.Elapsed.TotalSeconds -lt 75)
    if ($done) { Write-Output (ConvertTo-Json @{stage='task-result';code=[long]$current.LastTaskResult} -Compress) }
    Assert-Probe $done
    [void](Assert-Lifetime)
    $phase = 'result-read'
    $result = "$root\result.jsonl"
    Assert-ProbePath $result $root
    Assert-Probe ((Get-Item -LiteralPath $result).Length -le 262144)
    $evidence = [IO.File]::ReadAllText($result)
    $phase = 'result-validation'
    Write-Output (Get-ProbeDiagnostics $evidence)
    Assert-Probe ($current.LastTaskResult -eq 0)
    Write-Output (Read-ProbeEvidence $evidence $state.Sid)
    $success = $true
} catch { Write-Output (Get-ProbeSetupDiagnostic $phase $_); Write-Output '{"stage":"ci-experiment","ok":false}' }
finally {
    if ($state.AccountCreated) {
        try { [void](Assert-Lifetime); $steps = @(Get-ProbeCleanupPlan $state $state.Sid) } catch { $steps=@(); $cleanupOk=$false }
    } else { $steps=@(); if ($state.PathCreated) { $steps=@('path') } }
    foreach ($step in $steps) {
        try {
            switch ($step) {
                task {
                    $current = $folder.GetTask($plan.Task)
                    Assert-Probe ($current.Xml -ceq $taskXml)
                    if ($current.State -eq 4 -or $current.State -eq 2) { $current.Stop(0) }
                    $timer = [Diagnostics.Stopwatch]::StartNew()
                    while ($folder.GetTask($plan.Task).State -in @(2,4) -and $timer.Elapsed.TotalSeconds -lt 5) { Start-Sleep -Milliseconds 100 }
                    Assert-Probe ($folder.GetTask($plan.Task).State -notin @(2,4))
                    Assert-Probe ($folder.GetTask($plan.Task).GetInstances(0).Count -eq 0)
                    $folder.DeleteTask($plan.Task,0)
                }
                right { [void](Assert-Lifetime); [ProbeBatchRight]::Change($state.Sid,$false) }
                account { [void](Assert-Lifetime); Remove-LocalUser -SID $state.Sid }
                path { Assert-ProbePath $root $root; Remove-SafeTree $root }
            }
        } catch { $cleanupOk=$false }
    }
    if ($state.ParentCreated) {
        try {
            Assert-ProbePath 'C:\arxcovo-probe' 'C:\arxcovo-probe'
            Assert-Probe (@(Get-ChildItem -Force -LiteralPath 'C:\arxcovo-probe').Count -eq 0)
            Remove-Item -LiteralPath 'C:\arxcovo-probe' -Force
        } catch { $cleanupOk=$false }
    }
    Write-Output ('{"stage":"cleanup","ok":' + $cleanupOk.ToString().ToLowerInvariant() + '}')
}
if (!$success -or !$cleanupOk) { exit 1 }
