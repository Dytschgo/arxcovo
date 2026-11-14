# Compiles native imports as source only. Run() never constructs HeldFiles or
# OwnedSession and calls no native import, process API or wall-clock reader.
$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot 'Controller.cs'
$before = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
$source = [IO.File]::ReadAllText($path)
$heldPath = Join-Path $PSScriptRoot 'HeldFiles.cs'
$heldBefore = (Get-FileHash -LiteralPath $heldPath -Algorithm SHA256).Hash
$held = [IO.File]::ReadAllText($heldPath)
$parts = @($source, $held)
foreach($name in @('NativeJob.cs','Admission.cs')) { $parts += [IO.File]::ReadAllText((Join-Path $PSScriptRoot $name)) }
$imports = ($parts | ForEach-Object { [regex]::Matches($_,'(?m)^using [^;]+;') } | ForEach-Object Value | Sort-Object -Unique) -join "`n"
$source = $imports + "`n" + (($parts | ForEach-Object { [regex]::Replace($_, '(?m)^using [^;]+;\r?\n', '') }) -join "`n")
$references = @([IO.Directory]::GetFiles((Join-Path $PSHOME 'ref'), '*.dll'))
Add-Type -TypeDefinition $source -ReferencedAssemblies $references -ErrorAction Stop
[PureControllerChecks]::Run()
'managed pure baseline: pass'
$mutants = @(
    @('index', 'new Row(1,1,0), // 15', 'new Row(1,0,0), // 15'),
    @('complete', '&& budget.Complete && exited', '&& exited'),
    @('timeout', 'now-activity>=10000', 'now-activity>10000'),
    @('arguments', 'if(args[0]!=Kernel && args[0]!=Matrix) return false;', 'if(args[0]==null) return false;'),
    @('environment', 'pair.Value!=expected', 'pair.Value==null'),
    @('held-pin', 'return actual==externalExpectedHash;', 'return true;'),
    @('link2', 'links==1', 'links<=2'),
    @('argument-escape', 'slashes*2+1', 'slashes*2'),
    @('frame-eof', 'used!=0 || frames!=expected', 'frames!=expected'),
    @('frame-output', 'v[7]>1', 'v[7]>2'),
    @('manifest-extra', 'Need(set.Remove(p.Name));', 'set.Remove(p.Name);'),
    @('manifest-external', 'HeldFiles.HashMatches(bytes,externalSha,65536)', 'true'),
    @('manifest-commit', 'Text(e,"reviewed_commit",40)==reviewedCommit', 'true'),
    @('cleanup-kill-first', 'terminateSucceeded && active==0', 'active==0'),
    @('cleanup-active', 'active==0 && processExited', 'processExited'),
    @('cleanup-deadline', 'now<until', 'now<=until'),
    @('post-hash-links', 'before==1 && after==1', 'before==1'),
    @('frame-complete', 'frames!=expected', 'false'),
    @('frame-order', 'if(v[1]!=(uint)(frames/2) || v[2]!=(uint)(1-frames%2)) return Fail();', 'if(v[1]>1000) return Fail();'),
    @('manifest-self-field', 'Need(set.Remove(p.Name));', 'if(p.Name!="sha256" || names.Length==6) Need(set.Remove(p.Name));'),
    @('clock-order', 'last = elapsedMs;', 'last = 0;')
)
$i=0
foreach($m in $mutants) {
    $i++
    if(-not $source.Contains($m[1])) { throw 'mutant target missing' }
    $variant=$source.Replace($m[1],$m[2]).Replace('ReplayController',"ReplayController$i").Replace('PureControllerChecks',"PureControllerChecks$i").Replace('HeldFiles',"HeldFiles$i").Replace('NativeJob',"NativeJob$i").Replace('Admission',"Admission$i")
    $bytes=[Text.Encoding]::UTF8.GetBytes($variant)
    "mutant LF SHA256 $($m[0]): $([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant())"
    # Compilation errors terminate here: they never count as assertion kills.
    Add-Type -TypeDefinition $variant -ReferencedAssemblies $references -ErrorAction Stop -WarningAction SilentlyContinue
    $type=([System.Management.Automation.PSTypeName]"PureControllerChecks$i").Type
    $killed=$false
    try { $type.GetMethod('Run').Invoke($null,@()) }
    catch {
        $failure=$_.Exception.GetBaseException()
        if($failure.GetType() -ne [System.Exception] -or $failure.Message -notin @('end','begin alone incomplete','missing all rows','empty nonblocking deadline','no first event','filter reject','path reject','held pin mismatch','link2 reject','argument escape','quoted end','unterminated','partial EOF','output overflow','poll clock regression','manifest extra','manifest external mismatch','manifest commit mismatch','stream end','stream begin','stream complete','cleanup kill first','cleanup active','cleanup deadline','post hash links','wire complete','duplicate frame','manifest self hash')) { throw }
        $killed=$true
    }
    if(-not $killed) { throw "survived: $($m[0])" }
    "assertion-killed: $($m[0])"
}
if((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $before) { throw 'source bytes changed' }
if((Get-FileHash -LiteralPath $heldPath -Algorithm SHA256).Hash -ne $heldBefore) { throw 'held file source bytes changed' }
"unchanged LF source SHA256: $before"
"unchanged LF held file SHA256: $heldBefore"
