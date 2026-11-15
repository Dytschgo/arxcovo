# A9/U9: bounded stop and drain evidence join

2026-11-15; Sol implementation sub-agent; documentation only.
**A9/U9 remain OPEN. Fresh independent Opus review is required before publication.**
Parent `657b825` retains the pending A10 note and PR139 checkpoint; neither is
treated as merged acceptance. Scope: [KICKOFF M2/M3](../KICKOFF.md),
[ADR0020 D9/U9](../adr/0020-linux-execution-hierarchy-and-janitor.md),
[brief A9](../briefs/0020-linux-hierarchy-implementation.md), and the
[Opus bounded-join recommendation](../reviews/m2-a10-capability-evidence-opus-review.md).

## Accepted runtime evidence, joined to exact source

All six records bind attempt 1, successful completion and `cleanup: confirmed`
to these executed commits. Reviews retain the fixture/result join; internal
`absent` fields or green jobs alone are insufficient. No live recheck is claimed.

| Selection / local record | Exact executed commit | Accepted assessment |
| --- | --- | --- |
| A1/A2 [43627472312](linux-stagef-a1a2-run-43627472312.json) | `1e9d8818de1e337b1402ca7e2b71ac02456b82f3` | [A1/A2](../reviews/m2-a1a2-hosted-evidence-muse-review.md) |
| A3 Standard [43635131545](../reviews/m2-a3-standard-hosted-record.md) | `beb7117b08aad68da790e05a85b335f9655f5236` | [Standard](../reviews/m2-a3-standard-hosted-muse-review.md) |
| A3 None [43636804545](../reviews/m2-a3-none-hosted-record.md) | `4bb15612e30ec5bbf0c019bab347c83fd60a9d0a` | [None](../reviews/m2-a3-none-hosted-muse-review.md) |
| A4 positive [43666413693](linux-a4-baseline-run-43666413693.json) | `a328060000b8cb7bb389d39f8785f96f6bcb0c25` | [Positive baseline only](../reviews/m2-a4-hosted-baseline-muse-assessment.md) |
| A4 omitted S7 [43674201597](linux-a4-negative-run-43674201597.json) | `1958f9975d1620151ca9a182c5047dd8aeb43e65` | [Combined D9a scope](../reviews/m2-a4-native-negative-hosted-muse-assessment.md) |
| A5 [43644661407](../reviews/m2-a5-hosted-record.md) | `cf3a2510fc45e0cb438ae14cc41115e492bc0f41` | [A5](../reviews/m2-a5-hosted-muse-review.md) |

Historical [driver](../../tools/unix-exec-validation/driver.py) at each SHA supplies the order:
`run_body` starts the fixture unit, then `monitor_a1a2` acknowledges MainPID,
executable and supervisor membership, waits for terminal `inactive`/`failed`,
requires `Result=success`, `ExecMainStatus=0` and the exact passing test summary.
Its `finally` calls `cleanup_a1a2`: validate owned runtime files/unit attribution,
run `systemctl stop arxd.service` (40 s bound, require zero exit), snapshot,
then require `not os.path.lexists('/sys/fs/cgroup/system.slice/arxd.service')`.
Only afterward remove owned runtime/stage assets and set cleanup `confirmed`.
The driver does not remove cgroup directories itself on this success path.

Compared exact driver deltas: A1/A2→A3 adds profile selection and exact loaded
runtime verification; A3 Standard→None changes only TEST. Standard→A5 adds
A5 selection; A5→A4 positive adds helper delivery/removal, with helper removal
after the cgroup-absence check. Positive→omitted adds negative selection and
marker validation. None changes stop/absence ordering. A1/A2 checks FragmentPath
and owned objects, without the later post-load DropInPaths/byte verification.
Standard is installed except for A3 None; see the [per-run A10 attribution](linux-a10-capability-evidence-join.md).

A1/A2's historical `exec/hierarchy/fake.rs::prepare_a1a2` requires fresh empty
`runs` after setup. A5's `exec/disposable.rs` checks `a5::empty` after one orphan
removal. Both A4 `exec/disposable/a4.rs` versions check `a5::empty` before READY:
positive setup kills/reaps/removes the helper leaf; omitted S7 first proves
KillTimeout/retained live helper, then separately kills/reaps/removes it in
owned failure cleanup. A3's `exec/disposable.rs` only observes hardening;
it does not create/test `runs`. No selection leaves a populated run leaf at exit.

This adds accepted evidence that systemd removed the unit cgroup, including
`supervisor/` and, where created, empty `runs/`, at deactivation on those fresh
hosted Ubuntu 24.04 hosts. Absence is observed after stop, not timed to it:
the fixture already exited naturally before cleanup. It does **not** prove a
signal-driven stop, SIGTERM handling or `KillMode=mixed` escalation/removal.

## Production wiring and tests: source-only coverage

At the parent, [Unix Signals](../../crates/arxd/src/service/unix.rs) forwards TERM/INT
to shutdown. [Service lifecycle](../../crates/arxd/src/service/mod.rs) calls
stop_accepting, cancels, bounds drain to 10 s (failure exit 5), then audits/releases.
[Application::drain](../../crates/arxd/src/service/application.rs) joins serving,
then [Execution::drain](../../crates/arxd/src/service/executor.rs) cancels/joins workers,
then blocking work; discovery-sequence join is `cfg(test)`, not activated production.
[Runner loop](../../crates/arxd/src/exec/unix.rs) observes shutdown and calls tree
cleanup; [Group](../../crates/arxd/src/exec/cgroup.rs) kills/polls/removes leaves and
flags Drop cleanup failure. The [unit](../../packaging/linux/arxd.service) sets
KillMode=mixed and TimeoutStopSec=30s. These are wiring, not accepted runtime proof.
[Lifecycle tests](../../crates/arxd/src/service/tests.rs) include
`stuck_drain_is_bounded_and_never_claims_stop_audit`; [E7 tests](../../crates/arxd/src/service/unix_bridge/tests/e7.rs)
include `sp1_sp2_actual_drain_precedes_sequence_and_empties_workers` over injected
ports/held workers. Neither substitutes for real populated-leaf systemd shutdown.
No tests were run for this document; production Drain never ran in the six fixtures.

## Remaining gaps and next independent slice

A9/U9 stay open: populated-leaf stop, crash during run/setup, actual production
Drain/Runner cancellation and residue, installed/dedicated packages and version
coverage are **unverified on real hardware**. M2/full F/G/real-Runner PA6 stay open.
[A12](linux-a12-hosted-result.md) failed on `2a1d5ad`, cleanup unconfirmed:
no credit, no clean-host inference. [A7/A11 FINAL NO-GO](../reviews/m2-d7k-final-hosted-nogo-opus-assessment.md) stays closed.
After this one join, recommend independent M3 source/doc work on KICKOFF's
15-minute single-use join-code contract and expired/reused-code rejection cases,
with cluster CA/mTLS architecture routed to Opus. This does not mark M2 complete.
No grant, diagnostic, activation or further evidence framework is proposed.

Checks: local Git/source/docs reads and historical diffs; relative-link, UTF-8/LF,
line-count, scope and whitespace checks before commit. No tests/builds/network.
