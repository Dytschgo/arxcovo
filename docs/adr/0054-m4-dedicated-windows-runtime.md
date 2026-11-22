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
a fresh resource check. The guest protocol subsequently requires a fresh
challenge echo and advertised script support. Graceful stop requires a
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
ordered explicit-DACL matching with positive and negative cases. A
restrictive outer runner Job is reported as a typed unsupported case. These
tests ran under the developer's interactive account. Native WHPX, the
dedicated standard account, effective pipe ACLs against distinct SIDs,
physical disk ceilings and WTS owner activity are **unverified on real
hardware**. The optional Linux TCG fixture requires an explicit
digest-pinned manifest and was not exercised.
The release gate stays closed pending independent review, passing checks
and owner qualification of the exact package and native controls.
