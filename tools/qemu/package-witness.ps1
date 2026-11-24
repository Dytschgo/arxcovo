param(
    [Parameter(Mandatory)][string]$Build,
    [Parameter(Mandatory)][string]$Source,
    [Parameter(Mandatory)][string]$UcrtBin,
    [Parameter(Mandatory)][string]$Stage
)

$ErrorActionPreference = 'Stop'
foreach ($dir in @($Build, $Source, $UcrtBin)) {
    $item = Get-Item -LiteralPath $dir
    if (!$item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'Expected a regular input directory'
    }
}
$Build = (Resolve-Path -LiteralPath $Build).Path
$Source = (Resolve-Path -LiteralPath $Source).Path
$UcrtBin = (Resolve-Path -LiteralPath $UcrtBin).Path
$Stage = [IO.Path]::GetFullPath($Stage)
if (Test-Path -LiteralPath $Stage) { throw 'Staging directory already exists' }
New-Item -ItemType Directory -Path $Stage | Out-Null

# Meson's install inventory includes generated and source firmware.
$installed = Get-Content -LiteralPath "$Build/meson-info/intro-installed.json" -Raw |
    ConvertFrom-Json -AsHashtable
foreach ($entry in $installed.GetEnumerator()) {
    if ($entry.Value -isnot [string]) { continue }
    $destination = $entry.Value.Replace('\','/').Replace('/./','/')
    if ($destination -notmatch '/arxcovo-witness/(share/.+)$') { continue }
    $relative = $Matches[1]
    $origin = [IO.Path]::GetFullPath($entry.Key)
    if (!$origin.StartsWith("$Build\", [StringComparison]::OrdinalIgnoreCase) -and
        !$origin.StartsWith("$Source\", [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Firmware source outside the build and source trees'
    }
    $target = [IO.Path]::GetFullPath((Join-Path $Stage $relative))
    if (!$target.StartsWith("$Stage\", [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Firmware destination outside the staging tree'
    }
    New-Item -ItemType Directory -Force -Path (Split-Path $target) | Out-Null
    Copy-Item -LiteralPath $origin -Destination $target
}
foreach ($name in @('bios-256k.bin', 'kvmvapic.bin', 'edk2-x86_64-code.fd', 'edk2-i386-vars.fd')) {
    if (!(Test-Path -LiteralPath "$Stage/share/$name" -PathType Leaf)) {
        throw "Missing required firmware $name"
    }
}
Copy-Item -LiteralPath "$Source/COPYING","$Source/COPYING.LIB" -Destination $Stage

$queue = [Collections.Generic.Queue[string]]::new()
$seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($name in @('qemu-system-x86_64.exe','qemu-img.exe')) {
    Copy-Item -LiteralPath "$Build/$name" -Destination $Stage
    & "$UcrtBin/strip.exe" --strip-debug "$Stage/$name"
    if ($LASTEXITCODE) { throw "Strip failed for $name" }
    $queue.Enqueue($name)
    $null = $seen.Add($name)
}
while ($queue.Count) {
    $name = $queue.Dequeue()
    $header = & "$UcrtBin/objdump.exe" -f "$Stage/$name"
    if ($LASTEXITCODE -or !($header -match 'file format pei-x86-64')) {
        throw "Invalid Windows x64 image $name"
    }
    $details = & "$UcrtBin/objdump.exe" -p "$Stage/$name"
    if ($LASTEXITCODE) { throw "Cannot read import table for $name" }
    foreach ($entry in ($details | Select-String 'DLL Name:\s+([a-zA-Z0-9_.+-]+\.dll)\s*$')) {
        $dll = $entry.Matches[0].Groups[1].Value
        if (Test-Path -LiteralPath "$UcrtBin/$dll" -PathType Leaf) {
            "$name -> $dll (UCRT64)"
            if ($seen.Add($dll)) {
                Copy-Item -LiteralPath "$UcrtBin/$dll" -Destination $Stage
                $queue.Enqueue($dll)
            }
        } elseif ($dll -match '^(api|ext)-ms-win-' -or
                  (Test-Path -LiteralPath "$env:SystemRoot/System32/$dll" -PathType Leaf)) {
            "$name -> $dll (Windows)"
        } else {
            throw "Unresolved import $dll in $name"
        }
    }
}

if (Get-ChildItem -LiteralPath $Stage -Force -Recurse -Attributes ReparsePoint) {
    throw 'Reparse point in staging tree'
}
$files = [Collections.Generic.SortedDictionary[string,IO.FileInfo]]::new([StringComparer]::Ordinal)
foreach ($file in (Get-ChildItem -LiteralPath $Stage -File -Force -Recurse)) {
    $name = [IO.Path]::GetRelativePath($Stage, $file.FullName).Replace('\','/')
    $files.Add($name, $file)
}
$rows = foreach ($entry in $files.GetEnumerator()) {
    $file = $entry.Value
    $digest = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$($file.Length) $digest  $($entry.Key)"
}
[IO.File]::WriteAllText("$Stage/MANIFEST.sha256", ($rows -join [char]10) + [char]10)

