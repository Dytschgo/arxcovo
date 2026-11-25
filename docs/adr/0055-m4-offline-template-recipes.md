# ADR 0055 — M4 offline template recipes

**Status** Proposed pending owner-host qualification.

Arxcovo builds private Windows and Ubuntu TEST templates from pinned offline inputs. Each successful build captures a matching disk and private firmware VARS pair. The detached verification boot uses a disposable linked overlay and a fresh VARS copy. The common lifecycle publishes the candidate atomically after clean release, then creates stopped linked TEST clones with their own disk and VARS overlays. [ADR 0004](0004-windows-11-test-template-install.md) governs Windows TEST policy. [ADR 0035](0035-bound-sqlite-object-identity.md) governs retained-object store custody. [ADR 0054](0054-m4-dedicated-windows-runtime.md) governs launch, handoff and admission.

## Provenance and qualification

| Recipe | OS installer and selector | Qualification |
| --- | --- | --- |
| [ubuntu-noble-amd64-v1](../../crates/arxd/src/vm/template/ubuntu.rs) | Official `ubuntu-24.04.3-live-server-amd64.iso`, ISO digest and NoCloud schema as pinned in the recipe. The owner supplies verified kernel, initrd, firmware, QEMU and `arx-guest` assets. | Native launch remains subject to ADR 0054 and real install evidence for the exact host and pins. |
| [windows11-x64-test-v1](../../crates/arxd/src/vm/template/windows.rs) | One official owner-supplied Windows 11 x64 ISO. Evaluate Enterprise if the owner already has suitable media and entitlement. Inspect the exact ISO locally for the `install.wim` or `install.esd` container, positive index, edition ID and name, architecture, build and language. The current candidate uses en-US. | `recipe_unqualified`. The owner ISO, edition, signed virtio-win media and receipt, guest binary, QEMU receipt and matching UEFI CODE/VARS pins are pending. Exact-media boot, Setup, OOBE, driver and Sysprep evidence is pending. |

Windows binds ISO size and SHA-256, observed image metadata, virtio-win size and SHA-256, the three selected packages' member pins, the driver qualification receipt, WinPE driver letter, QEMU receipt, firmware pair, guest binary and protocol version, provisioner, answer settings and topology into the recipe digest. The owner driver receipt identifies the source release, file inventory, catalog signer and offline trust decision, versions, PCI matches and vioserial ABI. The caller inspects those fixed paths from the held driver ISO and supplies bounded readers. The recipe compares the observed edition and hashes every required asset before output. R1 and 5c bind the observations, receipts and reads to the same held objects that QEMU opens. New exact pins produce a new digest. Referenced goldens retain their original digest.

| Emulated hardware | Pinned Windows driver or medium |
| --- | --- |
| `virtio-blk-pci` | `viostor\w11\amd64`, fixed INF/CAT/SYS, loaded in WinPE for disk 0 |
| `virtio-serial`, `org.arxcovo.agent.0` | `vioserial\w11\amd64`, fixed INF/CAT/SYS and validated port ABI |
| Single `virtio-net-pci` | `NetKVM\w11\amd64`, fixed INF/CAT/SYS. Keep `user,restrict=on,ipv6=off` with no forwarding, share or network boot. |
| Built-in q35 AHCI CD | Official OS ISO, raw read-only optical media |
| Two separate fixed emulated AHCI CD ports | Generated answer seed and external signed virtio-win distribution ISO, distinct raw read-only storage objects |

R1 pins the exact controller addresses and accepts only the closed OS/seed/driver role set for Windows install. The two boot roles belong to the official ISO and the system disk. The owner must observe one-shot UEFI CD selection, Setup reboots into Windows Boot Manager on disk, repeatable answer discovery, correct drive letters, signed driver acceptance and the actual network fence.

## Offline setup and cleanup

The seed is a bounded, deterministic ISO9660/Joliet image with a fixed `ARXSETUP` label. Joliet carries the root `Autounattend.xml` name. Both namespaces reference the same file extents. The seed also contains the fixed discovery marker, `arxguest.exe`, `arxsetup.ps1` and `arxpins.txt`. The answer file pins the script and manifest, and the manifest pins the guest binary and script. Its generator accepts typed image selection and a single vetted drive letter. Recipe text and commands come from fixed literals.

