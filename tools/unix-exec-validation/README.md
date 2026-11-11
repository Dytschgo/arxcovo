# Disposable Linux experiment tooling

This is a future manual CI experiment, not a local provisioning tool. Run only
the pure tests on a development machine:

```powershell
python -m unittest discover -s tools/unix-exec-validation -p 'test_*.py' -v
```

`driver.py run` refuses wrong platform/context before any mutation or privileged
probe. Internal modes also require root and the exact protected installed path.
Do not invoke `run`, `service` or `harness` locally, even with spoofed CI variables.
Environment checks are accidental-use guards, not host attestation.

The public CLI accepts only `guard` or `run`, with no parameters. Run identity is
two bounded positive decimal GitHub run numbers. The driver reserves an exclusive
`/opt/arx-ci-<run>-<attempt>` directory and same-named transient systemd unit.
Source, registry cache and Rust 1.95.0 toolchain are copied to root-private paths;
links and special files refuse copying. No fixture task account can replace the
source, driver, Cargo, rustc, compiled executable or runner. Cargo is offline,
locked, and receives only the explicit environment in `cargo_env`; no token,
credential file, runner environment dump, or Git config is copied. Checkout's
credential persistence is disabled. Compilation happens before fixture creation.

Read-only preflight requires systemd, cgroup2, available memory+pids, protected
ancestry and a bounded hierarchy scan without non-root ownership, links or
group/other write grants. This deliberately conservative scan can refuse a
hosted image with unrelated delegation. Do not weaken policy to make it pass.
There is no claim that GitHub's default kernel/image satisfies these requirements.

Only the generated service gets `Delegate=yes`. The driver moves itself into a
new `control` child before enabling memory+pids in the service root, then makes
an empty protected `runs` child with those controllers enabled. No global root
controller settings are written. The library's per-run children live below
`runs`; the fixture UID gets no cgroup ownership or write grants.

A root-owned Cargo runner verifies the executable and exact libtest arguments,
checks fixture/account/group absence and records its PID before exec. The harness
itself creates `arxexec<PID>`, its private group and `/var/lib/arxexec<PID>`.
The service captures fixture inode and numeric UID/GID while the test is alive.
If residual resources lack capture, cleanup quarantines rather than guessing.
The harness currently does not emit the observed UID; success means its internal
identity assertions passed, not that an independent UID trace was collected.

Finally, stopping only the captured transient unit must remove its cgroup tree.
Residual account cleanup requires captured UID/GID, expected shell/group, no live
UID processes, and matching protected fixture path/inode. Removal uses symlink-safe
`rmtree`; no glob account deletion or recursive cgroup deletion exists. An orphan
group or changed resource fails cleanup. Failure retains root-private staging for
quarantine and fails the job; the fresh hosted VM is discarded. A hard workflow
cancellation/VM termination cannot guarantee Python finally runs; discarding the
host is the last boundary, not proof of successful cleanup. Do not retry probes.

The ten-minute job bounds a 360-second build, a 120-second exact Cargo invocation,
and a transient service with 140-second runtime plus ten-second stop timeout.
The service output file is capped at 1 MiB. Public evidence is restricted to fixed
success/cleanup lines or allowlisted error codes. Raw build/test output stays
root-private and is not uploaded; no panic text or environment is printed.

Primary references checked during preparation:

- [Checkout at the existing pinned SHA](https://github.com/actions/checkout/blob/11d5960a326750d5838078e36cf38b85af677262/README.md), including `persist-credentials`.
- [Rust toolchain action at the existing pinned SHA](https://github.com/dtolnay/rust-toolchain/blob/683b588c487a69d01f6c6ddf40f036ff682cc2c8/action.yml).
- [Kernel cgroup v2 documentation](https://docs.kernel.org/admin-guide/cgroup-v2.html), including no internal tasks and controller delegation.
- [systemd resource-control primary source](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd.resource-control.xml) and [systemd-run primary source](https://raw.githubusercontent.com/systemd/systemd/main/man/systemd-run.xml), covering delegation and transient services. The rendered freedesktop page timed out; upstream manual sources were fetched successfully. Installed-runner behavior still needs independent review and CI evidence.

Static tests are guard/planning evidence only. Runtime setup, systemd trimming,
Cargo runner arguments, account cleanup and Linux containment are **unverified
on real hardware**. No privileged experiment has been dispatched or executed.
