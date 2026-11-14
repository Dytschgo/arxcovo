# Compiles native imports as source only. Run() never constructs HeldFiles or
# OwnedSession and calls no native import, process API or wall-clock reader.
$ErrorActionPreference = 'Stop'
$fixedNames = @('Controller.cs','HeldFiles.cs','NativeJob.cs','Admission.cs','Test-PureController.ps1')
$fixedHashes = @{}
foreach($name in $fixedNames) {
    $bytes = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot $name))
    if($bytes.Contains([byte]13)) { throw 'expected exact LF source' }
    $fixedHashes[$name] = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
}
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
    @('clock-order', 'last = elapsedMs;', 'last = 0;'),
    @('names-extra', 'if(entries==null || entries.Length!=3', 'if(entries!=null && entries.Length==4) return true; if(entries==null || entries.Length!=3'),
    @('names-repeat', 'if(entries==null || entries.Length!=3', 'if(entries!=null && entries.Length==3 && entries[1].Name==entries[2].Name) return true; if(entries==null || entries.Length!=3'),
    @('names-missing', 'if(entries==null || entries.Length!=3', 'if(entries!=null && entries.Length==2) return true; if(entries==null || entries.Length!=3'),
    @('names-reparse', '(e.Attributes&0x400)!=0', 'false'),
    @('names-tag', 'e.ReparseTag!=0', 'false'),
    @('names-leaf', 'names.Contains(leaf)', 'true'),
    @('names-type', '(e.Attributes&0x10)!=0', 'false'),
    @('names-dot-type', '(e.Attributes&0x10)==0', '(e.Attributes&0x10)==0x80000000'),
    @('names-id', '(e.Low==0 && e.High==0)', 'false'),
    @('scan-id', 'a.Low==b.Low', 'true'),
    @('scan-wire', '&& HeldFiles.SameScan(first,second)', ''),
    @('scan-first-wire', 'Need(HeldFiles.ExclusiveNames(first,leaf));', ''),
    @('scan-second-wire', 'var second=directory.Scan();', 'var second=first;'),
    @('leaf-id-wire', 'Need(HeldFiles.IdText(entry.Low,entry.High)==pin.id);', '{}'),
    @('leaf-size-wire', 'Need(leafBytes.Length==pin.size);', '{}'),
    @('root-volume', 'if(!ReplayController.LaunchSpec.Absolute(root) || !Inside(root,helper)', 'if(root!=null && root.Length==3) return true; if(!ReplayController.LaunchSpec.Absolute(root) || !Inside(root,helper)'),
    @('root-nested', '|| Inside(root,system)', ''),
    @('root-equal', '|| string.Equals(root,system,StringComparison.OrdinalIgnoreCase)', ''),
    @('root-parent', '|| Inside(system,root)', ''),
    @('root-pins', 'if(!Inside(root,path)) return false;', 'if(path==null) return false;'),
    @('root-helper', '!Inside(root,helper)', 'helper==null'),
    @('root-temp', '!Inside(root,temp)', 'temp==null'),
    @('root-wire', 'Need(SystemRootSeparate(root,helper,temp,system,pins.ConvertAll(p=>p.path)));', ''),
    @('binding-root-hash', 'HeldFiles.HashMatches(disposition,externalDispositionSha,65536)', 'true'),
    @('binding-held-manifest', 'b.Length==size && HeldFiles.HashMatches(b,hash,size)', 'b.Length==size'),
    @('binding-git-bytes', 'GitBlob(bytes[p.role])==p.blob', 'true'),
    @('binding-reviewed-blob', 'p.blob==Text(blobs,source,40)', 'true'),
    @('binding-git-frame', '"blob "+bytes.Length.ToString(CultureInfo.InvariantCulture)+"\0"', '""'),
    @('binding-provenance-source', 'HeldFiles.HashMatches(bytes["source:"+source],Text(hashes,source,64),67108864)', 'true'),
    @('binding-provenance-commit', 'Text(pe,"source_commit_claim",40)==commit', 'true'),
    @('binding-provenance-helper', 'HeldFiles.HashMatches(bytes["helper"],Text(pe,"helper_sha256",64),67108864)', 'true'),
    @('binding-manifest-authority', 'e.GetProperty("runtime_authority").ValueKind==JsonValueKind.False);', 'true);'),
    @('binding-disposition-authority', 'e.GetProperty("runtime_authority").ValueKind==JsonValueKind.False && Text(e,"root",240)==root', 'Text(e,"root",240)==root'),
    @('binding-provenance-authority', 'pe.GetProperty("runtime_authority").ValueKind==JsonValueKind.False', 'true'),
    @('binding-provenance-native', 'pe.GetProperty("native_executed").ValueKind==JsonValueKind.False', 'true'),
    @('binding-alias-path', 'a.path==b.path && a.id==b.id', 'a.id==b.id'),
    @('binding-lock-bytes', 'a.size==b.size && a.sha256==b.sha256 && Equal(bytes[a.role],bytes[b.role])', 'true'),
    @('binding-role-alias', 'Need(Array.IndexOf(allowed,pair)>=0 || Array.IndexOf(allowed,reverse)>=0);', '{}'),
    @('binding-qword-order', 'low.ToString("x16")+high.ToString("x16")', 'high.ToString("x16")+low.ToString("x16")'),
    @('binding-facts-copy', 'return ClonePins(values);', 'return values;'),
    @('final-refusal', 'throw new InvalidOperationException("remaining admission gates unproved");', 'return;')
)
$i=0
foreach($m in $mutants) {
    $i++
    if(-not $source.Contains($m[1])) { throw 'mutant target missing' }
    $mutated=$source.Replace($m[1],$m[2])
    if($m[0] -eq 'names-leaf') { $mutated=$mutated.Replace('e.Name!=leaf','false') }
    $variant=$mutated.Replace('ReplayController',"ReplayController$i").Replace('PureControllerChecks',"PureControllerChecks$i").Replace('HeldFiles',"HeldFiles$i").Replace('NativeJob',"NativeJob$i").Replace('Admission',"Admission$i").Replace('BindingPureChecks',"BindingPureChecks$i")
    $bytes=[Text.Encoding]::UTF8.GetBytes($variant)
    "mutant LF SHA256 $($m[0]): $([Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant())"
    # Compilation errors terminate here: they never count as assertion kills.
    Add-Type -TypeDefinition $variant -ReferencedAssemblies $references -ErrorAction Stop -WarningAction SilentlyContinue
    $type=([System.Management.Automation.PSTypeName]"PureControllerChecks$i").Type
    $killed=$false
    try { $type.GetMethod('Run').Invoke($null,@()) }
    catch {
        $failure=$_.Exception.GetBaseException()
        if($failure.GetType() -ne [System.Exception] -or (-not $failure.Message.StartsWith('admission: ') -and $failure.Message -notin @('end','begin alone incomplete','missing all rows','empty nonblocking deadline','no first event','filter reject','path reject','held pin mismatch','link2 reject','argument escape','quoted end','unterminated','partial EOF','output overflow','poll clock regression','manifest extra','manifest external mismatch','manifest commit mismatch','stream end','stream begin','stream complete','cleanup kill first','cleanup active','cleanup deadline','post hash links','wire complete','duplicate frame','manifest self hash'))) { throw }
        $killed=$true
    }
    if(-not $killed) { throw "survived: $($m[0])" }
    "assertion-killed: $($m[0])"
}
if((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ne $before) { throw 'source bytes changed' }
if((Get-FileHash -LiteralPath $heldPath -Algorithm SHA256).Hash -ne $heldBefore) { throw 'held file source bytes changed' }
"unchanged LF source SHA256: $before"
"unchanged LF held file SHA256: $heldBefore"

foreach($name in $fixedNames) {
    $bytes = [IO.File]::ReadAllBytes((Join-Path $PSScriptRoot $name))
    $hash = [Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($bytes)).ToLowerInvariant()
    if($hash -ne $fixedHashes[$name] -or $bytes.Contains([byte]13)) { throw 'exact LF restore mismatch' }
    "exact LF unchanged $name $hash"
}
