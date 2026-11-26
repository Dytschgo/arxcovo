# ADR 0054 — Dedicated Windows VM runtime

The VM runtime is a separate foreground candidate for a dedicated local
standard account. Its profile has distinct owner and supervisor SID pins,
a bundle outside the managed volume and a state root inside it. The installed
service and production foreground schemas keep their existing execution and
transport boundaries. The VM controller commits portable intent. Native
launch and media operations require independent, operation-bound authority.
There is no authority issuer in this slice.

## Distribution and launch recipe

The candidate pin for TEST-VM qualification is the reviewed local QEMU 11.1.3
Windows x64 build described in the 2026-11-23 amendment below. Qualification
requires reviewing source and toolchain provenance and pinning the SHA-256
and size of the console QEMU and matching qemu-img executables, the libslirp
and loaded DLL set, ROM/data, and the EDK2 CODE and VARS images. Version
matching in the QMP greeting checks the protocol peer's declaration during
startup. Distribution qualification independently checks the bytes on disk.

The fixed recipe selects WHPX, `pc-q35-11.1`, explicit raw pflash nodes
with a read-only `edk2-x86_64-code.fd` and private writable copy of
`edk2-i386-vars.fd`, explicit disk formats and one restricted IPv4 user
network. Caller configuration cannot extend the recipe. Golden backing is a
single immutable qcow2 role. The helper checks immediate header metadata
before following known backing and requires an offline lease for writable
image metadata and creation. Activation requires immutable lineage and slot
exclusion proven through helper, process and Job release.

The candidate launcher uses an absolute image, a minimal explicit Unicode
environment, an explicit working directory and an exact stdio handle list.
Actual loader search, delay-load behavior and filesystem authority require
dedicated-account qualification with the intended binaries and mounted
roots. The Job confines process lifetime and applies memory and CPU controls.
The dedicated account and filesystem ACLs govern access to files and
credentials.

Launch plans are sealed. `CommandPlan` has private fields, no public
constructor and read-only accessors. Exactly four constructors exist:
`BootRecipe::plan`, golden `info`, writable-disk `info` and `create`. Each one
checks the artifact roles, the immediate qcow2 header, and the size and
peak-allocation ceiling before it returns a plan. The boot recipe takes the
first cluster of the guest disk and of the golden. The disk may name only the
approved golden as its backing file, the golden must be unbacked, and
neither may declare an external data file or an unknown incompatible
feature. A backed create takes the golden's first cluster too, because
qemu-img opens the declared backing with traversal enabled. Those clusters
must be read through the held custody pins. The gap between that read and
QEMU's own by-name open belongs to the activation gate below.

Header parsing follows the QEMU 11.1.x `block/qcow2.h` layout. A version-3
`header_length` of 104 or the pinned 112 is accepted, where 112 adds
`compression_type` and zeroed padding. Compression must be zlib (0), or zstd
(1) with incompatible bit 3. Every other incompatible bit, all compatible
and autoclear bits, and unknown header extensions are refused. Extensions
are bounded by the backing name or the first cluster. Their private helpers
return argv only, never a plan, so no crate-internal caller can produce an
unvalidated plan. Inspecting a writable disk and creating a staging image
both hold a borrowed `OfflineLease` (`PlanKind::InspectWritable` and
`PlanKind::Mutate`). The native launcher requires that lease and keeps it
borrowed for the life of the helper launch. The private process-creation
path takes argv only from a sealed plan in production builds. The attribute
list guard owns copies of the Job-list and handle-list values and destroys
the list before releasing them, on every success and error path. Test fixtures may re-execute only the test binary.
Visibility alone makes caller-supplied argv unrepresentable, so no
compile-fail harness is used. `OfflineLease` and `LaunchAuthority` stay
uninhabited, so no mutating plan or native launch can be constructed in this
slice.

## Channels and containment

QMP uses synchronous child stdin/stdout and separate stderr. Parent workers
bridge to bounded streams and drain stderr without retaining peer text.
The suspended process joins an unnamed kill-on-close Job at creation.
The attribute list limits inheritance to the three intended stdio handles.
Before resuming, the supervisor checks the held child, its Job membership,
and the dedicated token policy, including account, integrity, logon identity,
groups, privileges and default DACL.

The guest connection targets a fresh local pipe basename and the fixed
`org.arxcovo.agent.0` virtio-serial port. The parent starts draining stdio,
resumes the process for device initialization, and connects the serial
client before awaiting QMP greeting completion. The client uses anonymous
SQOS and explicit data rights. The held endpoint's owner and DACL and
server PID must match against the live held Job child before any exchange.
The restricted token's default DACL denies NETWORK and scopes rights to the
supervisor account, the local logon and SYSTEM. The exact QEMU build must
prove endpoint access, instance-creation rights and disconnect behavior.
Reconnect requires fresh endpoint proof. Failed or ambiguous identity
returns a typed refusal.

Same-account access is the accepted trust boundary. The default DACL grants
the supervisor account `FILE_ALL_ACCESS`, which includes
`FILE_CREATE_PIPE_INSTANCE`, `WRITE_DAC` and write-owner rights, so QEMU can
create its endpoint and the supervisor can inspect it. Any process running
under the supervisor SID can therefore also create a competing instance or
rewrite the endpoint descriptor. The runtime cannot tell same-SID processes
apart. The server-PID, liveness, Job-membership and handle-DACL checks bind
one connection to the held child, but they do not exclude same-account code.
The dedicated account must run only the supervisor and its Job children.
Its full DACL access is part of that boundary, not a defect in the
endpoint policy. The serial endpoint stays `GuestEndpointUnverified` until
owner qualification covers the distinct-SID, NETWORK and wrong-PID cases on
the exact build.

QMP uses the startup hold `-S`. The single internal `cont` is available
after valid capabilities, a prelaunch status, qualified endpoint custody and
a fresh resource check. The resource revalidator runs at two points. The
first is immediately before `ResumeThread`, after the cancellation and
held-child checks, and the sample must be within the five-second window
there. The second is between receipt of the prelaunch `query-status` reply
and `cont`. That sample must itself be fresh, and it arms a dispatch gate
with the deadline `captured + 5 s`.

The gate is one state machine: `Idle → Armed → Writing → WrittenInTime`, or
the terminal `Expired` or `Ambiguous`. Once it is armed, only the stdin
worker changes it, and it decides each transition at the pipe write:
- a write may start only before the deadline, which moves `Armed` to
  `Writing`; otherwise the state becomes `Expired`, nothing is written, and
  stdin closes;
- a write that returns before the deadline becomes `WrittenInTime`;
- a late or failed completion, or a worker that exits mid-window, becomes
  `Ambiguous`, and stdin closes.

The QMP acknowledgement never changes the gate. After it, boot waits until
the worker has finished any write it started (`Writing` has resolved), then
accepts only `WrittenInTime` and returns the gate to `Idle`. Every other
outcome refuses. The caller then reconciles the launch through the bounded
stop path, which terminates the Job. A refusal at the first revalidation
point leaves the thread suspended. A refusal at the second sends no `cont`
and closes the channel.

This bounds when the host *dispatches* `cont`. When QEMU reads it and when
the guest begins executing are outside the host's control, and are not
claimed.

Guest readiness for an installed boot has an absolute, fixed budget of
180 s from the accepted `cont`. No recipe, profile or caller can extend it.
Two facts shape the design:
- Firmware can open the port first. OVMF's VirtioSerialDxe opens a named
  port before Linux, QEMU then moves a pending frame into the firmware
  queue, and `ExitBootServices` resets that queue.
- QEMU 11.1.3 rate-limits `VSERPORT_CHANGE` to one event per second per
  device and keeps only the newest pending one, so a quick close and reopen
  can arrive as a repeated open.

Readiness therefore treats any challenge as possibly lost, and relies on
the guest-side change from the separate arx-guest PR: a fresh `Hello` at any
point resets the guest session, and the service answers every `Hello` with
an ack echoing it and keeps serving. The host side works as follows:
- The fixed port device id `arxguestport` is part of the recipe. The bounded
  QMP event set includes `VSERPORT_CHANGE {id, open}`, checked with the same
  envelope rules as every event. The id must match exactly; other ports'
  events are skipped and counted, and a malformed port event refuses.
- The host sends a fresh 128-bit challenge on every reported open of its
  port, including a repeated open.
- It also retries on a 1, 2, 4 and then 8 s schedule while the port is not
  known to be closed, including before any event arrives. Each open
  restarts the schedule at 1 s. A close only pauses the retries until the
  next open; it voids nothing.
- At most 32 challenges are issued per boot; the next one refuses. No `Hello`
  is written at or after the budget's deadline.
- Every challenge is a fresh nonce written only to this boot's verified
  pipe. An ack echoing any of them, with script support advertised, proves
  the guest service is live in this boot and completes readiness. A
  challenge not issued in this boot, or a malformed or oversized frame,
  refuses.
- The command channel inherits the readiness framer's buffered bytes, so
  nothing read past the accepted reply is lost. It skips a late ack of each
  other challenge from this boot at most once, before matching responses.
- Once a reply's first byte arrives, the frame must complete within 10 s. A
  result that completes at or after any deadline still refuses, because
  Tokio polls ready I/O before an expired timer.
- Cancellation stops the wait at once.
- There is no serial reconnect: the one verified pipe handle is kept
  throughout.

No systemd restart is needed for convergence. A stale `Hello` and a fresh
one both reach the service, which answers each, and the host accepts the
first valid echo. This design, the guest-side reset it depends on, the
firmware behavior, QEMU's event coalescing and timing, the retry schedule
and the 180 s budget against real cold boots are **unverified on real
hardware** against the pinned QEMU, EDK2 and guest binaries. Readiness on a
real guest also requires the guest-side reset PR to land first.

The initial slice refused installer plans with `InstallerPhaseUnavailable`.
The installer amendment below supplies their QMP-only startup and poweroff
observation. Production dispatch remains subject to the activation gates.

