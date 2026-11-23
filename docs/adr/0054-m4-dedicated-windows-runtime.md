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

## Amendment 2026-11-23 — Current-user native runtime on the enrolled PC

The owner's decision under [ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md)
applies to this enrolled PC only. Native VM execution uses the owner's
console logon in the foreground under [ADR 0029](0029-owner-selectable-windows-execution-account.md)
`current_user`. This supersedes the dedicated-account requirement, the
distinct owner and supervisor SID pins, and the distinct-SID/local-SAM WTS
match above for this mode. An explicit dedicated account remains possible with
its own identity boundary and qualification. Each execution mode requires its
own evidence and fails closed on an identity mismatch.

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
Audit the execution mode as `current_user` and keep raw identities local.

### Console activity and capacity

Read WTS activity in the current session and bind it to the foreground token
and a stable console logon incarnation across queries. Verify the SID mapping
locally and require the unique active, unlocked owner console with sane input
and logon timestamps. A SID match alone cannot identify a logon. Require ten
minutes of inactivity and observations at most five seconds old at dispatch,
clipped to the phase deadline. Recheck activity and reserves before resume,
at dispatch and while running. Recent input closes admission. Lock,
disconnect, session switch, ambiguity or stale observations also close
admission. Bring owned work to bounded quiescence when the owner returns or
activity or reserves cannot be established. Uncertain release stays charged.
The owner chooses one console-idle window for the unattended live test.
Explicitly pin any static VM to the opportunistic host, subject to the same
activity and reserve rules.

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
