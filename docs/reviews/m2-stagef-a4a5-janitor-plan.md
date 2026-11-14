# Stage F A4/A5 startup janitor source plan

2026-11-14; OpenAI / gpt-6.1-sol / high; design sub-agent; document only.
Base: `c9a992498fbaed4091710ef91e96aec2a7678181` (initial worktree clean).
A1/A2 accepted for their recorded run only. Per task steering, both A3 runs
completed; None acceptance review remains active. No future execution authority.
This plan grants no implementation, native run, dispatch, retry or activation.

## Exact source pins

Git blob IDs at the base above; paths relative to the repository root:

| Path | Blob |
| --- | --- |
| AGENTS.md | c2ae25947916672b13180778265354907b75e295 |
| docs/KICKOFF.md | 9dc49e726bce3af9144ac867b5f7d2c96f4bca77 |
| docs/briefs/0020-linux-hierarchy-implementation.md | 07ecbf698b210752c12c9c14ab8e8b3a15e74f6a |
| docs/adr/0020-linux-execution-hierarchy-and-janitor.md | fae2048431c77141503d0fec7fbdaa54288d4f52 |
| crates/arxd/src/exec/hierarchy/setup.rs | b33bed7cd5d7022f30c039e761c544567304d28c |
| crates/arxd/src/exec/hierarchy/linux.rs | ae26fad095242b2d01e48e84e8695261e295cad3 |
| crates/arxd/src/exec/hierarchy/fake.rs | 9a8ecf7262573b3041e1cabab83d94ff173e39ea |
| crates/arxd/src/exec/disposable.rs | e1644a95ee1b8691abda8f9e4f034930a65cfc23 |
| crates/arxd/src/service/unix_bridge.rs | 01c626dddbbb43ac43f4d332a79c15dcc1bc02d6 |
| crates/arxd/src/service/unix_bridge/readiness.rs | 1ff55b22f810e1907e21a2edc9c9df07be868f92 |
| tools/unix-exec-validation/driver.py | 1da0b695ac660f1bc2fbd5b59756661efdc09016 |
| tools/unix-exec-validation/test_driver.py | a891e1807586986ee851a3d030041b8744cfaa70 |
| tools/unix-exec-validation/mutation_check.py | df9a40a61a34068ec1dd0c8af22a30ba0f7c1fb9 |
| packaging/linux/arxd.service | 0b337773c60e49fe84de76db018d2235ae076254 |
| packaging/linux/50-arxcovo-standard-rights.conf | 93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74 |

## Minimal reuse and production assertion map

Reuse the protected offline prebuild, fixed `arxd.service`, unchanged packaged
Standard profile, exec-in-place launcher, `hold_identity_ack`, runtime attribution,
reservation and owned cleanup. Current TEST is A3 None; leave it unchanged here.
Future A5 selection would be one closed literal `exec::disposable::disposable_linux_a5`,
derived Standard profile only; retain existing cases, no independent mode switch.
Do not use cross-account fixture, account setup, shim, real Runner or product activation.
`prepare_a1a2` requires zero removals and newly created runs: it cannot accept leftovers.
Reuse its bounded postcondition checks, with explicit A4/A5 evidence expectations,
in test-local code; do not relax historical A1/A2 assertions or extend production ports.

The acceptance call is `setup::setup(&LinuxFs::system(), &location, &clock, &token)`;
location comes from `proc_cgroup`/`hierarchy::locate`, not supplied paths.
S0–S6 must pass unchanged: exact hardening, root identity, bound location/ancestors,
delegation, domain/controllers, empty unit/runs procs, sole current PID in supervisor.
S7 writes `runs/cgroup.kill = 1`, then strictly parses `cgroup.events` until unpopulated
(5 ms polls, 5 s logical deadline, cancellation checked); it validates ALL leaves
before `rmdir`, then freshly lists and checks unpopulated again. S8 enables runs;
S9 opens memory.max/memory.swap.max/pids.max and confirms no procs/children/population.
Require `Ready(SetupEvidence { leaves_removed: 1, runs_created: false,
unit_written: true, runs_written: true })`, exact planted leaf absence and S9 checks.
No READY notification or successful test summary on failure, cancellation or residue.

Execution readiness is distinct from systemd `READY=1`. `setup` returns evidence;
`StartupDomain::new` consumes that result, then `Readiness::set_ready` admits execution.
That wiring is private cfg(test), not an activated production service. Exercise its
existing fake-runner path for Pending refusal/zero calls, setup completion before
admission, and failure/shutdown refusal. Native setup + notification alone cannot
claim an actual MCP execution or discovery acceptance; A8/Stage G remain separate.

## A4 decision remains open

