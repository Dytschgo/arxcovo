# Windows development host: one-time owner checklist

Status: see the owner update below, which supersedes the dedicated-account steps for this PC for both M2 and M4. No administrator changes or real VM tests have been performed by agents. Complete the account steps before dedicated-account host acceptance, and the remaining steps before milestone 4 VM acceptance. Keep the existing Lab Manager installation, state and `agent-windows-test` VM untouched.

## Owner update 2026-11-23 (supersedes the dedicated-account steps below for this PC, for M2 and M4)

Relayed by the babysitter on the owner's behalf.

- **Execution account.** The owner's current login is the Windows host execution account, using ADR 0029 `current_user` execution. Elevated, admin and SYSTEM tokens are still refused. No separate Windows host account is created. Agents may create their own guest OS users inside VMs.
- **Scoped install permission.** Agents verify the M4 host prerequisites read-only first. If something is missing, they may install or enable only: Windows Hypervisor Platform/WHPX, QEMU, and the `D:\ArxcovoData` disk cap and reserve settings.
  - An interactive UAC step is recorded here with its exact command and asked of the owner once.
  - Existing VMs, including Hyper-V `agent-windows-test`, and their hypervisor settings are never touched, stopped or reconfigured; changes are additive only.
  - `D:\Code\LabManager` and its installed app and state stay untouched.
- **Read-only verification, 2026-11-23, by the coordinator:**

  | Check | Result |
  | --- | --- |
  | Login | `azuread\<owner>`. UAC-filtered: medium integrity, `BUILTIN\Administrators` deny-only, not elevated. Compatible with `current_user` execution. |
  | Hypervisor | `HypervisorPresent=True`. `WinHvPlatform.dll` and `WinHvEmulation.dll` are present. `WHvGetCapability(HypervisorPresent)` returns 1, and the feature mask is `0x2FF`. **WHPX is usable from this login.** The optional-feature state query needs elevation and was not run. |
  | QEMU | **Not found** on PATH or in the common install directories checked. The only `qemu-img.exe` on PATH belongs to the Android SDK emulator and is unrelated. A QEMU/WHPX boot is **unverified on real hardware**. That was the state at the time of the read-only check; see the QEMU install below. |
  | Data folder | `D:\ArxcovoData` exists. Free space: C: 600.9 GB, D: 1569.3 GB. |
  | CPU and RAM | 16 logical CPUs, 61.6 GB RAM. |

  The disk cap and owner reserve are not yet configured.

