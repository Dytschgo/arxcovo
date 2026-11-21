# ADR 0048 · Hosted Linux cluster build executor preflight

Status: Blocked at preflight (2026-11-21). Review draft.

## Decision

Hold M3 14a implementation and the dependent C14/C16 campaigns. The hosted
Docker and UID-staging profiles established by #199 and #229 cannot supply
the broker with the Standard executor's required cgroup namespace, placement
and writable delegation together. The missing facility is a trusted
provisioner that establishes and confines that hierarchy before the broker
starts. Qualifying such a facility requires a separate owner decision under
the D4 feasibility disposition.

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

## Evidence and blocker

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

## Resumption condition

An owner-approved isolated Linux facility would need to provide a private
view rooted strictly above the matching delegated unit and strictly below
the host hierarchy root, place the broker in the unit's sole-process
`supervisor` leaf, and make only the subordinate delegation writable.
It must also demonstrate the dedicated build account, independent daemon
identity and protected state, the broker's narrow capabilities, and actual
leaf migration and cleanup. A dedicated host-side namespace/placement
provisioner or a change to the frozen cgroup qualification contract
requires separate authority. The adopted one-build allowance is unused.
Actual hosted executor and installed-service or physical paths are
**unverified on real hardware**.
