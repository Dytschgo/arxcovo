# M2 A4/A6 practical acceptance proposal

2026-11-14; OpenAI / gpt-6.1-sol / high; design sub-agent, document only.
Recommendation: replace A4's survivor provenance with native populated-startup cleanup;
use deterministic injected-port A6 failures. Both require an explicit reviewed contract amendment.
A1/A2/A3 remain accepted for recorded runs; A5 run passed, independent review pending.
No new acceptance credit, implementation/run authority, activation or product approval flow here.

## Exact inspected pins

Clean base HEAD: `21a9074fdd514690d183f1956c17f8f23f4cd53b`; below are Git blob IDs.

| Path | Blob |
| --- | --- |
| AGENTS.md | c2ae25947916672b13180778265354907b75e295 |
| docs/KICKOFF.md | 9dc49e726bce3af9144ac867b5f7d2c96f4bca77 |
| docs/briefs/0020-linux-hierarchy-implementation.md | 07ecbf698b210752c12c9c14ab8e8b3a15e74f6a |
| docs/adr/0020-linux-execution-hierarchy-and-janitor.md | fae2048431c77141503d0fec7fbdaa54288d4f52 |
| docs/reviews/m2-stagef-a4a5-janitor-plan.md | f7d595c4d6aec10fd90999c055ceccb08c327f79 |
| docs/reviews/m2-stagef-a5-seeder-plan-muse-recheck.md | 0183bd1d3d57610ff69b9c63e70329091da6e22d |
| docs/reviews/m2-a5-hosted-record.md | 0321505aa63a1ac70b80c08c252ca18d95fe43ee |
| crates/arxd/src/exec/hierarchy/setup.rs | b33bed7cd5d7022f30c039e761c544567304d28c |
| crates/arxd/src/exec/hierarchy/linux.rs | 880d3f2bb47be9660caf84273be5a7cb3710fb89 |
| crates/arxd/src/exec/hierarchy/fake.rs | 411d0dec9d8bc653a63265e117ccc8010ea90c40 |
| crates/arxd/src/exec/hierarchy/a5.rs | 416d33808795e05585b6c2055e192ab9f73afb9f |
| crates/arxd/src/exec/disposable.rs | 5a8bc19dcd5534eb1affd2272eb9e753be04938e |
| crates/arxd/src/service/unix_bridge/readiness.rs | 1ff55b22f810e1907e21a2edc9c9df07be868f92 |
| crates/arxd/src/service/unix_bridge/tests/e6.rs | 7514f353217fe015974228af9d92fa07cb249095 |
| tools/unix-exec-validation/driver.py | 6358e3d262aea5540d9796244192d903786203e5 |
| packaging/linux/arxd.service | 0b337773c60e49fe84de76db018d2235ae076254 |
| packaging/linux/50-arxcovo-standard-rights.conf | 93a3abcce9782cc7fddf4c2c1dcc5fc74ec18a74 |

## Semantics and evidence limits

