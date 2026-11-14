# Grok review: Stage F A1/A2 alignment

2026-11-14; Grok (grok-4.7); single-agent independent design review. This
reviewer did not write the proposal. Both axes were read in this pass because
the task forbade further reviewers. Read-only against source, Git, and the
published systemd resource-control manual. No tests, builds, native runs, or
service operations were performed.

This acceptance covers source preparation only. It does not merge, publish,
dispatch, or authorize a native run.

## Pins

- Clean top of `t3/review-m2-stagef-alignment`, HEAD
  `eb35768618d6af5ef05a2e9daa335e5c51c48195`.
- Proposal blob `60faace01563c5c56a1aadd97df8fd4fb1910c7d` is identical at
  `b0466f377a38c30966359b01d08317bf2ec44e0e`,
  `a3d02c33b0f415d12768a4bceaac2d7699025c7c`, and HEAD. Those two proposal
  commits share parent `4a883906d0c399a5dcfd5c0f81b9034278ad886f` and have an
  empty diff.
- Cached `origin/main` is `625eeb062bd89c1b2692b2cc99759a809e69e849`. No fetch.
  Its tree differs from the proposal parent only in `docs/PROGRESS.md`, so the
  pinned driver, fixture, setup, unit, drop-in, and workflow blobs match this
  checkout's source.
- Manual workflow remains `e5f1a96dbfa522bda602db45bf24db000ebddd5c`
  (`.github/workflows/unix-execution-experiment.yml`): `workflow_dispatch`,
  both actors, `refs/heads/main`, reviewed SHA equal to `github.sha` and
  `github.workflow_sha`, `contents: read`, non-persistent checkout,
  `ubuntu-24.04`, ten-minute job.

## Standards

Pass. The proposed patch stays inside the driver, its Python tests, the
mutation runner, `crates/arxd/src/exec/disposable.rs`, and adjacent fake-port
tests. Packaging, workflow, activation, and owner APIs stay byte-identical.
Ordinary CI stays on fakes; the new test stays ignored. The shape does not add
a service framework, a human approval step, or a second evidence schema.
AGENTS.md's rules on meaningful tests, unrun hardware, and technical fences
are stated and kept. No baseline smell applies: the fixed unit and one exec
path replace the generated transient unit rather than wrapping it.

## Spec

The prebuilt ignored libtest, exec'd as MainPID of fixed `arxd.service` with
the packaged `DelegateSubgroup=supervisor`, meets `locate` and production
`setup` before any child. One capture-ownership correction is required.

### What holds

`DelegateSubgroup=` (systemd 254; the manual workflow image is `ubuntu-24.04`)
starts `ExecStart=` in `arxd.service/supervisor` and leaves the unit cgroup
empty. There is no `User=` and no `ExecReload=`, so the main process stays
root and no `.control` peer is created. `locate` accepts that path at any
slice depth. `setup` then requires, in order, standard hardening, uid and euid
0, the `user.delegate` byte `1`, an empty domain unit with `memory` and `pids`
available, and `cgroup.procs` in `supervisor` equal to `getpid()`. That file
lists thread-group ids once, so libtest threads are not extra PIDs. The
launcher must `execve` the prebuilt binary: a fork, a Cargo child, or
`/usr/bin/timeout` would fail the single-PID check. The fixture must not
create `runs`, synthesize the marker, or move into `supervisor`. Production
`setup` remains the only enabler and the only `runs` creator (`mkdirat` mode
`0700`), including S9.

The stage stays `/opt/arx-ci-<run>`. `ProtectSystem=full`, `ProtectHome=yes`,
and `PrivateTmp=yes` do not hide `/opt`. The build, toolchain copy, and
registry copy stay outside the unit, so `ProtectHome` never applies to them.
The closed exec environment is `NOTIFY_SOCKET` plus the four existing
disposable gates (`CI`, `GITHUB_ACTIONS`, `RUNNER_ENVIRONMENT`,
`ARX_DISPOSABLE_EXEC_TEST`). Do not pass `os.environ` or `cargo_env`.

Preserved from the pinned unit and drop-in: `Type=notify`,
`NotifyAccess=main`, both 30-second timeouts, `KillMode=mixed`, `UMask=0077`,
`Delegate=memory pids`, `DelegateSubgroup=supervisor`, `NoNewPrivileges=yes`,
`CapabilityBoundingSet=CAP_SETUID CAP_SETGID CAP_CHOWN` (CapBnd
`00000000000000c1`), `ProtectSystem=full`, `ProtectHome=yes`, `PrivateTmp=yes`.
`Restart=no` overrides `Restart=on-failure` for this runtime unit only. No
added capability and no profile widening. A1/A2 does not prove effective or
permitted capabilities, cross-account rights, or janitor cases A3–A12.

