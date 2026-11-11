# 0004: Windows 11 unattended TEST templates

Status: proposed from the early WHPX/install research. No install has run. **Unverified on real hardware.**

## Scope and supported facts

Keep Windows-host Windows 11 guests TEST-only, using QEMU/WHPX, owner-supplied official media and the settled test-only TPM/Secure Boot install bypass. Do not ship a Windows image, license key or owner credential. Linux-host swtpm and static Windows VMs are outside this step.

Microsoft's requirements include UEFI/Secure Boot capability, TPM 2.0, at least two CPU cores, 4 GB RAM and 64 GB storage. The planned bypass does not establish that those requirements are met. Microsoft documents windowsPE RunSynchronous execution in system context; this is the proposed unattended setup hook. The spike established no official stable contract for the LabConfig bypass values, so support must be checked against each pinned ISO rather than inferred from an older Windows build.

Sources: [Windows 11 requirements](https://learn.microsoft.com/en-us/windows/whats-new/windows-11-requirements), [setup RunSynchronous](https://learn.microsoft.com/en-us/windows-hardware/customize/desktop/unattend/microsoft-windows-setup-runsynchronous).

## Proposed build contract

- Identify a template by guest architecture, official ISO SHA-256, QEMU/firmware versions, driver bundle hashes, answer-file version and guest-agent version. Never substitute a different ISO silently after an install fails.
- Start under the dedicated standard host account, within its data directory and enforced RAM/CPU/disk limits. The owner enables WHPX beforehand. Use no TCG fallback or host privilege escalation to make a probe pass.
- Generate the answer file with explicit TEST scope. Apply the necessary test-only setup bypass in windowsPE, then complete installation/OOBE and install the versioned arx-guest service. Treat offline local-account OOBE, unattended completion and reboot sequencing as unknown until exercised on the selected ISO.
- Pin and verify the Windows virtio driver bundle required for the chosen disk/NIC/serial devices. Do not assume the official Windows ISO already contains virtio-serial drivers. Review the [virtio-win project](https://github.com/virtio-win/virtio-win-pkg-scripts) and the distribution's signatures/provenance before selecting a bundle.
- Use the network fence in ADR 0003 and the serial guest channel for provisioning. Test-only bypass is not a reason to expose the home LAN, host management plane, QMP or host files. Required package delivery and permitted guest networking need explicit acceptance; an isolated boot alone does not prove them.
- Keep generated credentials out of repository files, command-line logs and MCP responses. The setup agent's provisioning material must be removed or rotated before cloning. Activation/licensing choices and official media availability belong to the owner.
- Stop the installer on a three-hour deadline, retaining only bounded redacted setup/serial diagnostics and the exact failed stage. A readiness marker requires authenticated guest-agent execution after a fresh boot; an answer file merely being generated is not success.
- Shut down and seal the golden disk and matching UEFI VARS before cloning. Keep them immutable. Snapshot/restore uses the cold-boot disk/VARS semantics of ADR 0003, without claiming saved CPU/RAM state.

## Required early probe

After owner setup, install from the selected ISO without interactive input, observe guest-agent readiness, run a script, reboot and run it again. Prove the host account is standard and forbidden network paths fail. Build a linked clone, change a marker, take a stopped snapshot, break it, restore and cold-boot; verify the marker and untouched backing template. Exercise expiry and physical disk accounting separately. Record the actual ISO/build/driver versions and failures. None of these checks has passed yet.
