# ADR 0048 · Hosted Linux cluster build executor preflight

Status: accepted with A10 (2026-11-21). Hosted qualification pending.

## Decision

A10 adopts the disposable GitHub-hosted Ubuntu VM as the isolated Linux
facility for M3 14a and C14. The runner's existing sudo starts the broker
as the transient systemd unit named exactly `arxd.service`. The unit uses
the packaged delegation shape `Delegate=memory pids` and
`DelegateSubgroup=supervisor`. A private mount view gives the broker
read-only access to the cgroup hierarchy and a writable exception for its
own unit. A full `ProtectControlGroups` would prevent setup. The independent
Candidate H review preceded this equivalence decision. Its SHA256 is
`d2ea8190a6a8c71d8978e3722a93f5a294ff564699d7cf7095d9ed46516c91c6`.

A10 accepts build-UID read-only visibility of this job VM's cgroup
metadata. The job contains no owner data or secrets, and the layout follows
the packaged native delegation model. The build UID must still be denied
writes to outside cgroup controls and access to custody, broker roots,
history, control sockets, other processes and their descriptors. Fixed
distinct UID/GID accounts and a protected root-owned staging parent under
`/var/lib` carry those boundaries. The build UID has its own primary group
and no supplementary groups. The existing Runner, product privilege
contract and native packaging are frozen.

C16's three-container boundary remains open for 16a. A10 grants no
container equivalence, installed-service or physical-host qualification.

This preflight reviewed base `be64f8db29dae9b58ffdd35380cb966ddd94025c`, the
adopted 14a grant and the plan amendments. R5 concerns later C14 admission
and depends on a qualified executor.

## Frozen executor contract

The cluster daemon runs as a dedicated nonroot UID. Only the private
executor broker needs real and effective UID 0. Standard readiness expects
`NoNewPrivs=1` and a bounding set of `CAP_CHOWN`, `CAP_SETGID` and
`CAP_SETUID` (`0xc1`). The ready broker's actual
`/proc/self/cgroup` must resolve to `…/arxd.service/supervisor`, with
exactly the broker PID in `supervisor`. The matching filesystem walk from
`/sys/fs/cgroup` requires root-owned, protected cgroup v2 ancestors,
a service manager's `user.delegate=1` marker on `arxd.service`, an
empty unit-level process list and memory and PIDs available to the
delegation. The setup enables both controllers, owns a separate `runs`
subtree and checks cleanup before admitting work.
[Location and hardening parser](../../crates/arxd/src/exec/hierarchy/parse.rs) ·
[Setup predicates](../../crates/arxd/src/exec/hierarchy/setup.rs) ·
[Descriptor walk](../../crates/arxd/src/exec/hierarchy/linux.rs) ·
[systemd delegation](https://systemd.io/CGROUP_DELEGATION/)

The unchanged Runner creates one limited leaf per admitted run, sets memory,
swap and PID limits, and uses `cgroup.kill` for cleanup. Its child joins the leaf
before dropping to the pinned build UID/GID, clears capabilities and inherited
descriptors on exec, and verifies access to the workspace and denial of the
broker's root-only protected directories. Daemon-owned custody must also be
inaccessible to the build child and is a separate boundary.
[Runner cgroup controls](../../crates/arxd/src/exec/cgroup.rs) ·
[Runner identity and child transition](../../crates/arxd/src/exec/unix.rs)

## Original preflight evidence

| Route examined | Finding |
| --- | --- |
| #199 custody container | The hosted recipe stages distinct numeric UIDs and private storage/workspace mounts, drops all capabilities and enables no-new-privileges. Docker starts the UID-specific custody and denial witnesses. The recipe sets outer limits on the container and supplies no writable delegated cgroup mount or broker placement. |
| #229 C13 launcher | Hosted-only sudo stages root-owned artifacts and starts fixed distinct-UID daemon processes with bounded ownership and cleanup. The launcher supplies neither a private cgroup/mount view nor the delegated service hierarchy. Its UID handoff establishes process identity. |
| Docker private cgroup namespace with the broker's exact capabilities added | Docker's default cgroup mount is read-only. The broker placed by Docker starts at the namespace root and sees `0::/`, which the frozen locator rejects. `--cgroup-parent` changes the outer placement but still places the process in a Docker-owned child and leaves the namespace rooted there. Changing only the mount to writable would still leave membership and delegation unqualified. |
| Narrow bind of an existing host delegation | A bind changes the mount view while `/proc/self/cgroup` still describes the task's actual cgroup relative to its namespace. Binding an adjacent service into the private view supplies neither the required membership nor a valid migration into the sibling `runs` leaf. Sharing the host cgroup namespace with the default cgroup mount exposes the host-wide tree. Replacing that mount with a narrow bind alone leaves host-relative membership paths inconsistent with the frozen walk. |
| Existing systemd native route | A native service can express `Delegate=memory pids` and `DelegateSubgroup=supervisor`. That setup supplies unit placement in the native host cgroup tree. The prior manual execution experiment also stopped with `E_PROTECTION` before its harness pass. Neither establishes the scoped namespace/mount boundary needed here. |

Repository precedents and native reference.
[#199](https://github.com/Dytschgo/arxcovo/pull/199) ·
[Custody launcher](../../tools/ci/custody/run.py) ·
[13d](https://github.com/Dytschgo/arxcovo/pull/229) ·
[C13 launcher](../../tools/ci/m3-c13/run.py) ·
[Native service](../../packaging/linux/arxd.service) ·
[Prior hosted experiment](../evidence/unix-execution-first-dispatch.json)

Upstream behavior checked against Docker's documented private default,
the Moby 28.0.4 default mount and cgroup placement code, and the kernel's
namespace and migration rules. The Ubuntu image inventory identifies Docker
as an available runtime. This is a source-backed disposition of the existing
profiles. The hosted image's live cgroup, migration and capability behavior
remain unverified.
[Ubuntu 24.04 image inventory](https://github.com/actions/runner-images/blob/main/images/ubuntu/Ubuntu2404-Readme.md) ·
[Docker cgroup defaults](https://docs.docker.com/engine/containers/runmetrics/#running-docker-on-cgroup-v2) ·
[Moby default mount](https://github.com/moby/moby/blob/v28.0.4/oci/defaults.go#L81-L92) ·
[Moby cgroup placement](https://github.com/moby/moby/blob/v28.0.4/daemon/oci_linux.go#L729-L757) ·
[Kernel cgroup v2 delegation and namespaces](https://docs.kernel.org/admin-guide/cgroup-v2.html#namespace)

## A10 qualification

The Linux Rust worker reuses existing compiled artifacts. Leg 1 checks
provenance, kernel and manager versions, account and mount protection,
the broker's exact capabilities and membership, effective memory/PID
delegation and the raw marker. An explicit build-UID identity process
checks read visibility and attempts to open cgroup controls for writing
without issuing any write. The daemon-UID client exercises the credential
check, refusal on changed file protection and build account drift, and
restored readiness. The transient broker drains and stops before
the first leg can pass. Setup, identity and drain perform zero Runner
dispatches and zero consensus invocations.

Leg 2 restarts the same broker profile in the same job and consumes the
one granted Standard execution only after the first leg passes. The
daemon UID authenticates the root broker and submits the fixed read-only
`cargo --version` witness through normal admission. The actual build
child demonstrates the denial set and workspace access. An independent
observer confirms cgroup migration, configured limits and credential
drop. The daemon owns admission and terminal persistence. The probe
checks the leaf's removal and reports cleanup separately from the first
failure. Hosted timing and the observed profile
are pending the reviewed hosted run. Installed-service and physical paths
are **unverified on real hardware**.