Windows Setup uses the `/IMAGE/INDEX` selector and partitions the sole managed virtio-blk disk as GPT with EFI, MSR and Windows partitions. The TEST `windowsPE` pass writes `BypassTPMCheck` and `BypassSecureBootCheck` to the WinPE LabConfig hive. These are the two allowed checks for the Windows TEST recipe. Signed driver enforcement remains active throughout Setup and installed boot.

`specialize` runs the provisioner under SYSTEM through Deployment `RunSynchronous`. It locates a unique optical seed marker, verifies the staged script and manifest, copies and verifies the executable, then registers `arx-guest` as a demand-start own-process service under `NT AUTHORITY\LocalService`. The service has an unrestricted service SID and the `SeChangeNotifyPrivilege` and `SeIncreaseQuotaPrivilege` allowlist from the guest adapter contract. Administrators and SYSTEM control the installed executable and phase state. The work root grants its service SID create-subdirectory and delete-child with a Low integrity label.

The post-Setup task writes and verifies the binary `Security` override under `HKLM\SYSTEM\CurrentControlSet\Control\Class\{6FDE7547-1B65-48AE-B628-80BE62016026}\Properties` before the first shutdown. This is the dedicated class that vioserial supplies to `WdfPdoInitAssignRawDevice` when it creates a port PDO. KMDF uses that registry override when the PDO is created. The protected DACL grants SYSTEM and Administrators full access and the service SID read/write. The class value is independent of a specific PnP instance and requires no persistent task. The driver receipt must confirm this raw class for the exact signed pin. Its persistence through Sysprep and effective service access and script-token denial after re-enumeration are **unverified on real hardware**. [Virtio port creation](https://github.com/virtio-win/kvm-guest-drivers-windows/blob/master/vioserial/sys/Port.c) · [Virtio class definition](https://github.com/virtio-win/kvm-guest-drivers-windows/blob/master/vioserial/sys/vioser.h) · [KMDF raw-device registry override](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/wdfpdo/nf-wdfpdo-wdfpdoinitassignrawdevice) · [WDK example for the Properties key](https://github.com/microsoft/Windows-driver-samples/blob/main/general/cancel/startio/cancel.h)

On the first detached boot, the task sets and verifies these four image-local machine values as DWORD `0` under `HKLM\SOFTWARE\Policies\Microsoft\Windows\PowerShell`.

| Subkey | Value |
| --- | --- |
| `ScriptBlockLogging` | `EnableScriptBlockLogging` |
| `ScriptBlockLogging` | `EnableScriptBlockInvocationLogging` |
| `ModuleLogging` | `EnableModuleLogging` |
| `Transcription` | `EnableTranscripting` |

The task flushes the phase 2 boot marker after these checks and then configures and starts the automatic service. The service applies the full admission check, including user and machine execution policy, the LocalService policy hive, lockdown policy and the fixed self-test. [ADR 0059 from PR 273](https://github.com/Dytschgo/arxcovo/blob/91c39bfc09fee25ea4c500ed826caf271c3f3f1f/docs/adr/0059-windows-guest-script-identity.md)

`oobeSystem` uses documented privacy and offline screens and one standard local bootstrap identity with an omitted password. SYSTEM disables and removes it after `IMAGE_STATE_COMPLETE` and after the OOBE flags clear, so no interactive first logon is required. The earliest safe local-logon denial point depends on when the selected SKU creates and stops using that account. Qualification must confirm that point and cover the preceding interval within the isolated, headless install. The recipe supplies no reusable credential, product key, autologon, network setup action, or recovery credential. If the selected edition prompts for a key, requires a credential or cannot complete offline, the recipe stays unqualified.

The SYSTEM task disables and removes the bootstrap user, checks and removes its profile, clears known attributable cached answer files and the Setup override, and checks a bounded Panther, Sysprep and Setup staging inventory. Ambiguous paths or remaining accounts, cached answers, installer state and auto-logon configuration block success. Owner qualification also inventories protected logs, pending setup tasks, copied media and reusable credentials. Signed packages in the Driver Store and the protected service remain. Receipts use closed stage, reason, cleanup and version fields. They carry no answer bytes, raw guest logs, identity values or owner paths.

## Reboot, Sysprep and publication

R2 runs installer and finalizer boots with QMP-only startup and completion. It observes valid Setup resets within the fixed policy and requires terminal guest shutdown evidence, child exit, Job-empty and exclusive artifact reopen. Each installed readiness boot uses the existing verified serial pipe, fresh challenge and fixed Windows health probe. The installed health checks establish Setup completion and readiness. Use one monotonic three-hour deadline from first install launch through detached verification, with each stage clipped to the remaining budget. Teardown that lacks release proof retains charges and quarantines the artifacts under the common lifecycle.

1. Install with all three CDs. The task checks Setup completion, cleans bootstrap and answer state, verifies provisioned settings and shuts down. Release the held inputs and detach the CDs before booting the candidate again.
2. Cold-boot the disk and matching VARS alone. The task recognizes this boot as validation-only and starts the demand-start service only after cleanup checks, verified logging policy and the flushed phase 2 marker for this boot. Require installed SCM and guest channel readiness, a bounded read-only health check that confirms this boot's marker, and a clean solicited stop.
3. Boot the same disk and VARS again without media, this time in QMP-only finalization. The protected task records each eligible boot once, checks cleanup again, removes itself and its phase files, and invokes inbox Sysprep `/generalize /oobe /shutdown`. Capture the disk and VARS at rest after clean release.
4. Boot the distinct verification overlay and VARS pair with all installation media detached. Require complete offline OOBE within the installed readiness contract, working SCM and re-enumerated drivers, absent cached answers, tasks and bootstrap state, and a clean release. Recheck immutable candidate identity, digests and accounting before sealing. Qualification includes two independently booted linked clones with distinct machine SID, hostname and per-VM identity without exposing those values in receipts.

5a supplies the Windows service and restricted executor. R1 and R2 supply typed media and boot plans, one-shot boot handling and bounded QMP installer completion. 5c supplies the durable lifecycle, fixed health checks, private catalog and qualification evidence. Native activation needs the dedicated host and identity controls of ADR 0054 plus independent exact-head review and the owner-host checks. A real Windows Setup, offline OOBE, signed driver behavior, SCM installation, Sysprep, and distinct clone identities are **unverified on real hardware**.

## TEST guest default route amendment

The offline Ubuntu installer retains its disabled network configuration
(`/etc/netplan/00-installer-config.yaml`) and the shared Ubuntu finalizer
(`UbuntuRecipe::FINALIZE`) and NoCloud profile pin remain unchanged (4,757
bytes, `37aaba5448078914ff93f646cce78d265588f1a14d5d34e662552295aee0572c`), so
static-web and Compose seeds are unaffected. The Ubuntu TEST qualification seed
(`render_test_seed` / `write_test_seed`) appends one installer-side late command
that writes `/target/etc/netplan/60-arxcovo-test.yaml` with DHCPv4 and the
guest-only default route via `10.0.2.2` without rewriting
`00-installer-config.yaml`. The sealed Ubuntu TEST recipe identity shared by
the collector and preflight projection binds `UbuntuRecipe::ID`, the seven
external asset pins, and the embedded TEST recipe content (`SERVICE`,
`FINALIZE`, `NOCLOUD_PROFILE_PIN`, `TEST_ROUTE_COMMAND`, and
`DETACHED_HEALTH`), so a TEST route edit alters the TEST recipe identity while
leaving the shared base, static-web, and Compose seeds and recipe digests
unchanged. Windows writes the same route for the NetKVM NIC
from the privileged provisioning step. The installed probes require an active
route through that gateway. Egress remains subject to the sealed slirp
`restrict=on` policy and the host-side witnesses. The owner-host behavior is
**unverified on real hardware**.

| Pinned input | Previous bytes and SHA-256 | Current bytes and SHA-256 |
| --- | --- | --- |
| Ubuntu shared NoCloud profile | 4,757 · `37aaba5448078914ff93f646cce78d265588f1a14d5d34e662552295aee0572c` | Unchanged (4,757 · `37aaba5448078914ff93f646cce78d265588f1a14d5d34e662552295aee0572c`) |
| Ubuntu TEST route late command (`60-arxcovo-test.yaml`) | None | 416 · `82d4979ece16cef92ade02e11ee459d7cc29e0c1006e7cd33cf6524ae6bff8ea` |
| Windows provisioner | 13,110 · `d12b9627ee20087dcd2478646873a3932d5463b9dbe1af3c8c2056ad766f72cf` | 13,816 · `1c832f7047ce76ede0ff1f627a646f294cf0fa01df2d39a2877b72f77aaa1544` |
