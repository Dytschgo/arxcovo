$ErrorActionPreference = 'Stop'
. "$PSScriptRoot/e1a-bundle-preflight.ps1"

# Synthetic SID and in-memory descriptors. No filesystem ACL changes.
$owner = [Security.Principal.SecurityIdentifier]::new('S-1-5-21-101-102-103-1001')
$entries = "(A;;FA;;;BA)(A;;FA;;;SY)(A;;0x1200A9;;;$owner)"
function Check([string]$Sddl, [string[]]$Expected) {
    $raw = [Security.AccessControl.RawSecurityDescriptor]::new($Sddl)
    $actual = @(Get-E1aAclViolation $raw $owner)
    if (($actual -join "`n") -cne ($Expected -join "`n")) {
        throw "Unexpected diagnostics for synthetic ACL: $($actual -join '; ')"
    }
}

Check "O:BAD:P$entries" @()
Check "O:BAD:P$entries(A;OICI;FA;;;WD)" @(
    'ACE[3] trustee=other type=AccessAllowed flags=ObjectInherit, ContainerInherit mask=0x001F01FF (unexpected trustee, expected explicit non-inheritable ACE)'
)
Check "O:SYD:$entries" @('owner is not Administrators', 'DACL inheritance is enabled')
Check 'O:BAD:PNO_ACCESS_CONTROL' @('DACL missing or null')
Check "O:BAD:P(A;;FA;;;BA)(D;;FA;;;BA)(A;;FA;;;$owner)" @(
    'ACE[1] trustee=Administrators type=AccessDenied flags=None mask=0x001F01FF (expected ordinary allow, duplicate trustee)',
    'ACE[2] trustee=owner login type=AccessAllowed flags=None mask=0x001F01FF (expected mask 0x001200A9)',
    'missing ACE for SYSTEM'
)
Write-Output 'E1a ACL preflight descriptor tests passed'
