# ADR0036: Windows heavy service test group

Date: 2026-11-19. Status: proposed mitigation (e7 family attempt 1/3).

## Context and evidence

Main 92003177b72a907ddbefb446acbf7f8b0a406374 is red: Windows e7 A8
timed out after 15.248 s while E6 t19, E6 t15 and the executor durable
matrix took 29.194, 59.409 and 86.809 s. PR193 head 04b29b3c's
Windows run 44602605879 failed W1 at its existing five-second observation
bound (7.222 s total); concurrent E6 t15, E7 missing-location matrix and
executor matrix took 22.892, 20.790 and 85.156 s. Passing historical runs
also overlap these fixtures, so overlap alone does not establish causation.

The independent diagnosis on exact main retained these SHA256 artifacts:
- report.md: 170f6a344d45eef3995272628c12162432b2f050e4d9da73919de47fda457326
- concurrency.md: 2816a1978c2813d0ab69976d1e4b8b5827f265b53f8ce12699e8d053fabcf8d6
- run3.log: 1f43ecda6902cf204e86b37fe6e1b4b862b2d1a78bd41dbed45f1f4edfbe8d8d

They are retained locally in D:/Temp/arx-e7-family-diag. Intervals inferred
from completion timestamps and durations are approximate, not CPU/I/O traces.
Two-CPU affinity plus 64 nextest slots produced 261 passes and one e7 SP1
admission-observer timeout (one-second bound; 3.712 s total), in 12.900 s.
A8/W1 passed in 5.800/3.937 s. At 32 slots, all 262 service tests passed.
This demonstrates load sensitivity, not deterministic hosted reproduction.

## Decision

Use repository .config/nextest.toml with nextest 0.9.151 semantics:
windows-heavy-service has max-threads = 2. One default-profile override,
platform = 'cfg(windows)', assigns exactly these anchored test prefixes:
service::unix_bridge::tests::e6::, service::unix_bridge::tests::e7::,
and service::executor::tests::. E7 includes a8 and s1. These cover the
observed slow peers and failed admission observer. No additional service
fixture family has enough overlap evidence to justify inclusion here.

Keep global slot defaults, test inventory, assertions, timeout bounds,
fail-fast and retry defaults. Ubuntu/macOS tests receive no group assignment.
CI's cargo nextest run --workspace --locked discovers this file at workspace
root without flags. The optional diagnostic passes its separate --config-file
tools/ci/windows-diag-sample.nextest.toml explicitly; that file is unchanged.

Pin the exact parsed configuration and reject mutation of the group, limit,
platform, filters or retry policy in the existing static CI unittest module.
This deliberately requires an explicit contract update for future changes.

## Limits, falsifiers and follow-up

This is a contention mitigation, not a proved cause fix. A8/W1 holds already
run in blocking pools; SP1's failing hold already uses a blocking channel.
Thread and storage competition remain plausible without measured CPU/I/O
progress. Falsifiers are the same focal timeout at one/two group concurrency
with timely CPU/I/O progress, or no reduction in a repeatable red stress
control. Three passing samples cannot establish a flake rate or cause removal.

Founder-heavy consensus concurrency is out of scope: those failures occurred
earlier with no service peers. Exhausted founder/engine counters are unchanged.
No broader serialization, retries, quarantine, bound changes or workflow edits.

## CI cost and acceptance

The historical Windows nextest phase is approximately 190-250 s and the job
9-10 min, against the unchanged 15 min gate. Two group slots can serialize
long matrices and increase the tail; reduced contention may also shorten
their execution. Local service and stress measurements are recorded below.
These local proxies do not replace hosted CI or independent exact-head review.
Native service deployment and VM paths are **unverified on real hardware**.

Validation on Windows (Sol 6.1/high, D:/Temp/arx-heavy-group):
- Python static discovery: 133 tests, 3 skipped, pass; fmt and workspace
  all-target Clippy with -D warnings pass.
- nextest 0.9.151 show-config test-groups --workspace --locked: 57 members,
  Windows-only override, max threads 2. A8, S1 and durable matrix present.
- Default scheduling: service subset 262/262 pass, 6.973 s nextest wall
  (7.797 s command wall). Group members consumed 12.229 test-seconds.
- Two logical CPUs (inherited process affinity mask 3), 64 test slots:
  three runs 262/262 pass, nextest walls 10.697/10.530/10.520 s.
  A8: .278/.472/.317 s; W1: .160/.086/.081 s;
  SP1: .084/.056/.133 s. No retries or timeout changes.
  Affinity limits logical CPUs; no physical-topology/host equivalence claim.
  The process mask was restored in finally.
- Group limits reduce the 57 selected fixtures from up to the global slot
  count to two; other service tests remain free to fill global slots.
  This is a config/runtime membership proof, not per-process CPU profiling.
  Compared with the diagnosis's single red 64-slot run, focal durations fell;
  three passing samples do not prove stability or causation.

Deduplicated retained hosted completion lines give 247.569 group test-seconds
in main's failed run (45 completed members, longest 86.809 s; phase 214.671 s)
and 187.594 in PR193 (50 members, longest 85.156 s; phase 199.937 s).
Their fail-fast inventories are incomplete. Half that observed work is
94-124 s. A conservative planning allowance of up to about 125 extra seconds
(no credit for reduced contention or overlap) suggests a 190-375 s phase
and roughly 9-12.1 min job from the historical 9-10 min job. This is an
estimate, not an upper bound: missing members and runner noise remain.
The existing main workflow also has a 12 min job timeout, so the upper
planning range is tight even though below the 15 min policy gate. Hosted
CI must confirm both constraints; do not increase either limit here.
