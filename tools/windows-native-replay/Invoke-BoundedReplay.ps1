# Gate 2 preparation only. This entry point intentionally cannot launch anything.
[CmdletBinding()]
param(
    [ValidateSet('x86_64-pc-windows-msvc')][string]$Target = 'x86_64-pc-windows-msvc',
    [ValidateSet(10)][int]$CaseTimeoutSeconds = 10,
    [ValidateSet(360)][int]$GlobalTimeoutSeconds = 360,
    [ValidateSet(35)][int]$ExpectedCases = 35,
    [ValidateSet(20)][int]$ExpectedChildren = 20
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# No Add-Type, process API, cargo command or native registration in this stage.
# Even a supplied/edited manifest cannot enable this source-preparation entry.
throw 'GATE2_ONLY: native registration and directory-exclusion admission remain blocked; execution and environment reuse are blocked.'
