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

- **QEMU install, 2026-11-23, by an agent under the scoped exception.**
  - The Weil Windows installers linked from qemu.org stop at 11.1.0; no Weil 11.1.3 installer exists. The coordinator re-pinned the Windows binary to the official Weil 11.1.0 x64 build. That pin supersedes the earlier 11.1.3 candidate in ADR 0054, and the code and ADR are updated in a separate PR. MSYS2 publishes a Windows x64 `mingw-w64-ucrt-x86_64-qemu` package, which is the candidate patched build to evaluate, with package signatures and a receipt, before the patched-build gate below.
  - It was extracted portably to `D:\ArxcovoData\qemu\11.1.0` without running the installer, so no UAC prompt was needed.
  - The installer SHA-512 matches the publisher's checksum. The Authenticode certificate (Universität Mannheim) expired before the signing timestamp; the owner approved the install anyway.
  - Receipt: `D:\ArxcovoData\qemu\11.1.0\RECEIPT.txt` (SHA-256 `5b8a9e63…6ab1`).
  - Smoke test: `--version` reports 11.1.0, `-accel help` lists `whpx`, `pc-q35-11.1` is present, and a paused VM with no disk and no network started under WHPX, answered QMP and exited 0.
  - A security review of 11.1.0→11.1.3 found an ATAPI hang, a virtio-net heap bounds fix (CVE-2026-66900) and a block I/O corruption fix. A patched build is required before untrusted guests or persistent VM disks.
  - A full guest boot is **unverified on real hardware**.

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