- **QEMU install, 2026-11-23.**
  - The Windows x64 pin `qemu-11.1.3-windows-x64` identifies the portable local build at `D:\ArxcovoData\qemu\11.1.3`. It was built from signed QEMU 11.1.3 sources with MSYS2 UCRT64 and `--enable-whpx`. [ADR 0054](adr/0054-m4-dedicated-windows-runtime.md#amendment-2026-11-23--reviewed-local-qemu-1113-windows-x64-build)
  - The package executables and DLLs are unsigned. Use the reviewed SHA-256 pins. The installation's `RECEIPT.txt` hashes to `e0e33d3fb433c467f5f530bcb8cc8ce41bb8c3b10133bd54718915b7357b852e` and binds `MANIFEST.sha256` at `f1cf73c8ecae163e31467a3985fc13749aee1f12a8747756eb136108a1015759`. An independent re-check verified all 136 payload files.
  - Both executables reported 11.1.3. `-accel help` listed `whpx`. The paused, diskless and networkless `pc-q35-11.1` smoke negotiated QMP, reported `prelaunch` and exited cleanly. Full guest boot and production VM execution are **unverified on real hardware**.
  - The reviewed build contains the virtio-net, block I/O and ATAPI fixes that lift the prior distribution restriction on untrusted guests and persistent VM disks. The superseded Weil 11.1.0 install at `D:\ArxcovoData\qemu\11.1.0` is kept for reference only. The candidate profile refuses its manifest ID.

## Native qualification runbook

**Status: unverified on real hardware.** The reviewed QEMU package and paused,
diskless smoke above are supporting evidence. This runbook adds no native
qualification or acceptance result. Follow the
[ADR 0054 current-user amendment](adr/0054-m4-dedicated-windows-runtime.md#amendment-2026-11-23--current-user-native-runtime-on-the-enrolled-pc).

Run only new Arxcovo user-mode QEMU/WHPX Jobs under owned roots. Leave all
existing VMs, Hyper-V settings, the old Lab Manager installation and state, and
`agent-windows-test` untouched. Hyper-V coexistence checks are read-only and
use WHPX capability and aggregate host counters. Never issue Hyper-V commands
or query existing VM inventory or control state. Attribute evidence to owned
Jobs and abort owned work if reserves drop or there is unexpected host impact.

1. **Collect owner inputs once at qualification.** The coordinator asks once
   for the local paths to the official **Windows 11 Pro x64 en-US ISO** and
   the exact **signed virtio-win revision**, and for one owner-chosen
   console-idle window for the unattended live test. Record the exact positive
   Pro index in `install.wim` or `install.esd` and verify the installed image
   selector, edition, architecture, build and language against the entitled
   media. The owner also supplies the official Ubuntu 24.04.3 live-server
   amd64 ISO, the prepared capped-volume identity and provisioning evidence,
   a separate small disposable fixed-size NTFS trial volume backed by a file
   on D: with its identity and provisioning evidence, the driver qualification
   receipt and controlled witness enrollment. Use step 3 if the trial volume
   is unavailable.
   Validate the Windows answer file in Windows SIM against the exact image.
   Pin sizes and hashes, the viostor, vioserial and NetKVM INF/CAT/SYS files,
   catalog signer and offline trust decision, PCI IDs and port ABI. Extract
   and pin Ubuntu's kernel and initrd from verified media. Pin CODE and VARS,
   the Windows BootNext seed, guest binaries and protocol. Keep raw
   paths, identities and endpoints local. Use no activation key or reusable
   credential. A prompting, unsupported or unverified image fails
   qualification. These exact media choices apply to this host's execution of
   the offline recipes. [ADR 0055](adr/0055-m4-offline-template-recipes.md)

2. **Run read-only preflight from the owner's non-elevated console.** Set and
   verify TMP, TEMP, Cargo target and home, scratch and observer paths under
   the task-owned D: root. Pause large builds below 20 GB free on C:.
   Read and match the actual primary token, current WTS session, Windows x64
   build and `WHvGetCapability(HypervisorPresent)`. Rehash the exact package
   and inputs. Verify separately held package/input roots, fixed-size NTFS
   volume, backing and private store roots. For a restricted backing metadata
   query, combine owner provisioning evidence with live pinned root geometry
   and independently verified outer free space. If physical enforcement cannot
   be established, allow no native allocation. Verify the aggregate 200 GiB
   VM disk cap and at least 50 GiB always free on D:. Measure 16 GiB RAM and at
   least half the effective CPU cores reserved for the owner, including the
   effective nested Job limits and runtime overhead. The initial trial envelope
   allows one running VM, at most 8 GiB guest RAM and 4 vCPUs, within the owner
   reserves. [D: scratch procedure](dev-scratch.md)

3. **Resolve prerequisites identified by read-only preflight.** Collect all
   necessary elevated commands (WHPX, cap correction and trial-volume setup,
   including reattachment after a reboot and deferred rollback) in **one
   reviewed owner request**. Record the exact commands in the local evidence
   and separate docs-only [PROGRESS](PROGRESS.md) batch. Ask the owner once to
   run the applicable setup commands from elevated PowerShell. If cap
   correction needs elevation, derive and review its exact command from
   verified unused target geometry before including it in the batch. Keep
   private target identity data local.
   If the Medium-token check shows WHPX is missing and review confirms the
   remedy, include the DISM line below. Use the D: environment assignments
   for every block in the batch.

   ```powershell
   $env:TMP = 'D:/Temp/arx-m4-activation/tmp'
   $env:TEMP = $env:TMP
   $env:CARGO_TARGET_DIR = 'D:/Temp/arx-m4-activation/target'
   $env:ARX_OBSERVER_ROOT = 'D:/Temp/arx-m4-activation/observer'
   $env:ARX_SCRATCH_ROOT = 'D:/Temp/arx-m4-activation'
   dism.exe /Online /Enable-Feature /FeatureName:HypervisorPlatform /All /NoRestart
   ```

   The DiskPart attachment does not survive a reboot. The owner completes
   any required reboot before setting up the trial volume. After reboot,
   recheck WHPX from the Medium console and confirm the setup preconditions.
   Create a new 1 GiB fixed VHDX on D: only if the trial volume and backing
   file are both absent. A verified unused `T:` provides the real volume
   root required by ADR 0054. Before review and again before running, verify
   that the exact backing file and script paths are unused, their parent
   directories are owned and free of reparse points, and D: will retain the
   50 GiB floor after the full allocation and overhead. Keep the trial within
   the aggregate cap. If a precondition fails, resolve it before including an
   exact reviewed command in the request. The script selects only the new
   VHDX by path and stops on any error. Use DiskPart without Hyper-V cmdlets.
   [Microsoft fixed-VHDX procedure](https://learn.microsoft.com/en-us/windows-hardware/manufacture/desktop/boot-to-vhd--native-boot--add-a-virtual-hard-disk-to-the-boot-menu?view=windows-11#step-1-create-a-vhdx-from-diskpart) · [DiskPart scripts](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/diskpart-scripts-and-examples)

   ```powershell
   @'
   create vdisk file=D:\ArxcovoData\qualify\trial.vhdx maximum=1024 type=fixed
   select vdisk file=D:\ArxcovoData\qualify\trial.vhdx
   attach vdisk
   create partition primary
   format quick fs=ntfs label=ArxcovoTrial
   assign letter=T
   '@ | Set-Content -LiteralPath 'D:\Temp\arx-m4-activation\trial-create.txt' -Encoding ascii -ErrorAction Stop
   diskpart.exe /s 'D:\Temp\arx-m4-activation\trial-create.txt'
   if ($LASTEXITCODE -ne 0) { throw 'Trial volume provisioning failed' }
   ```

   If the host reboots during qualification, keep trial writes closed. Use
   the reattachment block from the same owner request only after verifying
   that the existing VHDX matches the recorded file identity and size at the
   checked path, the read-only `Get-DiskImage` query from step 7 reports it
   detached, and `T:` is unused. Verify the task-owned temporary directory
   before running the block. The block itself refuses a scratch root that is a
   reparse point and a script path that already exists, creates the script
   fresh, and removes it whether or not DiskPart succeeds. The elevated owner session runs the
   `Get-DiskImage` guard, the same as the rest of the block. Whether
   Storage-module queries need elevation is **unverified on real hardware**.
   [DiskPart attachment](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/attach-vdisk)

   ```powershell
   $trialImage = @(Get-DiskImage -ImagePath 'D:\ArxcovoData\qualify\trial.vhdx' -ErrorAction Stop)
   if ($trialImage.Count -ne 1 -or
       $trialImage[0].ImagePath -ne 'D:\ArxcovoData\qualify\trial.vhdx' -or
       $trialImage[0].Attached -isnot [bool]) {
       throw 'Trial volume attachment state is uncertain'
   }
   if (-not ($trialImage[0].Attached -eq $false)) {
       throw 'Trial volume attachment state is uncertain'
   }
   $trialAttachScript = 'D:\Temp\arx-m4-activation\trial-attach.txt'
   $scratchRoot = Get-Item -LiteralPath 'D:\Temp\arx-m4-activation' -Force -ErrorAction Stop
   if (-not $scratchRoot.PSIsContainer -or
       ($scratchRoot.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
       throw 'Re-attach scratch root is not an ordinary directory'
   }
   if (Test-Path -LiteralPath $trialAttachScript) { throw 'Re-attach script path is not fresh' }
   # New-Item without -Force fails if any entry, including a link, already has this name.
   $null = New-Item -ItemType File -Path $trialAttachScript -ErrorAction Stop
   try {
       @'
   select vdisk file=D:\ArxcovoData\qualify\trial.vhdx
   attach vdisk
   '@ | Set-Content -LiteralPath $trialAttachScript -Encoding ascii -ErrorAction Stop
       diskpart.exe /s $trialAttachScript
       if ($LASTEXITCODE -ne 0) { throw 'Trial volume reattachment failed' }
   } finally {
       # Non-terminating, so a cleanup problem never hides the reattachment error.
       Remove-Item -LiteralPath $trialAttachScript -Force -ErrorAction Continue
   }
   ```

   Repeat preflight from the Medium console after any setup. Recheck WHPX,
   physical capacity and the free-space floor. Before allowing trial writes,
   verify the backing file identity, size and full allocation, and prove
   that the mounted `T:` root is the trial volume with the pinned identity
   and NTFS geometry. After reattachment, compare these checks with the
   original evidence. If `T:` is absent or any check fails, stop and retain
   the file. If the QEMU package is missing, install
   only the exact reviewed portable payload under the owned package root and
   rerun the receipt, manifest and payload checks. The scoped prerequisite
   exception covers WHPX, the reviewed QEMU package and the disk cap and
   reserves. Host account, firewall, tracing and VM administration are outside
   this procedure.
   [Arxcovo agent instructions](../AGENTS.md)

4. **Run independently reviewed non-elevated native trials.** Review the
   amendment and exact runner and enforcement code first. For the paused
   trial, use the reviewed stock 11.1.3 bundle and a fresh evidence directory.
   The enrolled trial root must provide an empty `paused-cwd` directory.
   The checked directories grant the current user read/execute access and
   reserve writable ACEs for SYSTEM and Administrators. A directory owned
   by the current user can still have its DACL changed by that account.
   This follows ADR 0054's same-account trust boundary.

   **Owner admin step.** Close any QEMU or qualification process using
   this bundle. In an elevated PowerShell under the enrolled owner's
   login, run the following with the existing enrollment JSON. This
   replaces ACLs only inside the reviewed bundle and on `paused-cwd`.
   It sets Administrators as the owner so the unelevated supervisor does
   not retain implicit DACL control over these paths. The qualification
   runner itself still runs later from the non-elevated owner console.

   ```powershell
   $ErrorActionPreference = 'Stop'
   $enrollmentPath = Read-Host 'Full path of the existing qualification enrollment JSON'
   $enrollment = Get-Content -LiteralPath $enrollmentPath -Raw | ConvertFrom-Json
   $owner = [System.Security.Principal.SecurityIdentifier]::new($enrollment.owner_sid)
   $identity = [System.Security.Principal.WindowsIdentity]::GetCurrent()
   $principal = [System.Security.Principal.WindowsPrincipal]::new($identity)
   if ($owner -ne $identity.User -or
       -not $principal.IsInRole([System.Security.Principal.WindowsBuiltInRole]::Administrator)) {
       throw 'Use the elevated enrolled owner login'
   }
   $system = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-18')
   $admins = [System.Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')

   function Get-OrdinaryItem([string]$path) {
       $item = Get-Item -LiteralPath $path -Force
       if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
           throw 'A setup path is a reparse point'
       }
       return $item
   }

   $bundle = 'D:\ArxcovoData\qemu\11.1.3'
   if ([IO.Path]::GetFullPath($enrollment.stock_bundle) -ne $bundle) {
       throw 'Expected the enrolled stock 11.1.3 bundle'
   }
   $trial = [IO.Path]::GetFullPath($enrollment.trial_root)
   if ($trial -notmatch '^T:\\[A-Za-z0-9 _.-]+(?:\\[A-Za-z0-9 _.-]+)*$' -or
       $trial -ne $enrollment.trial_root -or
       ($trial.Split('\') | Where-Object { $_.EndsWith('.') -or $_.EndsWith(' ') })) {
       throw 'Expected the exact enrolled directory on the trial volume'
   }
   foreach ($path in @('D:\', 'D:\ArxcovoData', 'D:\ArxcovoData\qemu', $bundle, 'T:\')) {
       if ((Get-OrdinaryItem $path) -isnot [IO.DirectoryInfo]) { throw 'Expected a directory' }
   }
   $parent = 'T:\'
   foreach ($part in $trial.Substring(3).Split('\')) {
       $parent = Join-Path $parent $part
       if ((Get-OrdinaryItem $parent) -isnot [IO.DirectoryInfo]) { throw 'Expected a directory' }
   }

   $pending = [Collections.Generic.Queue[IO.FileSystemInfo]]::new()
   $nodes = [Collections.Generic.List[IO.FileSystemInfo]]::new()
   $pending.Enqueue((Get-OrdinaryItem $bundle))
   while ($pending.Count -gt 0) {
       $node = $pending.Dequeue()
       if ($node.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse point in bundle' }
       $nodes.Add($node)
       if ($node -is [IO.DirectoryInfo]) {
           foreach ($entry in Get-ChildItem -LiteralPath $node.FullName -Force) {
               if ($pending.Count + $nodes.Count -ge 512) { throw 'Bundle exceeds setup bound' }
               $pending.Enqueue($entry)
           }
       }
   }

   $cwd = Join-Path $trial 'paused-cwd'
   if (-not (Test-Path -LiteralPath $cwd)) {
       New-Item -ItemType Directory -Path $cwd | Out-Null
   }
   $cwdItem = Get-OrdinaryItem $cwd
   if ($cwdItem -isnot [IO.DirectoryInfo] -or (Get-ChildItem -LiteralPath $cwd -Force | Select-Object -First 1)) {
       throw 'The paused working directory must be empty'
   }
   $nodes.Add($cwdItem)

   # Protect descendants before their parents change inherited access.
   for ($index = $nodes.Count - 1; $index -ge 0; $index--) {
       $node = $nodes[$index]
       $checked = Get-OrdinaryItem $node.FullName
       if (($checked -is [IO.DirectoryInfo]) -ne ($node -is [IO.DirectoryInfo])) { throw 'Setup object type changed' }
       $acl = if ($checked -is [IO.DirectoryInfo]) {
           [System.Security.AccessControl.DirectorySecurity]::new()
       } else {
           [System.Security.AccessControl.FileSecurity]::new()
       }
       $acl.SetOwner($admins)
       $acl.SetAccessRuleProtection($true, $false)
       foreach ($sid in @($system, $admins, $owner)) {
           $rights = if ($sid -eq $owner) { 'ReadAndExecute' } else { 'FullControl' }
           $acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new(
               $sid, [System.Security.AccessControl.FileSystemRights]$rights, 'Allow'
           ))
       }
       Set-Acl -LiteralPath $checked.FullName -AclObject $acl
   }
   ```

   [Windows ACL inheritance](https://learn.microsoft.com/en-us/windows/win32/api/aclapi/nf-aclapi-setnamedsecurityinfow#remarks)

   **Owner rollback.** After the trial settles and all holds are released,
   use the same elevated console and recheck the bundle and working
   directory paths with the reparse-point and empty-directory checks above.
   Run `icacls.exe $bundle /reset /T /L` to replace the protected bundle
   DACLs with the current parent inheritance. Proceed only if it exits 0.
   Remove the checked empty working directory with
   `Remove-Item -LiteralPath $cwd`. Administrators remains the bundle owner.

   From the non-elevated owner console, invoke the qualification runner
   with `--paused-trial` before the existing enrollment and evidence
   arguments. The trial selects a diskless and networkless
   recipe, holds the entire inventory through stop, and records an
   incomplete diagnostic. A retained `.arx-paused-trial-charge` under the
   trial root indicates unsettled custody and prevents another trial.
   Reconcile it using independent release evidence before retrying.
   Later helpers and recipes require individual reviewed rows and qualification.
   [ADR 0054 held bundle inventory](adr/0054-m4-dedicated-windows-runtime.md#amendment-2026-11-24--owner-fallback-to-held-bundle-inventory)

   Prove `pc-q35-11.1` WHPX boot without fallback. On the held child, verify the
   11.1.3 QMP greeting, negotiation, paused prelaunch, typed responses and
   clean release. Prove the installed boot's private pipe ACL, server PID and
   Job membership before exchanging a nonce. Verify disconnect handling and
   the guest script capability across cold boots, including firmware and
   coalesced port events within the bounded retry policy. Refuse any stage
   whose held bundle or confined inputs cannot be established.
   Exercise token, Job, ACL, wrong-PID, slot and path refusals and scoped
   supervisor loss. Test disk exhaustion on the disposable trial volume
   supplied under step 1, with pre-allocation denial and no pressure on
   shared D:. Exercise recent input, idle, lock/unlock, switch/disconnect and
   ambiguous WTS, as well as CPU/RAM/disk reserve loss and bounded quiescence.
   Use owner-arranged unrelated-principal and NETWORK witnesses. Missing or
   inconclusive witnesses prevent a complete record.

   Verify the restricted installer/TEST network against known-live controlled
   IPv4, IPv6, DNS, host and slirp aliases, LAN/management and public witnesses.
   Start with a benign probe guest and repeat on the installed guests with their
   actual drivers. Prove no IPv6, forwarding, sharing or TAP in that class.
   Give the static HTTPS loopback origin its own listener ownership, TLS and
   egress proof. Docker requires its separately pinned patched QEMU/libslirp
   and public IPv4 TCP/UDP and DNS, deny coverage and fail-closed route-drift
   evidence. Exercise route-drift cases only on isolated representative
   infrastructure. M4 and static origin qualification require no Cloudflare
   credential. Any publishing trial is separate and defaults off.
   [ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md) · [ADR 0064](adr/0064-v1-scope-docker-static-vm.md)

   Boot the exact Ubuntu offline recipe with the pinned ISO, CIDATA and
   kernel/initrd. Verify the cleaned standalone candidate, detached installed
   health and clean release of its disposable verification overlay. For Windows,
   observe headless offline Setup/OOBE without a key, actual WinPE drive
   letters, signed drivers, one-shot BootNext and subsequent resets into disk
   boot. Prove clean detached SCM and guest health, followed by disk-only
   Sysprep. Cold-boot a verification overlay with all installation media
   detached. Check offline OOBE, boot and port readiness, and absence of
   cached answers, setup task, profile/bootstrap, media and credential residue
   before seal. Boot two Windows clones independently and prove distinct
   machine SID, hostname and per-VM identity without exporting their values.
   Apply ADR 0054's installer deadlines, reset limits and release gates.

5. **Verify installed script containment and release.** In the Windows guest,
   verify LocalService in session 0, the service privilege allowlist, raw
   port-class and service SID ACLs through re-enumeration, the effective
   execution policies and the Windows PowerShell 5.1 self-test. Confirm the
   Low script token, deny-only groups, privilege removal, Job cancellation
   and work directory release. Use bounded negative probes for protected
   writes, device and parent access, Task Scheduler, SCM, WMI and COM.
   Verify the required logging policy values and absence of event 4104 for the
   bounded probe script. Verify the Ubuntu installed service, boot identity,
   health and clean guest stop. Record typed results only.
   [ADR 0059](adr/0059-windows-guest-script-identity.md)

6. **Hash and seal the evidence.** Write one attributable run under the owned
   evidence root on D:. Give every run a fresh, empty, private evidence
   directory; once the runner has claimed it, retry in a new directory.
   The allowlisted record set is `inputs.json`,
   `host-baseline.json`, `distribution.json`, `custody-job-capacity.json`,
   `activity-reserves.json`, `whpx-qmp-endpoint.json`, `network-<class>.json`,
   `recipe-<id>.json` and `cleanup.json`. Record scope, input hashes and lengths,
   collector and release IDs, monotonic intervals, UTC validity, typed results
   and correlation IDs. Measure byte lengths and compute
   `Get-FileHash -Algorithm SHA256 -LiteralPath` over the allowlisted files
   through checked, owned paths. Canonical `qualification.json` references
   that SHA-256 and byte-length manifest, and local enrollment pins the
   SHA-256 of `qualification.json`. Select only a complete reviewed record.
   It expires after at most seven days or earlier on material drift. Keep
   raw paths, SID values and endpoints in the local enrollment. Publish only
   sanitized predicates and approved hashes.

   The reviewed 136-file QEMU 11.1.3 payload receipt digest is
   `e0e33d3fb433c467f5f530bcb8cc8ce41bb8c3b10133bd54718915b7357b852e`.
   Its manifest digest is
   `f1cf73c8ecae163e31467a3985fc13749aee1f12a8747756eb136108a1015759`.
   Use ADR 0054 and the manifest for the executable and remaining payload
   pins. These hashes identify the stock build. Bind the exact distribution,
   contract version, held inventory, protected search scope, confinement,
   and per-stage qualification and release results. No native trial or
   acceptance hashes exist yet.
   After the issuer (7c), fresh qualification and the required acceptance
   dependencies are in place, run the one unattended live test
   in the chosen console-idle window. Seal `acceptance.json` and its manifest
   with the acceptance client, qualification and deployed release digests.

7. **Roll back on failure or inconclusive evidence.** Close native admission
   and drain owned workers. Request bounded graceful QMP stop and prove held
   child exit, empty Job, joined workers and exclusive reopen before settling
   the owned generation. Quarantine and retain charges if release is uncertain.
   Revoke local qualification selection and preserve the ledger, evidence,
   static VMs and backups. Remove only attributable disposable trial or TEST
   artifacts through the proven lease and cleanup path. Leave QEMU, system
   capabilities and the forward-only store in place. If a separate B2
   publishing trial ran, withdraw only its attributed public route through
   B2 unpublish.

   After the trial or during rollback, the owner runs the deferred command
   from the step 3 batch only for the VHDX created by that batch. First prove
   all trial handles are released, and the backing file matches the recorded
   identity under the same checked path. If detaching, verify the script path
   is fresh under the task-owned scratch root. Query the exact image path and
   skip detach only when it reports `Attached = False`. A reboot or missing drive
   letter alone is insufficient. If release, identity, attachment state or
   detach is uncertain, retain the file and its charge.
   [Get-DiskImage](https://learn.microsoft.com/en-us/powershell/module/storage/get-diskimage?view=windowsserver2025-ps)

   ```powershell
   $trialImage = @(Get-DiskImage -ImagePath 'D:\ArxcovoData\qualify\trial.vhdx' -ErrorAction Stop)
   if ($trialImage.Count -ne 1 -or
       $trialImage[0].ImagePath -ne 'D:\ArxcovoData\qualify\trial.vhdx' -or
       $trialImage[0].Attached -isnot [bool]) {
       throw 'Trial volume attachment state is uncertain'
   }
   if ($trialImage[0].Attached) {
   @'
   select vdisk file=D:\ArxcovoData\qualify\trial.vhdx
   detach vdisk
   '@ | Set-Content -LiteralPath 'D:\Temp\arx-m4-activation\trial-detach.txt' -Encoding ascii -ErrorAction Stop
       diskpart.exe /s 'D:\Temp\arx-m4-activation\trial-detach.txt'
       if ($LASTEXITCODE -ne 0) { throw 'Trial volume detach failed' }
   }
   Remove-Item -LiteralPath 'D:\ArxcovoData\qualify\trial.vhdx' -ErrorAction Stop
   ```

## Foreground VM profile: state root custody

`arxd --config FILE` with a `test_vms` section serves the foreground VM tools from the configured `state_dir`. That directory must be an existing, owner-private, ordinary local directory: no link or reparse point, and not inside the protected service workspace or the executable's folder.

- **Windows.** From validation until the process exits, the foreground holds the state directory **and every ancestor up to the drive root** open without delete sharing. While it runs, nobody can rename, move or delete the state directory or any parent folder; Explorer and `Move-Item` fail with a sharing violation. Stop the foreground (Ctrl+C, or end the process) before you rename or move `state_dir` or a folder above it.
- **macOS.** The state directory and every ancestor must have no extended ACL, because an ACL can grant another account rename or delete rights that the mode bits do not show. Check with `ls -lde PATH` and remove entries with `chmod -N PATH`, or pick a location without ACLs. Unreadable ACLs are refused too.
- **Linux and macOS.** Every ancestor must be owned by you or root and must not be group- or other-writable, unless it is sticky (like `/tmp`) and the next entry is yours. Only your own account can then rename a component. POSIX is diagnostic-only; native VMs stay unavailable there.

## Owner actions (historical; superseded on this PC by the 2026-11-23 update above)

Kept for reference only. On this PC the execution account is the owner's current login, and agents perform the WHPX, QEMU and disk-cap steps under the scoped exception above. Do not follow the dedicated-account or owner-install steps below here.

- [ ] Create a dedicated local Windows account named `Arxcovo`, with account type **Standard user**. Keep its password out of chat, repository files and logs. ADR 0009 specifies S4U without collecting the account password; that mechanism remains unverified. A different mechanism requires a reviewed ADR, never an automatic password fallback. Do not add this account to Administrators or give it access to personal folders.
- [ ] Keep the account out of Hyper-V Administrators too. Standard-account WHPX execution still requires a real probe; report failure rather than increasing its privileges. See docs/adr/0003-whpx-test-vm-spike.md.
- [ ] Enable firmware virtualization if it is disabled. In Windows Features (`optionalfeatures.exe`), enable **Windows Hypervisor Platform**, then restart if requested. Do not disable host security features or change existing Hyper-V VMs. QEMU's WHPX backend requires this Windows feature; merely having QEMU installed is insufficient.
- [ ] Install a Windows QEMU distribution reached through [QEMU's download page](https://www.qemu.org/download/#windows). Make `qemu-system-x86_64.exe` and `qemu-img.exe` readable/executable by the dedicated account. Record the chosen version and installation directory locally. Agents will verify its accelerator support; installation alone does not prove WHPX works.
- [ ] Keep the pinned QEMU installation read/execute-only for the account. Designate private network probe targets locally. Any owner-installed firewall rules are defense in depth and must be tested; a program-path rule alone is not a complete sandbox.
- [ ] Create a separate data folder, for example `D:\ArxcovoData`, with subfolders for templates, test VMs and evidence. Give the dedicated account modify rights there. Do not reuse Lab Manager paths, VM disks or state. Avoid granting broad access to the rest of the drive.
- [ ] Configure a disk cap before real tests: start with **200 GiB aggregate** for Arxcovo data, maintaining **50 GiB free space** on the host volume. The application must account for templates, linked clones, snapshots and temporary install files together. Until that enforcement exists, do not launch unattended VM workloads. If an OS-enforced quota is added, have the owner configure and verify it; a qcow2 virtual capacity or folder name is not a physical-space cap.
- [ ] Configure opportunistic-host reserves before real tests: initially reserve **16 GiB RAM** and **half the logical CPUs** for the owner. Start with at most one VM, **8 GiB RAM and 4 vCPUs**, and adjust downward if these reserves or current free memory cannot be maintained. These are initial limits, not measurements of workload safety. Arxcovo must enforce them and defer VM work while the user is active as specified in the kickoff.
- [ ] Supply an official Windows 11 ISO and an official Ubuntu image/ISO through a local path accessible to the dedicated account. Do not commit installation media or license material. Windows-host Windows 11 guests are TEST VMs only; template installation uses the planned test-only TPM/Secure Boot bypass.
- [ ] When a verified service installer is ready, the owner installs `arxd` with its protected service identity and configures the separate dedicated build account. The build account must not read or modify service configuration, agent credentials, private keys or the activity database. Grant only the specific logon rights required by the verified execution mechanism. Keep standard build rights selected by default. Agents must not silently fall back to the owner's login when dedicated-account execution fails. The platform identity-switching mechanism still needs implementation and verification; do not apply speculative privilege or service changes from research notes.

## Agent verification after setup (historical; superseded on this PC by the 2026-11-23 update above)

Run these checks as the dedicated account, recording bounded outputs without credentials:

1. Confirm `whoami` identifies the dedicated account and an elevated/admin operation is refused. Confirm the configured folder permissions and resource limits.
2. Check `qemu-system-x86_64 --version`, `qemu-system-x86_64 -accel help` and `qemu-img --version`. Then exercise an isolated disposable WHPX boot; an accelerator appearing in a help list does not establish that it runs.
3. From Claude Code through MCP, list host capabilities and run `cargo --version` under the dedicated account. Confirm the activity feed records the caller identity, dedicated execution account and outcome. Install the Rust toolchain for that account if needed through the owner's setup path.
4. Before guest workload execution, verify the TEST network cannot reach the home LAN or host management plane, while permitted guest-to-guest paths work. Verify the agent command and guest channels do not bypass these fences.
5. Exercise real Windows/Ubuntu creation, guest commands, snapshot break/restore, expiry cleanup, disk-cap rejection and RAM/CPU reserve rejection. Observe actual process memory and disk allocation. Leave every unexercised path labelled **unverified on real hardware**.

Sources: [QEMU WHPX prerequisites](https://www.qemu.org/docs/master/system/whpx.html), [Microsoft virtualization hardware requirements](https://learn.microsoft.com/windows-server/virtualization/hyper-v/host-hardware-requirements).

## Owner-authorized M2 setup on this PC (2026-11-18; historical, superseded by the 2026-11-23 update above)

Dylan has designated this Windows PC for applicable M2 native acceptance. Before
agents run it, the owner must complete this short subset of the checklist above:

- Create the dedicated `Arxcovo` Standard account, outside Administrators and
  Hyper-V Administrators. Keep its password out of chat and logs.
- Create `D:\ArxcovoData` for test data, builds and evidence; grant the account
  modify access there without broad D: or personal-folder access.
- Configure and verify a 200 GiB aggregate disk cap with at least 50 GiB free on D:,
  reserving 16 GiB RAM and half the logical CPUs for the owner. A folder name or
  planned setting is not enforcement; native workloads wait for verified fences.
- Install required Rust/build tools for that account. Any protected service
  installation or required logon rights are owner steps once the verified installer
  is ready; do not make speculative service/privilege changes.

The coordinator sent this checklist to Dylan before any native run. Account
existence and enforcement remain unverified; no owner credentials were requested.
Run acceptance only as that account after preflight; no fallback to the owner's
login. All task data, build output and TMP/TEMP are on D:. QEMU/WHPX and official
Windows/Ubuntu guest media are later M4 requirements, unnecessary for M2 alone.
Linux-specific A7/A11 sudo/NNP and A12 need a qualifying Linux host and remain
hardware-pending. Existing Lab Manager paths/state and agent-windows-test stay
untouched. No admin or native acceptance action has been performed here.

## Static VM public-egress build

ADR 0064 uses the separate Windows x64 build at
`D:\ArxcovoData\qemu\11.1.3-arx-egress-2`. Its executable contains the
patched libslirp. Verify its receipt and full payload manifest against the
checked-in pin before using it for owner qualification. The paused WHPX
smoke covered startup and the loopback listener. Guest boot, egress under
traffic and route drift are **unverified on real hardware**. The public
profile remains gated on owner-host qualification.
[Build and policy contract](../tools/qemu/README.md) ·
[Build receipt pin](../tools/qemu/public-egress-receipt.json)