Graceful stop requires a
matching guest shutdown event, a clean child exit, Job-empty, worker joins
and artifact release. Containment termination yields a dirty release when
all custody checks pass. Inconclusive release keeps the generation charged
for reconciliation.

## Storage and owner activity

Owner provisioning supplies a dedicated fixed-size local NTFS volume.
A fixed VHDX additionally requires proof of its allocation and outer-volume
reserve. Runtime accounting includes allocated bytes, caller-available and
total free space, staging and metadata. Handle custody rejects reparse
components, unknown volume transitions and aliased file leaves. Activation
requires evidence of actual capacity enforcement, immutable artifacts,
safe layout and exclusive release.

The configured drive letter must name a real volume root. Path custody
resolves the drive root with `GetVolumeNameForVolumeMountPointW`, opens the
returned `\\?\Volume{GUID}\` root and requires the same volume serial and
file ID as the held drive-letter root handle. A `SUBST` drive or other
drive-letter alias for a folder fails this check. The runtime profile does
not yet carry a pinned volume identity. Before activation, the profile must
pin the owner-prepared volume's GUID path and serial, and every custody pin
must compare against that pin. A live identity that differs from it must
refuse with `PathCustodyUnverified`. This is a gate before activation, not
slice 3 behavior.

Path custody walks relative to held handles. The volume root is the only
object opened by absolute name. Each later component is opened with
`NtCreateFile` relative to the already-checked parent handle, using
`FILE_OPEN_REPARSE_POINT` and `OBJ_DONT_REPARSE`, and then its reparse,
sparse, compressed, type, link-count and volume attributes are checked. Every
pinned object is opened with data access and without delete sharing, so it
takes part in sharing checks. A writable leaf also shares write for its
consumer. The leaf reopen goes through the same parent handle. The tests
establish the following, on the developer host's NTFS:

- Renaming a pinned ancestor fails. Renaming or deleting a pinned leaf fails,
  for a newly created leaf and for an existing writable one. Adding a hard
  link to a pinned leaf fails. The link count is checked once at pin time,
  and the held pin then refuses a new alias.
- A pinned ancestor that still contains the next pinned component cannot be
  made a junction. This was tried through a write handle (refused by the
  share mode) and through an attribute-only handle (refused by NTFS's
  empty-directory rule for mount points). Share modes alone do not bar
  attribute-only opens.
- A pinned ancestor that is still empty can be made a junction through an
  attribute-only handle. This was observed for the parent of an exclusive
  create. The relative, no-follow walk then refused the next open, and
  nothing was created in the junction target.
- A relative open follows the held parent object after its name is moved
  and replaced by a junction.

What remains: the defense for an empty pinned ancestor is the relative
no-follow lookup, not an inability to convert it. Same-account code can
therefore make a walk refuse, which denies the operation but does not
redirect it. Behavior was observed on one host and NTFS version. Other
reparse tags (for example symbolic links, which need a privilege) were not
exercised. Owner qualification must confirm these on the dedicated volume.

QEMU and qemu-img still open their image, firmware, disk and data paths by
name. That residual sits inside the same-account trust boundary above:
another process under the supervisor SID could race those name-based opens
in a way the supervisor's pins do not fully prevent. Before activation, the
runtime must re-verify the volume, file ID and digest of each artifact the
child consumes, both before launch and after QEMU has opened them, and
refuse or quarantine on any mismatch. This residual is an activation gate.
It is not closed in slice 3.

WTS is the proposed owner activity source. The candidate samples local
sessions, binds the local SAM account to the pinned SID, verifies session
incarnation across queries and accepts only fresh, active, unlocked owner
activity records. Missing or ambiguous results close admission. The initial
policy uses ten minutes of idle time and a five-second freshness window.
Logical CPUs and physical memory supplement the confirmed Job controls.
Nested CPU scope, WHPX overhead, capacity and WTS access require owner-host
qualification.

## Evidence and activation

Default Windows tests use disposable NTFS paths and a child re-executed
from the test binary. They need no WHPX or QEMU. They cover creation-time
Job membership while suspended, the exact inherited-handle list, a forged
pipe-server PID refused before any exchange, Job handle loss and supervisor
loss. For supervisor loss, a disposable parent process owns the only Job
handle and the observer holds only a wait handle on the grandchild. The
same tests cover hard-link and rename refusal, junctions as an intermediate
component and as a leaf, the volume-root binding, and exact-owner and
ordered explicit-DACL matching with positive and negative cases. They also
race the custody walk, covering each case listed under storage above.
The wrong-PID fixture serves exactly the expected owner and DACL, so only the
server PID differs. Other tests cover:

- the resource revalidator refusing at resume and at `cont`;
- the dispatch gate, driven deterministically by an injected clock and a
  blocking post-write checkpoint: a stale sample never arms; an expired
  sample writes nothing; an acknowledgement observed before the worker
  records a late write is refused; and an in-time write is accepted. The
  boot-side settle future is polled directly to `Pending` before the worker
  is released;
- guest readiness on the paused tokio clock, with in-memory QMP and serial
  streams and a fake guest that answers every `Hello` in order:
  - firmware consumes a challenge, a close pauses retries, and the next open
    gets a fresh challenge;
  - a stale and a fresh `Hello` are both acked, the first valid echo is
    accepted, and the command channel skips the later ack;
  - a coalesced repeated open sends a fresh challenge;
  - with no events at all, the 1 s retry reaches the guest;
  - the 32-challenge cap refuses, and the budget bounds every retry;
  - a challenge from outside this boot, or an unissued, oversized or
    malformed ack, refuses;
  - no `Hello` is written at or after the deadline;
  - other ports are ignored, and malformed port events, including an
    out-of-range timestamp, refuse;
  - a partial reply survives losing the select to a port event, and a
    partial QMP record survives losing it to a retry, with no lost or
    duplicated bytes;
  - a late service inside the budget passes, and a silent one refuses at
    exactly 180 s;
  - a stalled frame refuses after 10 s;
  - cancellation stops the wait;
  - a reply that is ready together with the expired deadline refuses;
- the 112-byte 11.1.x create headers, with each refused feature,
  compression and extension case, and boot recipes with foreign, missing or
  extra backing references or an external data file;
- an existing writable leaf refusing rename, delete and new hard links;
- process creation failing after both attributes are set;
- each constructor refusing a wrong role, header or size.

An access denial counts as a restrictive outer runner Job only when it
comes from process creation and an identical creation without the Job-list
attribute succeeds. In that case a local run prints a `SKIP vm-job-fixture`
or `SKIP vm-supervisor-loss` line and returns. When `CI=true`, which GitHub
hosted runners set, the same classification fails the test instead. Hosted
evidence therefore shows a pass only if the supported Job path ran, with no
CI profile change. Any other denial fails the test everywhere.

These tests ran under the developer's interactive account. The full WHPX
recipe, the dedicated standard account, effective pipe ACLs against distinct SIDs,
physical disk ceilings, the volume-identity pin, QEMU's own name-based opens
and WTS owner activity are **unverified on real hardware**. The optional
Linux TCG fixture covers only QMP startup. It requires an explicit
digest-pinned manifest and was not exercised. Disk, synthetic qcow2 and
qemu-img backing behavior through the production recipe pieces belong to
the M4 PR 6 smoke. The production recipe types use Windows drive-letter
paths, and mutating qemu-img plans need the uninhabited offline lease, so
this Linux fixture cannot exercise them.
The release gate stays closed pending independent review, passing checks
and owner qualification of the exact package and native controls.

## Historical amendment — Weil 11.1.0 Windows x64 pin

The 2026-11-23 amendment below supersedes this pin.

The previous candidate was QEMU 11.1.3. That release is available as source.
The official Windows binary catalog provides the 11.1.0 build identified in
the slice 3 design. The candidate profile accepts only
`qemu-11.1.0-windows-x64`. The QMP greeting must report version 11.1.0.
[Windows binary catalog](https://qemu.weilnetz.de/w64/2026/) · [QEMU source archives](https://download.qemu.org/)

The portable installation receipt identifies Stefan Weil's 2026-08-11 x64
installer. The installer SHA-512 matched the publisher's checksum
`5bcf9eed634e8575a37b74f445af41a2fe4106da512d0c30c368301d4c105037fdfab40a5287367a28a957624cddebbc8c07e16c88ab6634f554cdf3d16bf543`.
Both executables reported `11.1.0 (v11.1.0-12130-ge470268ff4)`.
Windows reported Authenticode `UnknownError` because the signer certificate
expired on 2023-12-09, before the 2026-08-11 embedded timestamp. The owner
explicitly approved installation after disclosure of the validity error.
[Installer](https://qemu.weilnetz.de/w64/2026/qemu-w64-setup-20260811.exe) · [Publisher checksum](https://qemu.weilnetz.de/w64/2026/qemu-w64-setup-20260811.sha512)

| Payload file | SHA-256 from the installation receipt |
| --- | --- |
| `qemu-system-x86_64.exe` | `47d57a6072e0bb3bd98f87926eb129eb1736dfe818c67b3b81ef7ce4edd0b3cd` |
| `qemu-img.exe` | `9d930582d0d574bb106d89982c547688e10b8f356192d302cc3d0ae4e8674534` |
| `share/edk2-x86_64-code.fd` | `33090cc07675baa5190d9f1e84bf5176b33bcbfa9bacac522961150cdb6dbb2a` |
| `share/edk2-i386-vars.fd` | `5d2ac383371b408398accee7ec27c8c09ea5b74a0de0ceea6513388b15be5d1e` |
| `libslirp-0.dll` | `098093db72095516c877c371942444a508aaa3c546de563bd2ea36ba2e88a646` |

The paused, diskless and networkless WHPX smoke used `pc-q35-11.1` and QMP
over stdio. QMP greeted with 11.1.0, acknowledged `qmp_capabilities` and
`quit`, and the process exited with code 0. Stderr reported an ignored
interrupt-vector-0 request. The exact recipe, loaded dependencies and full
guest boot remain **unverified on real hardware**.

A diff of upstream `block/qcow2.h` at v11.1.0 and v11.1.3 was empty. The
version-3 header layout and 112-byte fixtures apply to the 11.1 stable series.
[v11.1.0 header](https://github.com/qemu/qemu/blob/v11.1.0/block/qcow2.h) · [v11.1.3 header](https://github.com/qemu/qemu/blob/v11.1.3/block/qcow2.h)

The upstream stable delta includes an AHCI/ATAPI hang fix relevant to
installer media, a virtio-net heap bounds fix (CVE-2026-66900), and a block
I/O race fix for data corruption. The delta covers upstream tags. Downstream
patch coverage in the Weil build remains unverified. Limit 11.1.0 to
trusted, disposable TEST-VM qualification. Require a patched build before
untrusted guests or persistent (static) VM disks, and rerun the real recipe
on any replacement. Native admission stays closed. M4 native activation
still requires the amendment in ADR 0058 that replaces the dedicated-account
identity gate with the `current_user` identity gate.
[Upstream stable comparison](https://gitlab.com/qemu-project/qemu/-/compare/v11.1.0...v11.1.3)

## Amendment 2026-11-23 — Reviewed local QEMU 11.1.3 Windows x64 build

The candidate profile accepts only `qemu-11.1.3-windows-x64`. A profile
naming `qemu-11.1.0-windows-x64` is rejected with `InvalidVmProfile`.
The QMP greeting must report 11.1.3 before capability negotiation.

The local distribution was built from the upstream 11.1.3 tarball with
SHA-256 `5a8289c7940dc77d3873c81a6c29ba3afe1446fc9d36e77fa1658cdbecb84b22`.
The detached signature verified against QEMU's release fingerprint
`CEACC9E15534EBABB82D3FA03353C9CEF108B584`. The build used the signed
MSYS2 x86_64 UCRT64 base dated 2026-11-20, GCC 16.2.0, Meson 1.12.1,
Ninja 1.13.2, and Python 3.14.10. The configuration selected
`x86_64-softmmu` with `--enable-whpx`, tools, libslirp 4.9.7, and zstd
1.5.7. One local Meson patch skips the developer-bundle symlink hook,
which requires symlink privileges on Windows. Its SHA-256 is
`35c1a14b1f9b6498ec43f5269dde137d9c0af288b97d10518ae6ca270ce6cb32`.
Independent comparison with the signed archive found only that change
to source files. [Source archive](https://download.qemu.org/qemu-11.1.3.tar.xz) · [Detached signature](https://download.qemu.org/qemu-11.1.3.tar.xz.sig)

The build and independent review confirmed three stable fixes in the
signed source:

| Fix | Stable commit | First release |
| --- | --- | --- |
| Virtio-net bounds fix for CVE-2026-66900 | `ec2fdc85829a5090ebaea2f4094b9616a4ea6309` | 11.1.2 |
| Block read-modify-write atomicity and data corruption | `7e6a8d069ff627ca8cfe6392b9ffdee2ee7ef2aa`, backport of `0bd5a1b3d60cca595e19bb73eec83307259be671` | 11.1.3 |
| ATAPI CD read hang during block drain | `1e4ab5af46a76f7aad63573686bc354a8bbe9a0d` | 11.1.1 |

Both executables and the shipped DLLs are Authenticode `NotSigned`. The
reviewed SHA-256 pins are the trust anchor for this build. The source
signature authenticates the upstream tarball. The installation receipt
binds an exhaustive manifest, resolving the initial review's HOLD on
partial receipt coverage. The independent re-check verified the exact
136-file payload, including the virtio option ROM, BIOS, and EDK2 data.

| Pin | Bytes | SHA-256 |
| --- | ---: | --- |
| `RECEIPT.txt` | | `e0e33d3fb433c467f5f530bcb8cc8ce41bb8c3b10133bd54718915b7357b852e` |
| `MANIFEST.sha256` | | `f1cf73c8ecae163e31467a3985fc13749aee1f12a8747756eb136108a1015759` |
| `qemu-system-x86_64.exe` | 28531009 | `0eff2d9977e8c8a6963d099ec3d2ebdae999b351c55d59c5ed3abcf97b0535f5` |
| `qemu-img.exe` | 2520350 | `0765417c097a7d9d7ddf5b0bf7cdb4ab033d11767d3839005111e367862a0f9f` |

This review lifts the 11.1.0 distribution restriction on untrusted
guests and persistent VM disks for the reviewed 11.1.3 payload. The
paused WHPX smoke started `pc-q35-11.1` with no disk or network, observed
QMP version 11.1.3 and `prelaunch`, and quit cleanly. Native VM execution
with the production recipe is **unverified on real hardware**. Native
admission still requires the activation custody and resource gates and
the ADR 0058 `current_user` identity gate.

## Amendment — Typed installer boot and passive poweroff

The sealed launcher admits three QMP-only purposes under the existing
operation-bound authority. Each requires a backing-less TEST installation
disk and private writable VARS. The Ubuntu plan attaches the official ISO
at q35 AHCI port 0 and CIDATA at port 1. It loads the pinned kernel and
initrd directly, with the fixed append string `autoinstall ds=nocloud ---`.
The Windows Setup plan attaches the official ISO at q35 AHCI port 0 and
the answer and driver media at ports 0 and 1 of the additional AHCI
controller at PCI address 0x1e. The Windows finalization plan boots only
the disk and VARS. Optical nodes are explicit read-only raw devices.
All three plans supply the existing named virtio-serial port through a
null backend. Installed boot retains the private serial pipe and existing
argv and readiness policy.

Windows Setup uses a distinct, content-pinned initial private VARS role.
Its qualification binds UEFI BootNext to the exact official ISO's
noninteractive boot entry and requires persistent disk-only boot selection
after the one-shot entry is consumed. Setup argv supplies neither a legacy
boot order nor boot indexes. The firmware and installer pin must prove the
initial CD handoff, its consumption, and disk boot through Setup resets.
Later boots reuse the resulting candidate VARS as the ordinary writable
role. The runtime matches the expected role, object identity and recipe
pin against retained custody. For initial Windows Setup, the launcher
reopens the same VARS under its held parent with deny-write sharing and
rechecks the content pin through that handle before suspended creation.
It retains the write exclusion until the handoff immediately before
resuming QEMU, whose writable pflash open requires it. The ordinary
object and ancestor pins stay held throughout. The remaining interval
before QEMU's by-name open falls under the activation custody gate. The
authority issuer must bind those inputs to the qualified recipe,
operation and generation. There is no issuer.
[QEMU boot selection](https://www.qemu.org/docs/master/system/bootindex.html) · [Windows file sharing](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-createfilew)

Installer startup shares the creation-time Job, QMP greeting,
capabilities, prelaunch status, and resource-gated dispatch of cont. Its
dispatch cutoff is clipped to the stage deadline. Completion waits on QMP
and rechecks resources every five seconds. Windows Setup permits at most
16 typed guest resets and expires after one hour without reset progress or
poweroff. Ubuntu installation and Windows finalization refuse resets.
All three require a validated SHUTDOWN from the guest with reason
guest-shutdown. A STOP allows two seconds for that event. Host shutdown,
panic, invalid or excessive events and early EOF fail the phase. Installed
stop keeps its own solicited powerdown path.
[QMP run-state events](https://www.qemu.org/docs/master/interop/qemu-qmp-ref.html#vm-run-state)

The lifecycle starts one monotonic recipe budget before its first launch.
The maximum is 60 minutes for Ubuntu and three hours for Windows. Stages
can only shorten the remaining deadline. Windows uses the same budget for
detached installed-mode health, disk-only finalization and later detached
verification. Reset and retry do not extend the absolute deadline.
Terminal observation is followed by a clean held-process exit, empty Job,
joined workers, exclusive reopen of disk and VARS, and the caller's bound
generation release check before media custody can end. The reopen and
read-only generation check run in a dedicated worker that owns custody
for as long as it runs. Only the async caller can accept its evidence
before the absolute release deadline. On expiry the caller closes the
result channel, requests cancellation of pending worker I/O, and returns
charged quarantine without waiting for that worker. The worker may finish
an in-flight read, and has no authority to publish, detach, or release the
generation. Teardown following a failed phase has an independent two-second
recovery budget. A proved release after a phase failure is dirty. A clean
installer release permits the next lifecycle step. Detached recipe health
still gates capture and seal.

The paused-clock and in-memory QMP fixtures cover the startup and poweroff
contract. Native filesystem fixtures cover exclusive reopen and stale
Windows Setup VARS. Paused-clock worker fixtures cover the deadline for a
stalled reopen or generation check and discard evidence after expiry.
Real Windows Setup, Subiquity, optical topology, one-shot firmware boot
order and guest reset behavior are **unverified on real hardware**.

## Installed static Ubuntu web variant

The sealed installed-static recipe permits one inbound IPv4 loopback forward.
Its netdev is exactly
`user,id=arxnet0,restrict=on,ipv6=off,hostfwd=tcp:127.0.0.1:<leased-port>-10.0.2.15:443`.
Only the supervisor can reserve a port from the fixed range. The bound plan
holds the claim and requires an unbacked static disk and private static VARS.
All other boot purposes retain the fixed restricted network.

The Windows launcher refuses this plan while the QEMU/libslirp pairing,
listener handoff and process and Job proof are unqualified. Activation
requires the held boot to own the exact loopback listener and a trusted TLS
probe to authenticate the static generation's origin. A port conflict or
unrelated listener cannot establish readiness. The port, private identity
and raw probe response never enter MCP. Native behavior is **unverified on
real hardware**. See [ADR 0063](0063-vm-store-v9-forward-only.md) for storage.

## Amendment 2026-11-23 — Public-only egress variant for the static VM (ADR 0064 E1b)

[ADR 0064](0064-v1-scope-docker-static-vm.md) adds a second sealed network
for an installed static boot, `StaticNetwork::PublicEgress`, beside the
restricted origin above. It names its read-only custodied policy artifact
and seals a distinct plan kind, `PublicEgressBoot`. Its netdev is exactly
`user,id=arxnet0,restrict=off,ipv4=on,ipv6=off,net=10.0.2.0/24,host=10.0.2.2,dns=10.0.2.3,arx-public-egress=<custodied-policy>,hostfwd=tcp:127.0.0.1:<leased-port>-10.0.2.15:443`,
followed by `-device virtio-net-pci,netdev=arxnet0`. The policy path is
passed as a QEMU keyval string, with any comma doubled. The port is the A2
supervisor lease from the fixed 49152–49215 range. The restricted origin,
TEST boots and installer purposes keep their settled argv byte for byte.

`arx-public-egress` exists only in the separately pinned patched
QEMU/libslirp build from E1a. The plan requires a distinct
`PublicEgressQemu` artifact role, so the stock `Qemu` image cannot run it
and the patched image cannot run any other recipe. Both the policy and the
image must name held objects: each carries a file identity and a bounded
content pin.

The candidate profile may carry an optional `public_egress` section. It
holds the manifest `qemu-11.1.3-arx-public-egress-windows-x64`, the pins
of the console executable and its receipt, and the enrolled policy pin.
The patched build links libslirp statically, so it has no DLL slot. The
section never replaces the stock `manifest`, which TEST VMs keep. A
configured pin is not a qualified pin. Qualification requires an exact
entry in the compiled reviewed-build list that pairs the receipt with the
executable bytes it vouches for. A reviewed receipt never qualifies any
other executable. The list stays empty until the E1a source, build and
native receipt are independently reviewed. That receipt must cover the
full payload and the loaded-dependency set. Any pin that names the
reviewed stock 11.1.3 executable or receipt is refused.

E1b follow-up adds the independently reviewed E1a (#292, re-review 94835d03) executable and receipt pair to the compiled list.

The policy artifact follows the E1a policy v1 contract. It is at most
32 KiB of printable ASCII with LF line endings and a final LF, and has no
spaces or blank lines. The fields appear in this order:

- `version=1`;
- `interface_index` and `interface_luid`, as nonzero canonical decimals;
- `source` and `gateway`, as canonical IPv4 addresses that are nonzero and
  distinct;
- zero to 512 distinct `deny=` CIDRs, with prefixes 1–32 and clear host bits;
- `fingerprint`, the lowercase SHA-256 of every preceding byte.

The compiled special-purpose table and the synthetic subnet always apply in
addition to the `deny=` lines. The table covers 0/8, 10/8, 100.64/10,
127/8, 169.254/16, 172.16/12, 192.0.0/24, the documentation, benchmarking
and 6to4 relay ranges, 192.168/16, multicast, reserved and broadcast.
Admission is stricter than the backend: the enrolled source and gateway
must already lie in the deny union. The in-file digest checks integrity
only. The supervisor authenticates the whole file by the enrolled pin.

Custody opens the policy read-only under an anchored directory and refuses
links, reparse points, extra hard links and unsafe leaf names. It checks
privacy through the held handles of both the directory and the file. On
POSIX, the effective UID must own each object and no group or other
permission bit may be set. On Windows, TokenUser must own each object, the
DACL must be protected, and the DACL must consist of exactly one explicit,
non-inheritable TokenUser FullControl entry, checked with the same
`vm_acl::exact` helper as the runtime inputs above. Custody reads the bytes
through the held handle and verifies the pin over exactly those bytes. At
every verification, including admission, it re-checks the anchor directory
and the leaf through their held handles: both names still denote the held
objects, and both handles still meet the privacy rule.
Windows holds the file without write or delete sharing and the directory
without delete sharing. The patched executable is held read-only the same
way and measured through its handle. As a bundle input it carries no
owner-only rule.

Admission refuses with a typed `EgressRefusal` when evidence is missing or
mismatched:

- the plan or its lease;
- the policy's presence, custody, grammar, completeness or fingerprint;
- a missing validated profile or public-egress enrollment (`Unenrolled`).
  Admission checks enrollment first. It reads the enrolled policy pin and
  the patched-binary pin from the validated `VmRuntimeProfile` itself,
  never from loose values. Only `VmRuntimeProfile::parse`, with its size
  limit and semantic validation, can produce that type. It implements no
  `Deserialize` and has a private field, and the foreground file's runtime
  section also goes through `parse`;
- a held policy that differs from the profile's enrolled pin;
- a held policy or executable that is not the exact object the plan's argv
  names. The normalized local path must equal the argv spelling exactly,
  case included, because NTFS directories can be case-sensitive. The file
  identity and content pin must also match;
- a route other than the enrolled default uplink (index, LUID, source and
  gateway);
- a patched binary that is missing, stock or unqualified, or whose held
  bytes are not the qualified executable.

Only a Windows drive path can name a plan input, so POSIX custody never
binds a plan. Every such refusal maps to the existing
`NetworkFenceUnverified`, so the MCP refusal vocabulary is unchanged. The
reference matcher mirrors the backend's checks for tests. It allows only
TCP and UDP. It drops the synthetic subnet, including the 10.0.2.2 and
10.0.2.3 aliases, before translation, and checks the deny union both before
translation and on the final socket destination. The single loopback
forward is host-initiated only. A guest-initiated flow to the forward peer
is refused.

Admission success is a portable check only. The Windows launcher refuses
`PublicEgressBoot` with `NetworkFenceUnverified`, as it refuses
`StaticWebBoot`, and `LaunchAuthority` stays uninhabited. The patched
build, Windows route and socket behavior, policy custody on the owner host
and real egress are **unverified on real hardware**.

## Amendment 2026-11-23 — Current-user native runtime on the enrolled PC

The owner's decision under [ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md)
applies to this enrolled PC only. Native VM execution uses the owner's
console logon in the foreground under [ADR 0029](0029-owner-selectable-windows-execution-account.md)
`current_user`. This supersedes the dedicated-account requirement, the
distinct owner and supervisor SID pins, and the distinct-SID/local-SAM WTS
match above for this mode. An explicit dedicated account remains possible with
its own identity boundary and qualification. Each execution mode requires its
own evidence and fails closed on an identity mismatch. There is no fallback
between modes.

### Execution identity and shared-SID trust

Pin one enrolled owner SID using canonical OS user-SID validation, including
the enrolled cloud identity. Bind it to the actual foreground primary
token's SID, logon SID, authentication LUID and session. Require an
interactive, non-elevated Medium-integrity token in the active console logon.
Accept a verified UAC-filtered token with Administrators marked deny-only.
Refuse elevation, enabled Administrators or Hyper-V Administrators authority,
SYSTEM, service, noninteractive and remote logons, impersonation, unknown
group attributes and any mismatch. Derive the restricted QEMU and qemu-img
tokens and verify the held suspended child's restrictions, group masks,
privileges, integrity, logon binding and default DACL before resume.

The owner accepts a shared Windows principal on this PC. Dedicated-account
separation from the owner's files and credentials is lost. Same-SID code can
interfere with local configuration, loaders, processes, pipes and storage.
The trusted account includes the foreground startup and local evidence.
Keep authenticated local transport, exact pipe ACL and live Job-child
endpoint checks, cross-principal and NETWORK denials, and an unnamed
non-inheritable kill-on-close Job with effective limits and no breakaway.
Keep private roots and immutable inputs pinned by held handles. Require
verified consumer opens and deny-replacement and no-redirection custody
through handoff. Close any effect whose custody the pinned QEMU or helper
cannot prove. These controls bind effects and limit child access and lifetime.
They do not protect against malicious code running under the same Windows
account (same SID) that interferes with the daemon, pipe or resources. Audit
the execution mode as `current_user` with no dedicated-account isolation
claim, and keep raw identities local.

### Console activity and capacity

Read WTS activity in the current session and bind it to the foreground token
and a stable console logon incarnation across queries. Verify the SID mapping
locally and require the unique active, unlocked owner console with sane
timestamps. A SID match alone cannot identify a logon.

Use WTS last-input time when the corroborating samples provide coherent
timestamps. If both samples report zero last-input time, `current_user` may use
`GetLastInputInfo` in the enrolled owner's process after corroborating that
process's physical-console session and the same logon. Prove ten continuous
minutes without observed input on a monotonic clock. Reset the observation
interval on input, stale evidence, lock, disconnect, session change or
uncertainty. Each WTS sample must be at or after its logon time in the
zero-input path. A change between corroborated input sources invalidates the
collector for its lifetime. Re-establish the collector before admission.
Mixed WTS input, clock wrap and backward steps close admission. The
diagnostic witness may observe the anchored owner's identity and WTS state
while switched away. Effects require the original owner console to be
active, unlocked and freshly verified idle.

Require ten minutes of inactivity and observations at most five seconds old
at dispatch, clipped to the phase deadline. Recheck activity and reserves
before resume,
at dispatch and while running. Recent input closes admission. Lock,
disconnect, session switch, ambiguity or stale observations also close
admission. Bring owned work to bounded quiescence when the owner returns or
activity or reserves cannot be established. Uncertain release stays charged.
The owner chooses one console-idle window for the unattended live test.
A static VM runs on this opportunistic host only when explicitly pinned to
it, subject to the same activity and reserve rules.

Enforce an aggregate **200 GiB VM disk cap on D:** with **at least 50 GiB
always free on D:**. Verify physical fixed-size NTFS capacity and, for fixed
VHDX backing, full physical allocation and both inner and outer free-space
bounds. Charge allocated bytes, outstanding and quarantined generations,
metadata and peak media, verification, snapshot, restore and copy staging
under serialized admission. Reserve **16 GiB RAM** and **at least half the
effective CPU cores** for the owner, accounting for nested Job scope and
QEMU/WHPX overhead inside and outside Job counters. Qualify initially with
one running VM at most, up to 8 GiB guest RAM and 4 vCPUs within those reserves.

### Qualification and admission

A complete, versioned native qualification record is valid for **at most
seven days**. Load it with a bounded, strict schema that rejects missing,
duplicate or unknown fields. Pin its reviewed digest locally and bind it to the
enrolled Windows host, store, profile, exact release, recipe and network
class. Requalify the affected scope before new work on material drift in the
OS, runtime or dependencies, input media, recipe, hardware, volume, network
or policy. After reboot, renew live startup evidence. Reject inconsistent
record age or wall-clock rollback. Record validity and the five-second live
observation limit apply independently. Owned teardown may finish after expiry.

Native admission stays closed by default and remains closed until the private
issuer in M4 PR 7c is in place and a fresh, matching native qualification
record exists. Each new effect also requires matching live proof bound to
its operation and generation. The reviewed qualification runner has private
trial authority for its disposable store, separate from daemon launch
authority. Qualify the restricted TEST/installer class, the static HTTPS
loopback origin and any Docker egress scope independently. Docker requires
its separate patched distribution and network evidence. Public publishing
defaults off. Agents have no authority to delete static VMs or backups.
[ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md) · [ADR 0064](0064-v1-scope-docker-static-vm.md)

The current-user activation gates and production native launches are
**unverified on real hardware**. Follow the
[native qualification runbook](../dev-host-setup.md#native-qualification-runbook).


## Amendment 2026-11-24 — Actual image and file-open witness for M4 qualification

A non-elevated, diskless and networkless WHPX investigation of the reviewed
QEMU 11.1.3 build reached QMP `prelaunch` and observed 16 manifest-matching
bundle DLLs and Windows system modules. Some system DLLs mapped after
`prelaunch`. The experiment also showed that read-share holds on existing
bundle files leave directory entry creation possible, and that app-local
copies of system DLLs could displace the expected System32 objects.
A QEMU `load_file` trace included a transient `kvmvapic.bin` read that the
recipe's argv does not name. These are investigation observations from one
host. The witness specified here is **unverified on real hardware**.

### Loader witness

Keep the existing `current_user` token, resource, Job and launch-plan gates.
Each launch allows exactly one process. The owning supervisor thread
creates the restricted child with `CREATE_SUSPENDED`, `DEBUG_PROCESS`
and creation-time Job, child-process policy and handle attributes, then
pumps events for that debug tree. It never uses `DEBUG_ONLY_THIS_PROCESS`.
Before resume, verify `PROCESS_CREATION_CHILD_PROCESS_RESTRICTED`,
creation-time membership in the unnamed kill-on-close Job, an effective
active-process limit of one and both Job breakaway flags clear. Require
the debug owner to successfully enable debugger kill-on-exit before
resume. Apply the same checks to QEMU and every helper launch. The
sealed build must exclude delegated launches through services or
brokers and alternate process-parent or token paths. Each helper the
supervisor starts gets its own held launch, witness and one-process Job.

Bind the held process and primary thread to the occupied slot,
operation and generation, Job and sealed plan's role table. The first
creation event must match those held identities and the executable
role for QEMU or qemu-img. Check event PIDs against live held objects,
with no PID-based reattachment. The Job and child-process restriction
prevent descendant creation. Debug inheritance is a second fence.
Treat any further `CREATE_PROCESS_DEBUG_EVENT` as a containment failure,
keep the new process stopped before its first user-mode instruction
and invoke the refusal settlement below. Identify it from the event
and account for it in teardown, even if Job membership is unexpected.
Job notifications provide corroboration. Their absence cannot prove
the restriction. Missing or ineffective containment closes launch.
[Microsoft process creation flags](https://learn.microsoft.com/en-us/windows/win32/procthread/process-creation-flags) ·
[Microsoft Job objects](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)

Use the file handle delivered by `CREATE_PROCESS_DEBUG_EVENT` and by each
`LOAD_DLL_DEBUG_EVENT` to verify the mapped object while the event is
pending. Close the event's file handle after inspection, following Windows'
ownership rules for process and thread handles. Continue a load event only
after checking a bounded, readable regular file by one of these rules:

- **Bundle image.** Its volume and file ID and canonical path match the
  held root-relative executable or DLL role in the reviewed manifest.
  Its exact size and SHA-256, measured through the event file handle,
  match the manifest. Hold the manifest, ancestor directories and leaf
  through release. A same-named or byte-identical copy at another
  identity fails.
- **System32 module.** Open and hold the native x64 system directory from
  `GetSystemDirectoryW`. Require a reviewed Windows dependency for the
  pinned OS build, with a verified final path directly in that directory.
  Resolve the expected leaf relative to the held directory with no
  reparse traversal and compare its canonical path, volume and file ID
  to the event file handle. Keep the reference handle through release.
  Prefix, basename, signature or version checks alone cannot establish
  that identity. Any other source is refused.

Use an exhaustive bundle inventory with no unlisted files. The effective
ACLs must deny file and subdirectory creation in the bundle and executable
directories and any enrolled search directory. Verify effective add-file
and add-subdirectory denial for both the foreground and restricted child
tokens, and keep directory and file custody intact through use. File share
modes guard existing objects. The directory policy closes the gap for new
names under the verified ACLs. The shared-account trust boundary above
still applies to the supervisor and local policy.

Use the absolute executable, an explicit empty, protected working
directory, a minimal Unicode environment without PATH search directories
and the exact handle allowlist. Require Windows safe DLL search mode and
creation-time System32 preference for OS dependencies. In the witnessed
build restrict dynamic loads to explicit trusted directories with
`SetDefaultDllDirectories` or `LoadLibraryExW` search flags. A setting made
by the child cannot govern its initial imports. Qualify startup and
delay-load behavior with the directory policy, creation-time mitigation
and actual mapped-object witness together. No user-selected search
directories, current-directory or PATH loads are admissible.
[Microsoft DLL search order](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order) ·
[Microsoft process creation attributes](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute)

Keep the debugger and its kill-on-exit policy active from creation to
observed process exit, including late loads and reloads during QMP,
guest execution and shutdown. Process each mapping separately even if
the same path was seen earlier. Bind the ordered load stream to the
same launch, use bounded counts, bytes and waits, and monitor it
independently of QMP and the open channel. Require the exact Windows
build and native tests to demonstrate complete initial and delayed
loader-event coverage with usable file handles. An unavailable debugger,
null or unreadable image handle, unexpected process, or incomplete
stream fails the image proof. A rejected mapping stays stopped and takes
the refusal settlement below.
[Microsoft debugging event handle rules](https://learn.microsoft.com/en-us/windows/win32/api/debugapi/nf-debugapi-waitfordebugevent)

### Coverage outside loader events

Ordinary executable images use the loader witness above. For the exact
Windows build and pinned executables, qualify KnownDLL resolution, the
actual System32 hosts for API-set imports, embedded activation manifests,
side-by-side resolution and alternate-directory search behavior at startup
and on delayed loads. Review manifest resources in all admitted images and
verify the protected search layout excludes external manifests, private
assemblies and redirection metadata. Refuse external activation inputs and
any resolution to a module outside the two image classes above, including a
side-by-side resolution outside them. An OS loader path with no usable
image handle or no verified identity leaves the affected stage closed.
[Microsoft DLL search order](https://learn.microsoft.com/en-us/windows/win32/dlls/dynamic-link-library-search-order)

Apply the following rules to the reviewed QEMU and qemu-img code and
reachable dependency paths:

- For every regular-file open, including firmware, ROM, machine data and
  paths found by `-trace load_file`, require the enrolled handle-adoption
  witness before the first use. This includes short reads and resources.
- A file-backed data section or view, including a resource or a
  memory-mapped file, must be constructed from the attested file handle.
  Authorize the section and each view under that role's allowed
  range, protection and lifetime before use. Allow non-executable views
  only. The witnessed path cannot reopen by name, or adopt an opaque
  section or view supplied by another process. Resource loading that
  opens or maps a file by name outside this path is excluded.
- Forbid executable sections and views created outside the ordinary
  loader, manual PE mapping and generated host code. The reviewed
  build removes or disables those paths. Require a verified
  creation-time dynamic-code prohibition without opt-out or downgrade
  and leave stages that need such code closed. WHPX guest RAM and
  anonymous data sections are allowed only for enrolled non-executable
  host data roles backed by private memory and within the slot budget.
- Named kernel objects and other non-file inputs have a closed stage
  list too. Allow the attributed stdio and witness endpoints, the
  verified local guest serial pipe for installed boot when present,
  and the exact Windows runtime and WHPX resources the reviewed stage
  needs. Enumerate those roles before launch for each sealed recipe.
  For each explicit object, record its role, namespace, required rights
  and expected creator or server. Bind a held handle to the expected
  object before use. For OS-internal bootstrap and platform
  objects that cannot be brokered, qualify the bounded set for the
  pinned host build, including how protected resolution and lack of
  caller control are established. Refuse unlisted named sections,
  pipes, devices, synchronization objects and other ambient inputs.
  A named object cannot stand in for a file role or supply executable
  code. Helper stages have no guest pipe or WHPX role.

The pin review must account for all reachable open, activation, section
and named-object paths in the binaries and their dependencies. Pair
source and binary review of imports and indirect entry points with
native coverage evidence independent of the enrolled hooks, including
controlled omissions and refused alternatives. This corroborates the
closed coverage inventory. It does not authorize consumption. The
trusted Windows loader, kernel and WHPX internals are inside the pinned
OS boundary. A build or stage with an unexplained or uncontrolled
input path remains closed. In-process observation and snapshots alone
cannot establish that paths outside the witness are unreachable.
[Microsoft resource-loading modes](https://learn.microsoft.com/en-us/windows/win32/api/libloaderapi/nf-libloaderapi-loadlibraryexw) ·
[Microsoft dynamic-code policy](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-process_mitigation_dynamic_code_policy)

### Consumer file opens

Use a reviewed in-process handle-adoption witness in both QEMU and
qemu-img. The launcher and the pinned binaries use a private, versioned,
bounded request/acknowledgement channel. Enroll only its exact child
endpoints alongside stdio in the creation-time inheritance allowlist.
Bind the channel to the held child, slot and Job at creation. Each request
and acknowledgement is bound to the launch nonce, a monotonic sequence
and the sealed plan's stage and role. Callers cannot supply channel
handles, paths, roles, access modes or protocol records.

Derive an open table from the sealed plan and reviewed distribution
contract before claiming the slot. Each row fixes the root and relative
leaf, artifact role, access and disposition, and either the existing
file identity and content or writable initial-state requirements, or
a fresh-output reservation. It also fixes the consumer call site,
stage, required or conditional status and allowed reopen count. The
supervisor opens each approved existing role relative to held parents,
without reparse traversal, and verifies identity and rights before
any write or truncation.

For a fresh qemu-img output, the pre-claim row identifies the held
parent and exact relative leaf, and starts with no file ID. After the
slot is durably charged and while borrowing its offline lease, use
`FILE_CREATE` relative to the held parent without reparse traversal.
Create a non-inheritable handle with read and write access and only
`FILE_SHARE_READ`. An occupied leaf refuses, even if it matches the
desired format.
Capture the volume and file ID from the create handle and bind the
row to that identity once. The create's sharing mode excludes
independent data writers and deletion from the instant of creation.

Keep that same open file object continuously held under the lease.
Before handoff, use `DuplicateHandle` to make a non-inheritable local
duplicate with only `FILE_READ_DATA`, `FILE_READ_ATTRIBUTES` and
`SYNCHRONIZE`. Duplicate the create handle with the minimum required
write-capable rights into only the held helper, as a non-inheritable
handle. Both duplicates reference the same open file object.
The supervisor uses its create handle only to bind identity and make
these duplicates. Confirm the rights and identity,
then close the supervisor's original write-capable handle before
acknowledging helper I/O. Any verification duplicate has no write
rights. The reviewed helper uses only its one grant for output I/O
and cannot reopen the output, export the grant or create a writable
file mapping. Stock Windows `-add-fd` cannot provide this handoff.

On success, the helper completes and flushes its writes and closes
its grant before exit. Require held helper exit, an empty Job and joined
workers before verification. Those checks and the closed grant inventory
prove no write-capable duplicate survives. The read-only duplicate
keeps the original open and its deny-write and deny-delete sharing
in force even after both write-capable handles close. Verify file
identity, header and permitted allocation through that retained
duplicate. Check the reserved name with a relative no-follow open
requesting only `FILE_READ_ATTRIBUTES`, with read and write sharing
for compatibility with the original open. Its identity must match
the create identity. The metadata open leaves the original sharing
guard in force. Keep the duplicate and parent pins until settlement
is proven.

If creation, identity or rights verification, duplication or closing
the supervisor's writer fails during handoff, withhold I/O authority,
stop any attributed helper and retain the custody already acquired.
A lost hold, surviving or unaccounted writer, name mismatch or
inconclusive verification refuses and keeps the affected generation
quarantined and charged.
Reacquiring a leaf by name cannot restore continuity.
[Microsoft native file creation and sharing](https://learn.microsoft.com/en-us/windows/win32/api/winternl/nf-winternl-ntcreatefile) ·
[Microsoft file objects and handle duplication](https://learn.microsoft.com/en-us/windows/win32/sysinfo/kernel-objects)

At each enrolled consumption site, the pinned binary adopts the granted
handle in the actual reader or writer and blocks before the first read,
mapping, write or truncation. While the handle is still live, the
supervisor duplicates it back from the held child and verifies its
identity and assigned role against the corresponding grant and held pin.
Only a matching, in-window acknowledgement releases that operation.
The reviewed hook must use that exact handle for I/O and deny an
unlisted request or a by-name fallback. There is no permit based on a
path string, a handle value alone, or a separately reopened path. Keep
the channel active through the last reopen or helper exit. Use the
writer-excluding reopen and allowed-mutation checks at settlement
for existing writable pins. A fresh create uses the retained-object
proof above, with no second data open for settlement.

Demonstrate coverage per exact binary and sealed recipe. Map each
reachable regular-file input and output site to an open-table row and
a native positive and omission case, including dependency and fallback
paths. The closed list below applies in addition to each stage's
enrolled images and non-file roles. Expand ROM and machine data to
an explicit list of manifest-pinned relative leaves per recipe. A
category label or the presence of a file in the bundle grants no
access. The table's conditional roles become required or forbidden
for the exact plan before dispatch. Every file outside those rows
is refused.

| Stage | Admissible file roles |
| --- | --- |
| Paused WHPX trial | Only the diskless, networkless recipe's ROM and machine data, and pflash CODE and private VARS if that recipe selects pflash. |
| Installed boot or detached verification | CODE, private VARS, ROM and machine data, the active disk and its one authorized golden backing if present. |
| Ubuntu install | The unbacked disk, CODE, private VARS, ROM and machine data, ISO, CIDATA, kernel and initrd. |
| Windows Setup | The unbacked disk, CODE, Setup VARS, ROM and machine data, and the ordered OS ISO, answer and driver media. |
| Windows Finalize | The unbacked disk, Finalize VARS, CODE, ROM and machine data. |
| qemu-img golden or offline info | Only the exact image under inspection and, if the sealed command traverses backing, its one authorized golden. |
| qemu-img create | Only the exclusively reserved fresh output and, for backed create, its one authorized golden. |

Prove short-lived helper opens even when every consumer handle closes
before helper exit.

Pin `kvmvapic.bin` to the reviewed root-relative distribution role for
any recipe that can load it. Require the observed open where the
machine configuration calls for it. A recipe that excludes it must
account for that exclusion in the source review and the exact native
recipe trace, and refuse an unexpected open. Apply the same rule to
other implicit ROMs and data files. A trace can discover names and call
order, including successful short-lived reads. Only the handle-adoption
protocol proves which objects supplied those bytes. Retain a sanitized
trace-to-role coverage matrix and the witness verdict for each supported
stage. Unexplained successful opens and unexplained omissions leave that
stage closed.

### Checkpoints, refusals and evidence

Process Snapshotting can corroborate the set of handles or mappings
visible at a checkpoint if the observed entries can be tied to the held
child and verified objects. It cannot prove opens that already closed,
mappings that were unloaded, later loads, or which handle served an I/O
operation. A QMP version, a manifest rehash, a pathname trace and passing
prelaunch each supply separate evidence. None substitutes for a complete
loader and user-file witness.
[Microsoft Process Snapshotting](https://learn.microsoft.com/en-us/previous-versions/windows/desktop/proc_snap/overview-of-process-snapshotting)

After QMP prelaunch, consume an explicit startup-open completion record
from the bound child and verify that all required pre-dispatch rows have
been observed. Where a plan would otherwise open a required input lazily,
adopt it before dispatch or leave that plan closed. Keep both witnesses
armed across the actual dispatch fence and through exit. Later mapped
images and allowed post-dispatch opens remain checked before their use.
Qualification success requires the terminal barrier and orderly
settlement below. Owner activity, cancellation, reserve sampling and
the stage deadline continue to govern all waits and dispatches.

An unsupported distribution or missing, mismatched or inconclusive
image proof returns `DistributionUnverified`. A missing, late,
ambiguous, substituted or wrong-role consumer-open observation returns
`PathCustodyUnverified`. Use `JobUnverified` for a failed process
containment check. There is no fallback to elevated ETW,
`SeDebugPrivilege`, snapshots or the stock build.

### Debugger stop, release and sealing

The supervisor holds its own process, Job, worker and custody handles
independently of those Windows supplies with debug events. Stop and
release proceed in this order.

1. Close admission and guest dispatch for the slot. On an ordinary
   stop, keep both witnesses active for the bounded graceful QMP stop.
   On refusal or witness loss, stop granting or acknowledging new I/O,
   mark the launch failed and keep the generation quarantined and
   charged pending independent settlement. Skip graceful QMP for a
   rejected event or witness failure. Keep a rejected or unexpected
   debuggee stopped at its pending event.
2. On refusal or expired graceful stop, request termination of the
   attributed Job while retaining its handle. If a debug event has
   attributed an unexpected process outside confirmed Job membership,
   retain a wait handle for it and request termination through the
   attributed process handle if it has the required rights. Use
   debugger kill-on-exit when direct termination is unavailable. The
   pump may drain events needed for kernel teardown while
   retaining the stop on user execution. A rejected nonterminal event
   can be acknowledged for draining only if separate, verified thread
   suspension on the stopped process guarantees that none of its
   threads can resume user code. A termination request alone is
   insufficient. If this guarantee or reliable draining is unavailable,
   retire the debug owner with kill-on-exit still enabled and proceed
   with independent termination and quarantine. Never detach a live
   debuggee, including on timeout, or restart observation by PID.
3. If the pump remains available, drain and continue each attributable
   `EXIT_PROCESS_DEBUG_EVENT` so Windows can finish kernel teardown.
   Continue terminal events before waiting for the independently held
   process handles to signal. Process and thread handles owned by
   Windows follow debug-event lifetime rules. The debugger session
   ends with the last debuggee's exit. There is no live detach step.
   If the debugger crashes, its kill-on-exit policy and the
   independently owned Job stop are the fail-closed path. A lost
   supervisor leaves a durable in-flight charge. Recovery treats any
   generation without proven settlement as quarantined.
4. With custody still held, require all attributed held process
   handles signaled, the owned Job empty and the debugger, I/O and
   witness workers joined. Then prove file custody and exclusive
   release. Use the writer-excluding reopen for existing writable
   pins and the continuous create hold for fresh outputs. Only
   after this independent settlement proof release the process, Job
   and custody handles and settle the slot. A timely, independently
   proven containment release may settle a failed trial as dirty
   under the existing release rule. Inconclusive or late settlement
   retains the charge and quarantine for reconciliation.
5. Record the scoped verdict and release outcome. A success seal
   requires complete accepted image and consumer evidence through the
   terminal barrier, with no unresolved events or acknowledgements,
   and clean settlement. A failed or incomplete witness can produce
   a failure record and an independently proven dirty release. It
   cannot produce successful qualification.

Record the contract version, recipe and distribution digests, scoped
role and image verdicts, completion checkpoints and release result in
the bounded evidence. Raw paths, machine or file identifiers, handles
and peer text stay out of published evidence.
[Microsoft debugging events and process exit](https://learn.microsoft.com/en-us/windows/win32/debug/debugging-events) ·
[Microsoft debugger kill-on-exit policy](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-debugsetprocesskillonexit)

### Distribution and implementation split

1. **Witnessed distribution PR.** Keep the current stock 11.1.3 pin
   and `stock_open_contract` refusal. The stock build supplies no
   complete proof for transient opens, so first review a distinct
   witnessed 11.1.3 Windows x64 distribution based on the signed source
   and pinned toolchain above. Review the minimal source changes for
   constrained loading and exact open coverage in both executables,
   including the channel protocol. Issue a new distribution ID, source
   archive/signature and patch digests, reproducible build inputs and
   configuration, executable and complete payload size/SHA-256 pins,
   and a reviewed receipt binding those artifacts to the protocol and
   open-table version. Pin the new distribution explicitly. QMP still
   reports the upstream version. The Docker public-egress distribution
   and qualification remain separate.
2. **7b3 native qualification adapter, after #304 and the distribution
   review.** Preserve #304's stock refusal before effectful probing.
   The witnessed counterpart of `stock_open_contract` returns a
   private, held contract from the reviewed pin, sealed recipe and
   live root-relative role bindings. It cannot certify observations
   that have yet to occur. Wire the restricted suspended launch, Job,
   stdio and witness workers, live WTS/WHPX and volume observations,
   and durable charged slot. Implement `verify_opens` on the same
   `Host::Opens` and `Host::Slot` after the paused exchange and before
   any guest dispatch. It checks the startup barrier and returns
   success only while the two live witnesses still own the launch.
   Keep them active through stop and settlement. Extend from the
   paused trial to each helper and recipe only with its own proven
   rows. The private trial authority stays separate from 7c's daemon
   issuer and production admission remains closed until its existing
   gates pass.

### Verification for the implementation

Use deterministic fixtures for manifest and System32 identity, wrong
initial image, unlisted or byte-identical wrong-file images, case and
reparse ambiguity, delayed load and unload/reload, null handle,
missing/wrong/duplicate/late/replayed open proofs, cross-slot or
cross-Job events, transient reads and short-lived helper exits. Drive
all required rows and mutations for omitted or bypassed hooks. Include
an unexpected process event, unlisted section or named object,
unbound and occupied fresh output rows, and output substitution at
settlement. Exercise event and message bounds, completion racing
cancellation or owner activity, witness loss before and after
prelaunch, refusal while frozen at initial and late image events, and
terminal event acknowledgement before process wait. Cover dirty release
without a success seal and durable quarantine on uncertain teardown.
Fixtures cannot mint a native qualification record.

Native-only acceptance uses reviewed binaries on disposable enrolled
roots under the non-elevated token. Prove creation-time Job and debug
event delivery, creation denial on the protected directories, refusal
for an unlisted benign fixture mapping, approved startup and late
System32 mappings, exact sealed-recipe traces and handle proofs for
every enabled row, including data and resource views, transient
firmware and short-lived qemu-img operations. Exercise a benign
descendant refusal for each launcher and the debugger's pre-execution
stop with an isolated fixture. Cover protected activation and named
objects, an occupied and a freshly created output, missing or
substituted input proof, and refusal during initial and late loads.
For fresh outputs, prove a separate writer is refused immediately
after creation, during helper I/O and through settlement after both
write-capable handles close. Verify the read-only duplicate cannot
write and still verifies the original object. Exercise a failed handoff
and loss of the continuous hold. Demonstrate terminal-event drain,
debugger and supervisor loss, clean or dirty release and durable
quarantine.
Run the full enabled recipe and helper matrix, including real WHPX cold
boots and observed coverage through teardown. These witness,
adapter and full-recipe cases are **unverified on real hardware**.


## Amendment 2026-11-24 — Owner fallback to held bundle inventory

The owner selected option 2 for v1 once the full witness could no longer
be delivered in a host-only adapter. The owner's rationale was that
"the witness only matters while host execution (current_user) is itself
fenced; an agent with host exec can do much more without touching QEMU."
Its marginal value does not justify stalling M4. This amendment replaces
the earlier loader and consumer witness requirements for v1 startup and
qualification. The full witness is tracked in [Known issues / later
release](../ROADMAP.md#known-issues--later-release).

The 7b3 builder found that #314 acknowledges the granted ROM handle
before the pinned child binds it into the CRT descriptor via the fallible
`_open_osfhandle`. The builder also found no reviewed exact-Windows-build
System32, KnownDLL, API-set, activation or native runtime-object coverage
inventory. Evidence `D:/Temp/arx-m4-7b3/report.md`, SHA-256
`44eba6f9a84ad995365a13a1a09f19d9f35a5246565b95f913fe3e526ac9aa98`.
Independent verification confirmed the coverage and native-evidence gap
as a hard blocker. It confirmed the handshake order and noted that the
supervisor can nevertheless verify the same live Win32 handle before ACK.
Evidence `D:/Temp/arx-m4-7b3-verify/verdict.md`, SHA-256
`2e3c83bfad25bba235b9a59b50f51f8b084466ee6a3c04270036418ddb5a0c27`.
[Witnessed distribution review](https://github.com/dytschgo/arxcovo/pull/314)

The paused trial uses the reviewed stock Windows x64 QEMU 11.1.3 pin from
#284 and its 136-file receipt and manifest. Stock QEMU can run the sealed
recipe directly. The #314 child requires its witness protocol and accepts
only the reviewed paused argv and helper rows. The stock pin also applies
to the M4 production QEMU and helper plans. Their pre-launch requirement
is the same held-bundle inventory with a sealed per-plan executable, data
and writable inputs. The separately reviewed [Docker public-egress
pin](0064-v1-scope-docker-static-vm.md) retains its own artifact,
qualification and network gates. This slice wires only the paused trial.
[Landed stock pin](https://github.com/dytschgo/arxcovo/pull/284)

The inventory authenticates the stock receipt and manifest, walks every
manifest entry relative to held, non-reparse NTFS directories and checks
each default data stream's length and SHA-256. The authenticated SHA-256
commits the bytes and therefore their length even for manifest rows
whose only per-file pin is a digest. The inventory matches exact case
and the complete tree of files and directories, rejects extra and missing
names, and bounds names, counts, bytes and the time allowed between local
filesystem operations. Listed objects stay open without write or delete
sharing from verification through stop and settlement. Windows sharing
does not block a new sibling from being created, so the executable,
bundle and empty working directories must also pass the protected directory
ACL check. The absolute recipe, minimal environment and initial image
search policy remain enforced. The paused adapter verifies the held
inventory immediately before create and again after the paused QMP
exchange. See [Windows file sharing](https://learn.microsoft.com/en-us/windows/desktop/FileIO/creating-and-opening-files)
and [creation-time image policy](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-updateprocthreadattribute).

The inventory attests that the complete reviewed bundle was present and
held under that namespace policy. Actual mapped modules, firmware and
other consumer opens, alternate OS activation or data mappings and
kernel/named-object coverage are outside its evidence. Same-SID host
execution remains inside the trusted boundary. If the enrolled user owns
a checked directory, that user can exercise implicit WRITE_DAC to grant
write access and change the directory between observations. The inventory
guarantee assumes a trusted owner, administrator and SYSTEM. WHPX is checked via
fresh platform capability samples and through the paused child's typed
`query-accelerators` reply.

For 7c, a complete native record binds the exact release, contract,
host/build and execution profile, volume and store identities, stock
receipt and manifest digests and lengths, and the sealed recipe, firmware,
media, guest-protocol and network-policy inputs. Require the held-bundle
inventory and protected search scope for every admitted plan, the
restricted token and effective Job, live WTS/WHPX and capacity/reserve
results, per-class network and guest/recipe acceptance, independent
release proof, and a clean end-to-end settlement in the validity window.
The issuer must validate the complete native record and local digest
selection, reject fixture or incomplete diagnostics, and recheck live
inputs and inventory before its own dispatch. This replaces the
full-witness prerequisite assumed by the initial 7c investigation
(`D:/Temp/arx-m4-7c/report.md`, SHA-256
`0d714a1731c9a9fd79712f854352c5986ea0c7232f5d9c66ee40d80d5730289f`).
The paused runner keeps its authority separate from 7c and cannot
produce the complete record. Native launch, live host observations and
settlement are **unverified on real hardware**.

### 7b implementation notes (PR 1 host qualification groundwork)

The qualification record decoder and manifest checks are shared with the
owner-local writer. The writer uses the held evidence directory for each
create-new row, reopens the allowlisted rows for verification, and checks
freshness again after flushing the temporary marker and before the final
rename. Complete publication stays unwired in PR 1 (deferred to PR 2); both
`--paused-trial` and `--full-test` remain diagnostic-only and publish
`incomplete.json`, while daemon `LaunchAuthority` and `OfflineLease` stay
uninhabited below the extracted private checked execution core.

The native Finalize primitive yields the held disk and VARS objects after
clean release. The offline reader enforces cooperative end-to-end time, I/O,
step and heap budgets, validates primary and backup GPT headers and entries,
verifies qcow2 refcount allocation (`refcount == 1`), checks `$Volume` clean
state and `$MFT` `$BITMAP` allocation, and reconciles path traversal against
authoritative `$I30` directory indexes while refusing reparse points,
attribute lists and extension records. It reads only the SOFTWARE hive image
state and Setup `State.ini` without host mounting or new crate dependencies.
Finalize's `windows_offline_setup_state` remains fail-closed
(`InstallerPhaseUnavailable`) in PR 1 pending PR 2 and verification against a
sanitized Windows-produced generalized image. The installed path remains
**unverified on real hardware**.

The host launch purpose is one private enum deriving argv, channel name,
bounded prelaunch quiet check, parser, stop protocol and a single per-purpose
Job/overhead envelope (2 GiB Job for diskless rows; 6 GiB Job for live
network). Controlled WTS-ambiguity and CPU, RAM and disk reserve faults
traverse the live `LivePoll` refusal-to-stop path and record original facts
from the same checked two-sample WTS/LSA observation used for the mutation.
The console witness loop captures a pre-prompt input cursor, binds the
enrolled witness account, validates returned session IDs and stable owner
logon identity, corroborates transitions across samples, and requires a fresh
idle observation before the next effect. Per-invocation release dispositions
are recorded in a run-wide ledger that latches `ChargedQuarantine`. Deliberate
live pressure and console witness runs remain **unverified on real hardware**.

The TEST network probe binds the Ubuntu 24.04.3 live kernel and initrd to
the exact held ISO and checks the sealed plan for forwarding, sharing, TAP,
IPv6 and writable media. The qualifier self-tests run-local TCP/UDP
witnesses on the enrolled host IPv4 and IPv6 addresses and the loopback
slirp alias. The DNS witness binds the exact address of the sole configured
IPv4 resolver. It refuses an unowned or occupied address, a fallback
resolver and failure to settle the listeners. Any accept on a random TCP
witness port during the observation window fails isolation. UDP and DNS
fail isolation on a nonce-matching receipt. No packet capture, firewall
mutation, external service or raw payload is part of the observation.

The guest verifies the active `virtio_net` NIC, the `10.0.2.15` lease and
the default route via `10.0.2.2`, installing it if restricted DHCP omits it.
A reachable gateway neighbor confirms the on-link control. The
nonce-bound `ARXNET3` report binds socket attempts to this setup. An IPv4
local failure blocks qualification, as does any IPv6 result inconsistent
with the observed absence of a global IPv6 address and route. Host-side
silence contributes isolation evidence only after all validated attempts,
the positive control, and verified settlement. The adapter rechecks media
bindings at create and after prelaunch and keeps QMP guest powerdown
separate from control-plane quit.
Live TEST network behavior is **unverified on real hardware**.

Windows Finalize requires both offline Setup observations through the same
writer-excluded released pair. The bounded reader has synthetic
qcow2/GPT/NTFS/hive fixtures. A sanitized Windows-produced pair is pending.
The owner-host full qualification supplies real-image validation. An
unreadable pair, release mismatch or missing Setup observation prevents a
complete dossier. Real Windows media and offline layout support are
**unverified on real hardware**.

The Windows Setup stage consumes a private copy of the exact enrolled
`windows_setup_vars` artifact. A writer-excluded source pin binds the local
path, size and SHA-256 before Setup and after verified release. The mutable
copy carries the one-shot BootNext entry. The reviewed owner-host seed is
pending, and absent or invalid input yields an incomplete dossier. The
BootNext handoff is **unverified on real hardware**.

The fixed TEST provisioners configure the guest-only IPv4 default route
through `10.0.2.2` for the installed VirtIO NIC. The installed endpoint
checks the route before running the witness probes. The launcher continues
to enforce slirp `restrict=on` throughout. [ADR 0055 records both pin changes](0055-m4-offline-template-recipes.md#test-guest-default-route-amendment).

### 7b PR 3 amendment — foreground selection and boot-bound records

Contract 4 binds the boot marker, the qualifier stage clock's origin in
Windows uptime milliseconds, and the issuance time on that stage clock.
The reader checks the marker independently and adds the recorded origin to
relative issuance and completion times. The UTC validity window and the
elapsed UTC versus monotonic check remain separate gates. Contract 3
dossiers must be requalified.

Windows supplies the boot sequence in the current process's
ProcessTelemetryIdInformation header. The OS loader increments the
sequence at boot. Changing the wall clock cannot set the loader's
sequence. A domain-separated digest of the sequence is recorded. The
reader fails closed on a failed or malformed query. GetTickCount64 supplies
the common uptime basis. Both binaries independently read these sources.
[Windows process telemetry header](https://learn.microsoft.com/en-us/windows/win32/devnotes/process_telemetry_id_information_type)
[Loader boot sequence](https://learn.microsoft.com/en-us/windows-hardware/drivers/ddi/ntddk/ns-ntddk-kuser_shared_data)
[Windows uptime](https://learn.microsoft.com/en-us/windows/win32/api/sysinfoapi/nf-sysinfoapi-gettickcount64)

Foreground selection borrows the production store guard, pins both
reviewed release images, and accepts the pinned daemon as the executing
image. It measures the same projection as the qualifier from held native
inputs before reading the selected record. It opens existing seal rows
relative to the read-only dossier directory, with write and delete sharing
denied. The qualifier continues to claim only its own new output directory.

Foreground execution consumes a 7c permit at the checked create edge,
yielding the uninhabited `LaunchAuthority` and matching the pinned seal,
plan kind/phase, and exact production workspace, lease file and generation.
The live reserve check includes 1 GiB of WHPX kernel overhead on top of the
Job memory limit, matching `recipe_live`. The checked Job and boot protocol
retain the verified guest channel until teardown. Settlement carries exact
generation slots and holds both writer-excluded handles after child exit, an
empty Job and joined stdio workers. Uncertain settlement keeps the generation
charged. The qualifier's separate disposable path still rejects daemon offline
leases. Production dispatch, WHPX guest boot and teardown remain
**unverified on real hardware**.

### 7b PR 4 amendment — production recipe inputs and helper release

The foreground binds the two recipe identities to the native record and
keeps the measured inputs private. It prepares the Ubuntu seed from the
held guest, installer and stock assets with the reserved generation as
the instance identity. Windows preparation retains the checked answer
ISO and installed-image probe from the same measurement. The initial
VARS copy comes from the held stock firmware for Ubuntu and from the
enrolled one-shot Setup VARS for Windows.

The daemon supplies the exact reserved disk, VARS and seed slots. Each
new leaf requires a separate authorization at its checked create edge,
with the workspace lease, seal, recipe, phase, content pin and deadline
bound to the effect. The adapter creates only new leaves relative to
the held directory. If staging fails partway, the daemon keeps the
affected reservation charged until it can reconcile custody.

The foreground derives the sealed boot plan from those inputs and the
ledger-pinned disk, VARS and optional golden. The launch edge compares
the supplied plan with that derivation, including file identities and
digests that do not appear in argv. Stock custody independently checks
the paths and held objects. Initial VARS bytes are rechecked under writer
exclusion through suspended creation. The fixed recipe uses 4 GiB guest
RAM and two vCPUs under the qualified 6 GiB, 2500-rate Job. The
production runtime still needs the 7c issuer before it can dispatch.

Helper settlement carries the exact plan kind and role/identity
projection along with the live workspace lease and writer exclusions.
The ledger consumer keeps the proof through inspection and settlement.
Missing or mismatched exclusions yield charged quarantine and withhold
helper output. The qualifier still uses its separate authority and
disposable workspace. Production dispatch and settlement are
**unverified on real hardware**.

## Amendment for the Docker qualification dossier

Docker qualification uses contract 4 with the existing 19-stage Docker sequence,
native provenance and the `arx-vm-docker-v1` dependency projection. It measures
both release images by role and binds the held runtime profile, distinct store
roles, reviewed stock inventory, guest and installer inputs, firmware, fixture,
policy, topology, origin trust profile and exact Compose recipe. The network row
commits a run-scoped digest of the private receiver and control evidence.

For the E1a patched distribution, `loaded_artifacts` commits the reviewed receipt,
complete exact-case manifest and explicit executable, image-tool and firmware
roles. The collector must retain the complete reparse-free, writer-excluded
payload inventory through checked execution and settlement. The pinned E1a
receipt records recursive import review, static libslirp linkage and the module
origins observed during paused smoke runs. The held inventory satisfies the
Docker loaded-dependency requirement for v1 under the restricted search
namespace from this ADR. Its digest describes that reviewed inventory and
receipt. Patched guest execution and release still require their own stage
evidence.

The Windows adapter measures the recipe before opening a selected dossier.
An independently enrolled receipt pins the reviewed Ubuntu web pool, Docker
closure and Ubuntu keyring. The dossier binds the exact review receipt. The
review must verify Ubuntu and Docker signatures,
the signing identities, signed indices and archive sizes and digests, the exact
base and package versions, and the inventory of the read-only ARXCOMPOSE image.
The adapter checks the receipt and every corresponding held input against its
pin. Treat any missing review or changed input as incomplete evidence. The
adapter does not rerun an OpenPGP verifier during dossier selection.

The writer checks the Docker chain before output, reopens every exact row
through the held directory, and resamples the independent boot, uptime,
dependencies and disposable settlement before the create-new marker rename.
The patched runtime effects, receiver measurements and native settlement are
**unverified on real hardware**. Static activation and publication retain
their existing gates.

The collector uses a separate disposable workspace claim. It flushes a maximum
disk charge and each create intent before staging. An uncertain release keeps
the claim charged and prevents workspace reuse. The settlement path checks the
held workspace and verifies the Docker dossier before publishing the marker.
Detached and endpoint copies select their fixed guest bootstrap through
fw_cfg. The detached copy leaves services masked. The endpoint copy provisions
the engine using the exact fixed units and accepts only the pinned Compose v3
hello on that session. The Windows Docker CLI refuses before claiming output
until the native receiver and route adapter is wired. Hosted tests substitute
those peers and exercise the same collector and marker-last writer.