Required chain: prior fixed-unit instance completes setup, creates one bounded
test-only workload in its owned leaf and proves live membership/populated=1.
Bound one helper by prewritten memory=64 MiB, swap=0, pids=1 and a fixed lifetime.
Then SIGKILL the identity-bound main process. The next instance must observe that
SAME live populated residue before calling production setup; only its S7 may kill
it. Confirm held process identity/death, unpopulated state, removal and readiness order.
Ordinary startup creates no child/leaf before execution Ready. Planting is an explicit
ignored-fixture exception in the PRIOR instance after its setup, never normal startup,
never an early discovery/run. A controlled helper is not cross-account execution proof.

Concrete unresolved semantics: `KillMode=mixed` may kill the workload on main death;
systemd may remove the subtree at stop (ADR U9). The unchanged fixture has Restart=no,
and the current monitor expects success, not a deliberately killed first instance.
A4 needs a reviewed bounded two-instance sequence, stable residue provenance and
per-instance PID/capture/ack ownership within existing deadlines before source work.
Do not weaken KillMode/profile, bypass manager lifecycle, move survivors outside runs,
plant after the crash, or treat an already empty/absent leaf as A4 success.
If unchanged-package semantics cannot preserve population, report A4 unestablished;
do not build a general restart harness or pursue optional diagnostics to force credit.

## Independent bounded A5 preparation

A5 needs only an empty orphan at production-setup entry, no survivor/crash machinery.
Future ignored fixture seeds exactly the fixed canonical leaf `arx-0-0` under
exclusively owned runs before its sole setup call; no child is spawned. Require the
unchanged packaged Standard drop-in, attributed by the existing exact runtime-file
checks; a None-profile host is ineligible. The fixture carries the identical four
disposable-host environment gates and real/effective root checks used by A1/A2.

Home the seeder in `hierarchy/linux.rs` as one narrow `#[cfg(test)]` inherent
`LinuxFs` helper, where private `Dir` descriptors are accessible. It may return held
runs/leaf `Dir` handles to the ignored fixture; do not expose raw fds or add a
`CgroupFs` method. Use descriptor-relative exclusive mkdirat for `arx-0-0` only.
Before either mkdir, perform these read-only port calls, failing on any mismatch:
`proc_status` with PROC_MAGIC and `hardening == Standard`; `identity == (0,0)`;
`proc_cgroup` with PROC_MAGIC and `locate` equal to the fixture's location;
`open_root`/`stat`, then each `open_child`/`stat` along `unit_components`, all root
owned cgroup2 directories with no group/other writes; and `xattr_delegate == b"1"`.
Also read unit type/procs/controllers and supervisor procs through the existing
bounded port: domain, no unit processes, memory/pids available, trusted supervisor
holding only `pid()`. These are read-only copies of setup's existing conditions,
not a call to setup or an enable/write. The production setup repeats them later.
Then require `mkdir_runs == Created` (never adopt an existing directory), open/stat
runs as trusted root-owned cgroup2 0700, and exclusively create the literal leaf.
Require empty procs, populated=0 and no child directories before setup. Keep handles
through setup/postconditions; assert one removal, runs reused, and no notification early.
This models orphan state at janitor entry, not a demonstrated systemd crash lifecycle.
No setup-then-second-setup substitute, manual leaf removal before assertion or new selector API.

## Ownership, lifetimes and feedback before any privileged run

Keep the EXCL/flock reservation from collision inspection through final cleanup;
collision owns nothing. Exact package/drop-in bytes/set and MainPID/artifact/supervisor
checks remain before release. Bind seed objects to that unit/run; reject links,
foreign/replaced objects and nested/unknown leaves; never adopt or delete them.
Keep 4096-byte control reads, 512-record/64-child listings, 1 MiB capture, existing
120 s hold/monitor, 140 s runtime, 40 s stop and ten-minute job budget. A4 must fit
these bounds before implementation; no silent timeout multiplication. Final cleanup
stops only the identity-proven owned unit, snapshots in the existing finally position,
requires cgroup absence, removes verified runtime/stage objects and releases reservation.
Uncertain cleanup retains evidence/reservation and forbids host reuse.

Future pure loop: extend Fake/Manual around H3/H4/H6/H18/H19/H20 with one populated
and one empty seed, exact removed count and operation ordering; fake readiness stays
Pending through kill/wait/removal/S9. Mock driver selection/profile, identity hold,
wrong summary, collision/replacement and cleanup failures without launching anything.
Tie mutants to assertions: omit kill → populated seed fails; skip wait → ordering fails;
skip rmdir → count/absence fails; stale listing → H18 residue fails; early Ready →
Pending/zero-runner-call assertion fails; accept wrong result → summary assertion fails.
These are planned checks, not runs or acceptance. Only source reads/document/Git work
occurred here; A4/A5, remaining F/G and M2 stay open, **unverified on real hardware**.
No owner question is needed for these routine engineering decisions.
