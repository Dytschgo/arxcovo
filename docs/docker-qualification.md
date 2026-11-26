# Owner runbook for static Docker qualification

**Status: unverified on real hardware.** The Windows `--full-docker` entry point
currently refuses before claiming output or starting native effects. Complete
the read-only preparation below as needed. Run the trial command only with a
reviewed release that wires checked native creation, live route enforcement,
private receiver capture and the boot-bound HTTPS exchange.

Use the [Windows host setup](dev-host-setup.md#native-qualification-runbook)
for the non-elevated current-user/WHPX checks and the
[M4 owner runbook](m4-acceptance.md) for the independent stock TEST comparison.
Leave existing VMs, their hypervisor settings and the old Lab Manager untouched.

## Owner inputs

- Pin the exact reviewed daemon and Windows x64 qualifier images, the host
  build and current non-elevated logon/profile, the production store and
  volume enrollment, and the private fixed-cap trial workspace and empty
  evidence root. The workspace and evidence root must be disjoint from one
  another and from the production store. Stop the daemon for the trial. The
  production store is a read-only identity input and stays outside all guest
  and helper arguments.
- Verify the measured 200 GiB aggregate cap, 50 GiB D: reserve, 16 GiB RAM
  reserve and at least half the effective CPU cores available to the owner.
  Check actual free capacity for all simultaneous disks, VARS, media,
  fixtures and helpers. The static guest uses 2 vCPUs and 4 GiB RAM, with
  a 6 GiB Job and a separate 1 GiB WHPX allowance. The diskless control
  uses its own 2 GiB Job limit.
- Supply the stock 11.1.3 bundle and the reviewed E1a QEMU 11.1.3 patched
  distribution with static libslirp 4.9.7, build `11.1.3-arx-egress-2`.
  Independently verify the QEMU source archive and signature, libslirp
  archive, patch and toolchain against review. Match the reviewed installed
  receipt, exact manifest and tree, lengths, executable, helpers and
  firmware roles including initial VARS. Obtain any omitted length or role
  pin from independently reviewed bytes before enrolling. Keep the stock
  bundle for restricted install, detached health, and stock controls. Use
  the patched bundle only for the public-egress boots.
  [E1a receipt](../tools/qemu/public-egress-receipt.json)
- Supply the official Ubuntu 24.04.3 live-server amd64 ISO, pinned installer
  and guest artifacts, and the reviewed Ubuntu web pool. Authenticate Ubuntu
  and Docker signing keys and signed release/index metadata independently,
  including the Docker key fingerprint and bytes. Verify exact archives,
  versions including epochs, base-package inventory, the signed closure and
  the bounded read-only `ARXCOMPOSE` dependency medium.
- Provide a disposable root-capable Ubuntu fixture with the pinned guest
  network profile and a digest-pinned benign container. Independently
  enroll the policy and complete topology/deny inventory, the public
  TCP/UDP and recursive DNS receivers, the denial targets, and the bounded
  receiver-capture and controlled route-drift procedures. Include dormant
  site adapters, VPNs, host services, routers, public-numbered LAN and WAN
  hairpin. Use an isolated representative uplink if the ordinary site
  cannot sustain the enrolled invariant. Supply the private generation-bound
  CA/SNI profile and keep keys under owner/guest-root custody. No
  Cloudflare credentials are needed.

## Prepare a separately protected E1a copy

Use the D: scratch environment from [dev-scratch.md](dev-scratch.md) in both
consoles. From non-elevated PowerShell, set `$preflight` to the full path of
the checked-in `tools/qemu/e1a-bundle-preflight.ps1` in the reviewed checkout.
Run the read-only check against the candidate. The output uses sorted tree
node numbers, the root/data/payload role and the exact failing ACE index,
type, flags and mask. Trustees are reported by role. Exit 0 means the ACL
shape passed, 1 means a violation and 2 means the tree could not be inspected
safely.

```powershell
$preflight = (Resolve-Path -LiteralPath 'tools/qemu/e1a-bundle-preflight.ps1').Path
$candidate = Read-Host 'Exact candidate bundle directory on D:'
pwsh -NoProfile -File $preflight -Bundle $candidate
$LASTEXITCODE
```

The check requires Administrators ownership and a protected DACL with one
explicit allow each for Administrators and SYSTEM with FullControl and the
owner login with ReadAndExecute. Use it for preparation. The qualifier still
requires held identity, contents and custody checks at use.

**Owner admin step.** Open an elevated PowerShell under the same owner's
login. Close processes using the source bundle and keep both trees quiescent
during setup. Choose a fresh destination beneath an existing, independently
checked administrator-controlled parent on D:. This command creates new
files and applies protection only to that separately prepared copy. If an
operation fails, stop and leave the copy unenrolled until the owner has
inspected it.

```powershell
$ErrorActionPreference = 'Stop'
$preflight = (Resolve-Path -LiteralPath 'tools/qemu/e1a-bundle-preflight.ps1').Path
. $preflight
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Use elevated PowerShell under the enrolled owner login'
}
$admins = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')
$system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')
$owner = $identity.User
if ($owner -eq $admins -or $owner -eq $system) { throw 'Expected the owner login' }

$source = Read-Host 'Exact independently reviewed patched bundle directory on D:'
$parent = Read-Host 'Exact dedicated administrator-controlled destination parent on D:'
$copy = Join-Path $parent '11.1.3-arx-egress-2-qualification'
$sourceNodes = @(Get-E1aTree $source)
# Get the parent and validate its ancestors using the same path checks.
$null = @(Get-E1aTree $parent)
foreach ($pair in @(@{A=$source; B=$copy}, @{A=$copy; B=$source})) {
    if ($pair.B.Equals($pair.A, [StringComparison]::OrdinalIgnoreCase) -or
        $pair.B.StartsWith($pair.A + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Source and copy must be disjoint'
    }
}
if (Test-Path -LiteralPath $copy) { throw 'The destination already exists' }
New-Item -ItemType Directory -Path $copy | Out-Null
foreach ($node in $sourceNodes) {
    $node.Refresh()
    if (-not $node.Exists -or ($node.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'The reviewed source tree changed'
    }
    $relative = [IO.Path]::GetRelativePath($source, $node.FullName)
    if ($relative -eq '.') { continue }
    $dest = Join-Path $copy $relative
    if ($node -is [IO.DirectoryInfo]) {
        New-Item -ItemType Directory -Path $dest | Out-Null
    } else {
        [IO.File]::Copy($node.FullName, $dest, $false)
    }
}

$nodes = @(Get-E1aTree $copy)
for ($index = $nodes.Count - 1; $index -ge 0; $index--) {
    $node = $nodes[$index]
    $node.Refresh()
    if (-not $node.Exists -or ($node.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
        throw 'The new copy changed before protection'
    }
    $acl = if ($node -is [IO.DirectoryInfo]) {
        [Security.AccessControl.DirectorySecurity]::new()
    } else {
        [Security.AccessControl.FileSecurity]::new()
    }
    $acl.SetOwner($admins)
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($sid in @($admins, $system, $owner)) {
        $rights = if ($sid -eq $owner) { 'ReadAndExecute' } else { 'FullControl' }
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new(
            $sid, [Security.AccessControl.FileSystemRights]$rights, 'Allow'
        ))
    }
    Set-Acl -LiteralPath $node.FullName -AclObject $acl
}
```

Close the elevated console. In the original non-elevated console, rerun the
preflight command with the new copy as the candidate. Reverify the entire
manifest and byte pins against the reviewed E1a receipt, then enroll the
**new copy's exact path and held identity** in the independently reviewed
private Docker enrollment. Keep that enrollment separate from the stock TEST
enrollment and the output root. Use the enrollment field contract of the
reviewed native release. The gated CLI currently parses only the Docker class
at this boundary.

## Run and review

1. Verify the exact executable and enrollment pins, and recheck the host,
   reserves, workspace and network policy from the owner's non-elevated
   console. Arrange the controlled owner-idle window and stop the arxd
   daemon. Keep the private receiver/control observers armed before any
   test attempt. Confirm they observe tagged positive controls before and
   after each window. A guest timeout by itself yields no denial verdict.
2. Use separate stock TEST enrollment and evidence for the TEST comparison.
   Follow the reviewed [native TEST sequence](dev-host-setup.md#native-qualification-runbook).
   Exercise the same working public receivers. Retain independent counts
   and the original TEST seal for the later comparison. Recheck the
   Docker host pins and owner reserves after the stock run.
3. Once the native Docker release has been reviewed and opened, invoke its
   exact pinned runner once with a fresh, existing empty evidence directory.
   Fill these three paths from the reviewed private enrollment and verified
   release. Keep them out of transcripts and shared logs.

   ```powershell
   $qualifier = Read-Host 'Full path to the reviewed Windows x64 arx-vm-qualify.exe'
   $enrollment = Read-Host 'Full path to the private class=docker enrollment JSON'
   $evidence = Read-Host 'Full path to the fresh empty private Docker evidence directory'
   $qualifierPin = Read-Host 'Reviewed qualifier SHA-256'
   $enrollmentPin = Read-Host 'Reviewed private enrollment SHA-256'
   foreach ($inputPin in @(@{Path=$qualifier; Hash=$qualifierPin}, @{Path=$enrollment; Hash=$enrollmentPin})) {
       if ($inputPin.Hash -notmatch '^[0-9A-Fa-f]{64}$' -or
           (Get-FileHash -ErrorAction Stop -Algorithm SHA256 -LiteralPath $inputPin.Path).Hash -ne $inputPin.Hash) {
           throw 'Reviewed input pin mismatch'
       }
   }
   & $qualifier --full-docker $enrollment $evidence
   $status = $LASTEXITCODE
   if ($status -ne 0) { throw "Docker qualification stopped with code $status" }
   $seal = Join-Path $evidence 'qualification.json'
   Get-FileHash -Algorithm SHA256 -LiteralPath $seal | Select-Object -ExpandProperty Hash
   ```
4. Review the emitted contract 4 Docker dossier and marker-last settlement.
   The public fixture must demonstrate TCP, UDP and valid recursive DNS,
   zero arrivals at denied receivers across the full inventory, LAN inbound refusal,
   IPv6/ICMP refusal and flow closure on controlled route drift. Restore the
   original enrolled topology before the installed boot and resample. The
   installed boot must bind the exclusive IPv4 loopback listener to the
   held child, verify exact port, TLS profile and boot with a bounded pinned
   CA/SNI health exchange, complete the exact Compose v3 hello, and release
   cleanly. Require independent settlement of the owned workspace and
   helpers.
5. If a run is incomplete, cancelled, times out, or cannot settle, retain
   its diagnostics and any charge. Reconcile ownership and release from
   independent evidence before retrying with fresh roots. Never remove a
   charge marker to force a retry.
6. Review the Docker seal and scope before pinning them through the
   dedicated static selection for that same boot. Assemble the separate
   owner-private acceptance receipt from the original observations and
   final Docker and TEST seal digests after both are sealed.
   [Network receipt and workload driver](static-vm-acceptance.md)

Keep raw addresses, topology, receiver captures, private paths, account and
logon IDs, file IDs, nonces and key material in held private inputs. Shared
evidence contains scoped digests, opaque IDs, typed outcomes and bounded
counts. Keep DPAPI secrets, production Compose releases and public provider
credentials outside the trial.

A reviewed successful qualifier establishes the disposable Docker host,
network, recipe, endpoint and release dossier. Calm Life and the second
project, HTTP 206/416, persistence and backup/restore, and the live tunnel
journey each require the separate [static VM workload acceptance](static-vm-acceptance.md).
Public publishing defaults off. Trials use private loopback TLS. Agents
have no authority to delete the static VM, service volumes or settled
backups. Attributable disposable cleanup stays within the owned trial
workspace. [Publishing policy](adr/0058-v1-scope-static-vms-and-cloudflare.md) ·
[Backup custody](adr/0070-vm-store-v14-compose-backup.md) ·
[Owner retirement](adr/0071-vm-store-v15-compose-backup-retirement.md)
