# ADR 0024: bound concurrent three-node storage fixtures

Status: accepted for implementation, 2026-11-16; hosted repair evidence pending.

Main01a42d5 Windows CI43788629298 failed T2/U6 leader waits and T3/T4 write waits. Opus reproduced the leader deadline under a shared-flush latency model on that exact revision, and reduced fixture concurrency removed the signature without changing deadlines. The write-wait connection is inferred; hosted cause is supported, not proven.

Nextest assigns only arxd library tests in cluster::consensus::three_node to one max-threads=1 resource group. Other tests keep their existing default scheduling. There are no skipped tests, retries, deadline increases, production/storage changes or CI timeout increases. A Python guard rejects broadening the group or adding bypass directives; actual nextest group mapping and execution provide behavioral evidence.

This trades fixture overlap for less contention among independent real-SQLite clusters sharing one flush device. Every fixture retains its internal three-node concurrency. Windows job and nextest duration must be measured under the unchanged12-minute cap; a timeout returns to design rather than bypassing the gate.

Residual U6 shutdown Orphaned under sufficiently slow applies is not solved by this scheduling repair. It is suite fix attempt3 under delegated oversight; another hosted cluster deadline/Orphaned failure requires escalation rather than a fourth change. Main remains red and the existing green-exact-main merge prerequisite is not waived. A green repair PR alone is not a green main run.

M2 remains incomplete. A7 FINAL NO-GO and consumed A12 no-retry remain unchanged; hardware paths remain **unverified on real hardware**.
