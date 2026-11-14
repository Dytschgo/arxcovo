# Compiles native imports as source only. Run() never constructs HeldFiles or
# OwnedSession and calls no native import, process API or wall-clock reader.
$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot 'Controller.cs'
$before = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
$source = [IO.File]::ReadAllText($path)
$heldPath = Join-Path $PSScriptRoot 'HeldFiles.cs'
$heldBefore = (Get-FileHash -LiteralPath $heldPath -Algorithm SHA256).Hash
$held = [IO.File]::ReadAllText($heldPath)
$imports = "using System.IO;`nusing System.Runtime.InteropServices;`nusing System.Security.Cryptography;`nusing Microsoft.Win32.SafeHandles;`n"
$source = $imports + $source + [regex]::Replace($held, '(?m)^using [^;]+;\r?\n', '')
Add-Type -TypeDefinition $source -ErrorAction Stop
[PureControllerChecks]::Run()
'managed pure baseline: pass'
$mutants = @(
    @('index', 'new Row(1,1,0), // 15', 'new Row(1,0,0), // 15'),
    @('complete', '&& budget.Complete && exited', '&& exited'),
    @('timeout', 'now-activity>=10000', 'now-activity>10000'),
    @('arguments', 'if(args[8]!=Kernel && args[8]!=Matrix) return false;', 'if(args[8]==null) return false;'),
    @('environment', 'pair.Value!=expected', 'pair.Value==null'),
    @('held-pin', 'return actual==externalExpectedHash;', 'return true;'),
    @('link2', 'links==1', 'links<=2'),
    @('argument-escape', 'slashes*2+1', 'slashes*2'),
    @('frame-eof', 'used!=0 || frames==0', 'frames==0'),
    @('frame-output', 'v[7]>1', 'v[7]>2'),
    @('clock-order', 'last = elapsedMs;', 'last = 0;')
)
$i=0
foreach($m in $mutants) {
    $i++
    if(-not $source.Contains($m[1])) { throw 'mutant target missing' }
    $variant=$source.Replace($m[1],$m[2]).Replace('ReplayController',"ReplayController$i").Replace('PureControllerChecks',"PureControllerChecks$i").Replace('HeldFiles',"HeldFiles$i")
    # Compilation errors terminate here: they never count as assertion kills.
    Add-Type -TypeDefinition $variant -ErrorAction Stop -WarningAction SilentlyContinue
    $type=([System.Management.Automation.PSTypeName]"PureControllerChecks$i").Type
    $killed=$false
    try { $type.GetMethod('Run').Invoke($null,@()) }
    catch {
        $failure=$_.Exception.GetBaseException()
        if($failure.GetType() -ne [System.Exception] -or $failure.Message -notin @('end','begin alone incomplete','missing all rows','empty nonblocking deadline','no first event','filter reject','path reject','held pin mismatch','link2 reject','argument escape','quoted end','unterminated','partial EOF','output overflow','poll clock regression')) { throw }
        $killed=$true
    }
    if(-not $killed) { throw "survived: $($m[0])" }
    "assertion-killed: $($m[0])"
}
if((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $before) { throw 'source bytes changed' }
if((Get-FileHash -LiteralPath $heldPath -Algorithm SHA256).Hash -ne $heldBefore) { throw 'held file source bytes changed' }
"unchanged LF source SHA256: $before"
"unchanged LF held file SHA256: $heldBefore"