Proven by source: package uses KillMode=mixed, Restart=on-failure; fixture overrides
Restart=no and RuntimeMaxSec=140, with a 120 s monitor. Neither overrides KillMode.
Official semantics: mixed sends final SIGKILL to remaining unit processes once MainPID
exits; default SendSIGKILL=yes. [systemd.kill](https://www.freedesktop.org/software/systemd/man/latest/systemd.kill.html#KillMode=)
Default ExitType=main treats MainPID exit as service completion; Restart=no suppresses
automatic restart, not teardown. [systemd.service](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html#ExitType=)
Inference: ordinary killable descendants cannot reliably supply A4's live post-crash seed.
This is no installed-version observation; delegated-directory removal remains A9/U9.
Kernel semantics: cgroup.kill recursively SIGKILLs descendants; populated tracks live
tasks; childless, taskless cgroups are removable, including with zombies. Freezing does
not prevent fatal signals or empty-directory removal. [Linux cgroup v2](https://docs.kernel.org/admin-guide/cgroup-v2.html)
Thus frozen/sleeping helpers and held directory FDs are not a sound nonremovability induction.
Source: LeafStructure/KillInterface map to Setup/mechanism_unavailable, not quarantine;
LeafRemoval/KillTimeout/VerifyFailed map to Janitor/quarantined. Do not change that mapping.

## Smallest implementation sequence after independent contract review

1. A4: add one ignored fixture and one fixed test-local helper, reusing A5's module-local
   descriptor seeder and read-only S0–S4 preflight; keep historical A1/A2/A5 assertions.
   Require identical four disposable-host environment gates, real/effective root and
   unchanged Standard unit/drop-in. Exclusively create runs/ and literal arx-0-0;
   never adopt existing objects. Hold trusted no-follow descriptors; reject replacements.
   Test-only seeding enables memory/pids on unit then runs (read back both), sets leaf
   memory.max=64 MiB, memory.swap.max=0, pids.max=1 BEFORE placing the helper there.
   This preparatory controller write is an explicit fixture exception, not production setup.
   Helper executes only fixed waiting code: no commands, forks, network, storage or probes;
   blocked handshake before migration, parent-held process identity, 15 s absolute lifetime.
   Confirm membership, live helper, leaf/runs populated=1, empty runs procs and sole MainPID
   in supervisor. Require >6 s remaining lifetime, then immediately call production setup ONCE.
   Require its unchanged 5 s kill deadline; prove helper died by SIGKILL and reap it before
   its own deadline. Only S7 may kill on the success path; expiry/early death fails acceptance.
   Require one removal, runs reused, both controller writes, fresh S9 and literal leaf absence.
   Reuse exact-summary/READY/identity-ack/owned-cleanup monitor; no restart sequence.
   Keep existing StartupDomain Fake/Manual evidence for Pending through kill/wait/removal/S9,
   zero early runner calls and post-setup admission. Native setup/READY alone proves no MCP run.
   On failure, bounded owned cleanup may kill/reap; uncertain cleanup retains reservation and
   forbids reuse. Keep 4096-byte reads, 512 records, 64 children and 1 MiB capture limits.
2. A6: extend existing Fake/Manual + StartupDomain tests, no native fault harness:
   inject FsError::Busy at the exact validated leaf rmdir; separately delay populated
   beyond 5 s and inject fresh verify residue. Drive actual setup outcomes through readiness,
   executor refusal and persisted quarantined activity; assert zero runner calls, no Ready,
   and no same-domain recovery on subsequent requests. Preserve shutdown's Stopping result.
   Existing h5_removal_busy and t15 dispatch are components, not this joined evidence.
   Mutation checks must catch swallowed rmdir failure, skipped verification and early Ready;
   native A4 must reject an omitted kill. All proposed checks remain unrun here.
   No chmod trick, mount, migration race, uninterruptible task or unsafe survivor workload.

## Exact proposed narrow amendment (ADR D9/Stage F and brief A4/A6)

> A4: On an unchanged packaged Standard disposable unit, production startup setup kills
> one fixture-preseeded, bounded populated leaf, confirms emptiness and removes it before
> execution admission. Native cleanup and injected-port readiness ordering are separate
> evidence. Prior-MainPID SIGKILL provenance is excluded; crash lifecycle and directory
> removal are not established by A4 and A9 remains independently required.
> A6: Deterministic injected-port LeafRemoval, KillTimeout and VerifyFailed outcomes from
> production setup must latch quarantined, refuse execution and persist the refusal.
> This is non-native failure evidence; native nonremovable-leaf induction is not claimed.
> Production cleanup, bounds, readiness protections and restart-only recovery are unchanged.

Disposition: independent architecture/security review must accept this exact reduction in
lifecycle/native-failure coverage before amendment or source work; otherwise A4/A6 stay unmet.
Then use normal different-provider source review and green required checks before any separately
authorized bounded run and acceptance review. No general restart infrastructure is needed.
M2, remaining F/G and real-Runner PA6 stay open; installed/dedicated paths are
**unverified on real hardware**. Only source/docs/Git reads and official web research occurred;
no builds, tests, native processes, services, owner steps, dispatch or GitHub actions ran.
