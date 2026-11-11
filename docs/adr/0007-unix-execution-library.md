# 0007: Unix execution library before transport integration

Status: implementation candidate; independent security review required. Builds
on proposed ADR 0006 (`dd0840f`). Unix execution is **unverified on real hardware**.
This does not complete milestone 2 or authorize owner account/service operations.

The library is exported as `arxd::exec`; no CLI, HTTP or MCP handler calls it.
Owner configuration is a Rust value, deliberately without deserialization or
Debug output. A future owner-only loader must enforce its access boundary. Agent
input supplies an absolute executable, argv, relative cwd, timeout and output
quota. There is no implicit shell, fixed cargo endpoint or inherited environment.

The daemon must have real/effective UID zero. Before each spawn NSS must resolve
the pinned account UID/GID and its account-named primary group. Any supplementary
group other than that GID refuses execution. The child clears all supplementary
groups using `setgroups`, then drops real/effective/saved GID and UID (Linux
`setresgid/setresuid`, macOS root `setgid/setuid`). Calling `initgroups` after
fork would invoke NSS and possibly allocate; membership is resolved before fork
and the child installs the strictly smaller empty supplementary set. All failures
abort exec. No owner/root identity fallback exists. Admin configuration is
unavailable pending a separate owner policy implementation.

Linux additionally sets no-new-privileges, disables keepcaps, clears ambient and
effective/permitted/inheritable capabilities, and verifies that capability data
is zero. Its child uses `openat2` with no-symlinks, then beneath/no-symlinks for
relative cwd. Missing syscall support refuses execution. macOS uses a sequence
of `openat(O_DIRECTORY|O_NOFOLLOW)` calls anchored to retained descriptors, then
`fchdir`. All workspace opens happen **after** dropping identity. No privileged
workspace stat/canonicalize is used. These are cwd resolution restrictions, not
a filesystem sandbox: arbitrary builds retain the dedicated account's ordinary
OS access. The owner must prevent writable mount aliases and protect service
state with OS permissions. macOS has no Linux no-new-privileges guarantee;
set-ID programs remain subject to OS policy. Do not claim post-exec continuous
identity attestation on either platform.

Owner workspace and enumerated service/key roots must be lexically disjoint and
without symlinks. Private roots and every ancestor must be root-owned, without
group/other writes; final private roots have no group/other access. macOS paths
must therefore spell the real `/private/...` path when `/var` or `/tmp` is a link.
After the drop, the child also attempts `fchdir` on retained private-root
descriptors and refuses unless search access is denied; this checks actual OS
ACL enforcement without opening a workspace as root. Protected roots must enumerate **all** service configuration, state and key
directories; this library does not locate them or invent key storage policy.
Account-specific ACLs, sudoers/authorization policy and owner-provisioned mounts
still require independent review and disposable platform evidence. Account name
and NSS membership are rechecked per run, not locked against owner changes.

Linux requires an existing root-controlled cgroup2 subtree, empty of direct
tasks, with memory and pids controllers already enabled. Every ancestor must be
root-owned and not group/other writable. Per-run children get `pids.max`,
`memory.max` and zero swap allowance. Creation, controller setup and availability
of `cgroup.kill` must succeed before spawn. The child writes itself into the run
cgroup before dropping identity or executing any untrusted code. The build UID
must not be delegated any writable cgroup elsewhere; provisioning must verify
that policy. Every outcome kills the cgroup, polls populated state and attempts
removal. Normal completion confirms both `populated 0` and successful leaf
removal; either failure sets `tree_cleanup_confirmed=false`. Error-unwind/Drop
cleanup remains best effort, and every execution error requires caller quarantine;
Drop alone cannot emit a confirmed result. Cleanup failure is explicit; the integration must quarantine failed
cleanup and reconcile remaining run directories after daemon restart. This is
not yet a crash/restart janitor. No process-group fallback exists on Linux.

macOS process-group SIGKILL is best effort. A descendant can create another
session/group and escape; `tree_cleanup_confirmed` is always false. There is no
claim of macOS memory/pids containment. This limitation blocks a complete tree
fence acceptance claim. The runner observes exit without reaping until cleanup,
keeping the group leader PID reserved while signalling its group.

Linux close-range CLOEXEC and a bounded macOS descriptor CLOEXEC sweep prevent
daemon descriptor inheritance. Child stdin is `/dev/null` (EOF); stdout/stderr
are nonblocking and drained fairly with bounded per-poll work. Each stream keeps
at most its requested head+tail quota, with a saturating count of discarded
**read** bytes. All bytes remain untrusted. Cancellation, timeout and the shared
shutdown token trigger cleanup, including after the leader exits. The caller
must signal shutdown and join its run threads. Forced daemon termination needs
the future restart janitor. Polling is 5 ms; cleanup/reaping adds bounded waits.
Uninterruptible kernel I/O, NSS or a filesystem stall during synchronous spawn
cannot be given a strict wall-clock deadline by this implementation. Deploy only
after review of this limitation; no real-time guarantee is claimed.
macOS requires an owner-provisioned finite hard descriptor limit no greater than
1,048,576; an unlimited hard limit refuses execution rather than risk inheritance.

The observed identity is transferred on a private CLOEXEC socket **from the
child** after dropping IDs and opening cwd, before exec. It is labelled as such
in `RunResult`, with daemon-root mode, process outcome and cleanup confirmation.
Output may itself contain private data generated by the build account: a future
transport must apply access control and avoid indiscriminate logging. The library
does not read daemon environment or log argument/environment values.

**Audit integration contract:** the authenticated caller's ActivityStore must
commit `started` before invoking `Runner::run`; transaction failure forbids the
call. It must persist results/errors, quarantine failed cleanup and reconcile
unfinished activity on restart. The library has no independent DB and cannot
prove this caller-side ordering. Wiring it to MCP before review and tests of this
ordering is out of scope.

Dependency: exactly `libc = 0.2.191`, Unix-only, reusing the existing workspace
lock entry. Signatures were checked against the downloaded registry source for
that exact version, including Apple's signed `getgrouplist` GID versus Linux's
unsigned GID. docs.rs version pages and upstream GitHub could not be fetched by
the web tool; no successful upstream fetch of that pin is claimed. Other primary
references checked: [Rust pre_exec safety contract](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html#tymethod.pre_exec),
[kernel cgroup2 controls](https://docs.kernel.org/admin-guide/cgroup-v2.html),
[openat2 manual](https://man7.org/linux/man-pages/man2/openat2.2.html),
and [Apple setuid/setgid semantics](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/setuid.2.html).
