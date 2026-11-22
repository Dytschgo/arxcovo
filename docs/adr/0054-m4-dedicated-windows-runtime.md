# ADR 0054 — Dedicated Windows VM runtime

The VM runtime is a separate foreground candidate for a dedicated local
standard account. Its profile has distinct owner and supervisor SID pins,
a bundle outside the managed volume and a state root inside it. The installed
service and production foreground schemas keep their existing execution and
transport boundaries. The VM controller commits portable intent. Native
launch and media operations require independent, operation-bound authority.
There is no authority issuer in this slice.

## Distribution and launch recipe

The candidate pin is upstream QEMU 11.1.3, built for Windows x64 with
MinGW-w64/UCRT. Qualification requires a reviewed signed source and
toolchain receipt, exact SHA-256 and size for the console QEMU and matching
qemu-img executables, the libslirp and loaded DLL set, ROM/data, and the EDK2
CODE and VARS images. No binary receipt is available. Version matching in the
QMP greeting checks the protocol peer's declaration during startup.
Distribution qualification independently checks the bytes on disk.

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

Header parsing follows the QEMU v11.1.3 `block/qcow2.h` layout. A version-3
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
The merged v1 guest protocol is host-initiated: the guest service reads
exactly one `Hello` first and treats a second one as a protocol error. So
the host sends one fresh 128-bit challenge and tolerates silence until the
budget expires. Until the guest opens its virtio-serial port, QEMU leaves
that frame unread in the host pipe. A service that starts late therefore
still receives this boot's challenge, and only an ack that echoes it, with
script support advertised, is accepted. Stale, replayed, malformed or
oversized replies refuse. Once a reply's first byte arrives, the rest of
that frame must arrive within 10 s. Cancellation stops the wait at once.
There is no retry, no second `Hello` and no reconnect. These properties come
from the protocol and QEMU's multiport virtio-serial behavior; they are
**unverified on real hardware** against the pinned binary.

Installer boots are refused. A plan with installer media is
`PlanKind::InstallerBoot`, and the native launcher's first check, before any
handle, Job or process exists, returns `InstallerPhaseUnavailable`. No guest
service runs during installation, so the installer phase needs a QMP-only
power-off observation and the two-medium (ISO plus CIDATA) recipe. Both are
deferred to the explicit runtime-fence follow-up proposed in the slice 4
design (§1 last row, §4 step 2, §7). The installer phase stays an
activation gate.

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
- guest readiness on the paused tokio clock: a late service inside the
  budget passes; a silent or too-late service refuses exactly at 180 s;
  stale, replayed, oversized and malformed acks refuse; a stalled frame
  refuses after 10 s; and cancellation stops the wait without delay;
- an installer recipe refused with `InstallerPhaseUnavailable` at the
  launcher's first check;
- the 112-byte v11.1.3 create headers, with each refused feature,
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

These tests ran under the developer's interactive account. Native WHPX, the
dedicated standard account, effective pipe ACLs against distinct SIDs,
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
