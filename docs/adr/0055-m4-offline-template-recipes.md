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

`specialize` runs the provisioner under SYSTEM through Deployment `RunSynchronous`. It locates a unique optical seed marker, verifies the staged script and manifest, copies and verifies the executable, then registers `arx-guest` as a demand-start own-process service under `NT AUTHORITY\LocalService`. The fixed startup task enables automatic startup after Setup completion, cleanup and the device security property write. The service has an unrestricted service SID and the `SeChangeNotifyPrivilege` and `SeIncreaseQuotaPrivilege` allowlist from the guest adapter contract. Administrators and SYSTEM control the installed executable and phase state. The work root grants its service SID create-subdirectory and delete-child with a Low integrity label. The selected vioserial device grants its service SID read/write. Qualification checks the effective device-object DACL after a cold boot and again after generalization.

`oobeSystem` uses documented privacy and offline screens and one standard local bootstrap identity with an omitted password. SYSTEM schedules cleanup after `IMAGE_STATE_COMPLETE` and after the OOBE flags clear, so no interactive first logon is required. The recipe supplies no reusable credential, product key, autologon, network setup action, or recovery credential. If the selected edition prompts for a key, requires a credential or cannot complete offline, the recipe stays unqualified.

The SYSTEM task disables and removes the bootstrap user, checks and removes its profile, clears known attributable cached answer files and the Setup override, and checks a bounded Panther, Sysprep and Setup staging inventory. Ambiguous paths or remaining accounts, cached answers, installer state and auto-logon configuration block success. Owner qualification also inventories protected logs, pending setup tasks, copied media and reusable credentials. Signed packages in the Driver Store and the protected service remain. Receipts use closed stage, reason, cleanup and version fields. They carry no answer bytes, raw guest logs, identity values or owner paths.

## Reboot, Sysprep and publication

R2 runs installer and finalizer boots with QMP-only startup and completion. It observes valid Setup resets within the fixed policy and requires terminal guest shutdown evidence, child exit, Job-empty and exclusive artifact reopen. Each installed readiness boot uses the existing verified serial pipe, fresh challenge and fixed Windows health probe. The installed health checks establish Setup completion and readiness. Use one monotonic three-hour deadline from first install launch through detached verification, with each stage clipped to the remaining budget. Teardown that lacks release proof retains charges and quarantines the artifacts under the common lifecycle.

1. Install with all three CDs. The task checks Setup completion, cleans bootstrap and answer state, verifies provisioned settings and shuts down. Release the held inputs and detach the CDs before booting the candidate again.
2. Cold-boot the disk and matching VARS alone. Require installed SCM and guest channel readiness, a bounded read-only health check, and a clean solicited stop. The fixed task recognizes this boot as validation-only.
3. Boot the same disk and VARS again without media, this time in QMP-only finalization. The protected task records each eligible boot once, checks cleanup again, removes itself and its phase files, and invokes inbox Sysprep `/generalize /oobe /shutdown`. Capture the disk and VARS at rest after clean release.
4. Boot the distinct verification overlay and VARS pair with all installation media detached. Require complete offline OOBE within the installed readiness contract, working SCM and re-enumerated drivers, absent cached answers, tasks and bootstrap state, and a clean release. Recheck immutable candidate identity, digests and accounting before sealing. Qualification includes two independently booted linked clones with distinct machine SID, hostname and per-VM identity without exposing those values in receipts.

5a supplies the Windows service and restricted executor. R1 and R2 supply typed media and boot plans, one-shot boot handling and bounded QMP installer completion. 5c supplies the durable lifecycle, fixed health checks, private catalog and qualification evidence. Native activation needs the dedicated host and identity controls of ADR 0054 plus independent exact-head review and the owner-host checks. A real Windows Setup, offline OOBE, signed driver behavior, SCM installation, Sysprep, and distinct clone identities are **unverified on real hardware**.
