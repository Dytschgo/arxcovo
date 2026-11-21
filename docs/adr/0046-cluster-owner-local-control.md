# ADR 0046 · Cluster owner-local control

Date 2026-11-21. M3 step 13c, amended by R1.

The Linux daemon and the operator use separate non-root OS accounts. The build
and local MCP accounts each use a third or fourth UID. The operator account
belongs to a dedicated control group. The build and MCP accounts lack that
group. Root and host administration define the deployment trust boundary.

The owner socket uses a pathname under protected ancestors. The dedicated
daemon-owned directory has mode 2710 and the daemon-owned socket has mode 0660.
Both use the dedicated control group. Each side walks the pathname without
following symlinks, verifies ownership and mode, checks the actual POSIX ACL
attributes and pins filesystem object identities. A root-owned sticky ancestor
may contain an entry owned by a trusted UID. The daemon verifies the exact
operator UID with SO_PEERCRED before reading a request. The client verifies the
expected daemon UID with SO_PEERCRED before writing one. The daemon account is
reserved for the daemon and its trusted processes.

The closed protocol admits Mint and a bounded Join paste. A half-close marks
the end of the one request and its one response. Frames carry at most 8,800 bytes
and use closed operation and disposition tags. Four connections may be admitted
at once. The listener supervisor owns admitted work through disconnect and
drains it on close. An uncertain receipt produces Unknown. A client invocation
submits once.

The retained Mint lease is admitted under the bound job's close gate. The
existing bounded fresh read establishes the exact leader epoch, matching
committed, effective and applied uniform membership, and current voter
bindings. The retained enrollment listener's kernel socket cookie binds the
observation to its exact accepting descriptor and advertised endpoint. The
namespace derives the actor from the authenticated UID, retained installation
context and cluster. Code and request IDs and code secret come from system
randomness. The existing Mint proposal carries the verifier, with the paste
held in memory. Only the original first result in the same epoch and a second
fresh observation of the original view and usable pending code authorize
release. A lost, stale or unverifiable result leaves any possible committed
slot intact and conceals the paste.

The Linux CLI uses the explicit `service cluster-control` command with the
socket path and expected daemon UID. It opens and checks the foreground
controlling terminal independently of redirected program streams. Mint writes
only to that terminal. Join reads one canonical envelope terminated by Enter
with echo and canonical mode disabled. The terminal guard restores settings on
success, errors, timeout and handled signals. The owner protocol and terminal
buffers use bounded allocation and volatile wiping on drop. ADR0040's durable
Join consumer owns the checksum, CA pin and local route checks. Temporary
values internal to the frozen codec retain their existing memory behavior.

Step 13c stages this source and exercises non-Engine controls. Step 13d wires
the protected endpoint and trusted installation context into daemon startup.
The C13 grant owns the first real Mint and full protected transfer. Installed
service and multi-host operation are **unverified on real hardware**.