`Type=notify` becomes active when this PID sends `READY=1` through
`service::unix::Notify::send_to`, after `SetupOutcome::Ready` and the A2
asserts, and within `TimeoutStartSec`. No `MAINPID=` and no fork. Outer
compares MainPID, `/proc/<pid>/exe`, and the supervisor path while the test is
held on one private ack file on the mode `0700` stage. Ack those identity
checks only. libtest prints `test … ok` and `1 passed; 0 failed` after the
test function returns, so the ack must not wait on that text. After exit,
require the 1 MiB ceiling, that summary, and `Result=success` with
`ExecMainStatus=0`. Exit 0 before READY is a start failure. `Restart=no` does
not retry it.

Collision handling matches the fixed name. Refuse an existing loaded unit,
alias, fragment, runtime file, drop-in, or cgroup, including inactive or
identical. Re-check under the nonblocking fixed-unit lock. A collision returns
before mutation and its cleanup must not stop or delete that unit. Build,
protected artifact, argv, and the offline locked environment finish before any
unit file. Outer stop timeout stays above the packaged 30-second stop (40
seconds). Stop, remove, and reload only files this invocation created, after
the cgroup is confirmed gone. Uncertain stop, removal, ownership, or cgroup
absence quarantines the environment. No recursive cgroup delete and no account
delete. The cross-account test and its assertions stay in
`disposable.rs`, ignored and unselected. `ProtectSystem=full` would refuse
its `useradd`; do not widen the profile to make that work.

Mocked Python tests and fake-port setup tests are the ordinary CI evidence.
The mutation runner's current any-failure credit is too weak; the proposal's
exact-once anchor, green baseline, named assertion failure, and zero test
errors are the right bar. Native compatibility was not run and cannot be
inferred. Run `43091339273` stays an unknown `E_PROTECTION` cleanup and is not
retried. `NEW_SHA` is not a workflow bypass. M2, Stage F, Stage G, and real
Runner PA6 stay open. Main 71 and the optional diagnostic tracks stay parked.
Hardware stays **unverified on real hardware**.

### Required correction

The capture inode needs one creator. The proposal's drop-in allowlist includes
"output redirection to the protected bounded test capture", and the launcher
also "opens capture with private/no-follow rules". A `StandardOutput=` or
`StandardError=` path is opened by systemd before `ExecStart=`, without the
driver's no-follow exclusive create (`open('xb')` today). The launcher's
exclusive open then fails or no longer owns the inode.

The launcher alone creates the capture on the protected stage, dup2s it onto
both descriptors, and execs. The drop-in does not point `StandardOutput=` or
`StandardError=` at that path. The in-unit 1 MiB drain cannot move with the
parent, so the outer monitor must stop the unit this run created if the
capture exceeds 1 MiB while the process is held. The after-exit size check
stays. No other design change.

### Advisory (non-blocking)

State the ack-then-exit-then-summary order in the driver test so a later edit
does not wait for libtest's trailing lines before writing the ack. That wait
deadlocks: the test will not return until the ack, and the summary is printed
only after it returns.

## Accepted scope

After that correction, Sol may implement only:

- `tools/unix-exec-validation/driver.py`
- `tools/unix-exec-validation/test_driver.py`
- `tools/unix-exec-validation/mutation_check.py`
- `crates/arxd/src/exec/disposable.rs`
- adjacent fake-port tests that call the existing setup port

Do not edit the production parser or setup algorithm, packaging, workflow,
permissions, activation, or owner-install APIs. Documentation and evidence
belong to the implementation delivery. A native run still needs its own
reviewed proposal and an explicit owner disposition of the retained no-retry
rule, on a new disposable environment.

## Verdicts

- **Standards: pass.** 0 findings.
- **Spec: accept source preparation with 1 correction.** The launch matches
  `locate`, production setup, and the packaged fences. The correction is the
  single capture owner and the outer 1 MiB stop.
- **Security: pass with that same correction.** No additional finding. The
  change does not weaken root, hardening, delegation, protected-copy, SHA, or
  host-floor checks, and it does not adopt or delete an existing unit. Native
  execution remains unauthorized.

Standards 0. Spec 1; worst is the dual capture opener. Security adds none.
