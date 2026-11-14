# Managed pure checks only. No native imports, process calls or wall-clock reads.
$ErrorActionPreference = 'Stop'
$path = Join-Path $PSScriptRoot 'Controller.cs'
$before = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
$source = [IO.File]::ReadAllText($path)
Add-Type -TypeDefinition $source -ErrorAction Stop
[PureControllerChecks]::Run()
'managed pure baseline: pass'
$mutants = @(
    @('index', 'new Row(1,1,0), // 15', 'new Row(1,0,0), // 15'),
    @('complete', '&& budget.Complete && exited', '&& exited'),
    @('timeout', 'now-activity>=10000', 'now-activity>10000'),
    @('arguments', 'if(args[8]!=Kernel && args[8]!=Matrix) return false;', 'if(args[8]==null) return false;'),
    @('environment', 'pair.Value!=expected', 'pair.Value==null')
)
$i=0
foreach($m in $mutants) {
    $i++
    if(-not $source.Contains($m[1])) { throw 'mutant target missing' }
    $variant=$source.Replace($m[1],$m[2]).Replace('ReplayController',"ReplayController$i").Replace('PureControllerChecks',"PureControllerChecks$i")
    # Compilation errors terminate here: they never count as assertion kills.
    Add-Type -TypeDefinition $variant -ErrorAction Stop -WarningAction SilentlyContinue
    $type=([System.Management.Automation.PSTypeName]"PureControllerChecks$i").Type
    $killed=$false
    try { $type.GetMethod('Run').Invoke($null,@()) }
    catch {
        $failure=$_.Exception.GetBaseException()
        if($failure.GetType() -ne [System.Exception] -or $failure.Message -notin @('end','missing all rows','empty nonblocking deadline','no first event','filter reject','path reject')) { throw }
        $killed=$true
    }
    if(-not $killed) { throw "survived: $($m[0])" }
    "assertion-killed: $($m[0])"
}
if((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $before) { throw 'source bytes changed' }
"unchanged LF source SHA256: $before"
