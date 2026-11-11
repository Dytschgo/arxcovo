# 0003: WHPX snapshot and network constraints

Status: proposed from an early source/documentation spike. No implementation or hardware acceptance yet. **Unverified on real hardware.**

Keep the settled QEMU/WHPX stack and dedicated standard account. Pin the actual Windows QEMU distribution, bundled network library and firmware before testing; upstream source does not prove the selected binary behaves identically.

## Snapshot semantics

QEMU v11.1.3 registers a WHPX migration blocker for missing dirty-memory tracking and CPU/system-state save/restore support. Its save_snapshot path checks migration blockers. Treat full CPU/RAM checkpoints as unavailable until the pinned binary is probed; do not advertise resumable snapshots.

Proposed TEST snapshot: disk state plus its matching UEFI VARS state. Restore stops QEMU, restores both and cold-boots. Use offline snapshots first for consistency. Any later live disk-only snapshot must report its crash-consistent semantics and coordinate guest writes. Templates remain immutable while linked clones exist. Never run qemu-img mutations against an image owned by a running QEMU process.

Sources: [WHPX blocker, v11.1.3](https://raw.githubusercontent.com/qemu/qemu/v11.1.3/target/i386/whpx/whpx-all.c), [snapshot implementation](https://raw.githubusercontent.com/qemu/qemu/v11.1.3/migration/savevm.c), [qemu-img](https://www.qemu.org/docs/master/tools/qemu-img.html).

## Account and network fence

The owner enables Windows Hypervisor Platform. WHvCreatePartition documentation does not establish whether an ordinary account can initialize the selected WHPX build. Probe that account directly without TCG fallback. Failure is a blocker to resolve, not permission to grant Administrators/Hyper-V Administrators membership.

Default QEMU user networking permits routing through the host. The initial restricted backend is `user,restrict=on,ipv6=off`, with no hostfwd, guestfwd, SMB or TFTP exposure. Forwarding rules can override restriction, so the argument builder must reject them for the fenced backend. Agent commands use virtio-serial. QMP/VNC require a host-owned authenticated broker and OS access controls; loopback binding alone does not isolate other local accounts.

This backend is a fence, not proof of required guest-to-guest reachability. Cross-host guest networking remains a separate capability that needs its own implementation and acceptance within the kickoff's milestone/roadmap boundary. Do not claim it from an isolated guest boot or silently substitute a different network design.

Owner-managed firewall rules for a pinned read-only QEMU path can provide defense in depth, but path rules alone are not a process sandbox: copying an executable or launching another network client may evade them. Validate the actual boundary; do not treat an untested firewall recipe as enforcement.

Sources: [QEMU network options](https://www.qemu.org/docs/master/system/invocation.html), [WHPX prerequisites](https://www.qemu.org/docs/master/system/whpx.html), [WHvCreatePartition](https://learn.microsoft.com/en-us/virtualization/api/hypervisor-platform/funcs/whvcreatepartition).

## Bounded hardware probes after owner setup

1. Under the dedicated standard account, confirm identity/group membership and run WHPX with QMP status under a five-minute timeout, no TCG fallback. Boot a disposable Ubuntu image and observe a serial readiness marker.
2. On an eligible qcow2 test disk, probe full-state snapshot rejection. Separately write a marker, take a stopped disk/VARS snapshot, delete the marker, restore and cold-boot; the marker must return and the template must be unchanged.
3. Through serial commands, test IPv4/IPv6, DNS, host aliases, a designated LAN target, host management and public destinations. All forbidden paths must fail. Confirm QEMU has no unexpected host-side network sockets. Private target addresses stay out of source/evidence exports.
4. Install the owner-supplied Windows 11 ISO unattended under a bounded three-hour timeout. This spike established no official stability contract for LabConfig bypass keys; revalidate the test-only TPM/Secure Boot bypass and offline OOBE for every supported ISO. Never ship the image or a license key.
5. Measure physical disk growth and reserve enforcement before allowing agent workloads. Capture bounded outcomes/errors and exact versions, without credentials. Guest-to-guest success must be proved separately from the negative network tests.

No probe has run. The last read-only preflight found the account absent and QEMU unavailable on PATH. No Lab Manager state, existing VM or administrator setting was touched.
