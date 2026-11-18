# Main founder F6/F7/F8: bounded independent disposition

Reviewer: OpenAI / Sol 6.1 / high, 2026-11-18; no children.
Scope: diagnosis only, one authorized local control, no source or product change.
Read AGENTS, KICKOFF, dytschgo-debug (including active profile), the founder
[brief](../briefs/m3-next-runtime-boundary.md), its contract/review/evidence,
and [ADR0024](../adr/0024-windows-three-node-test-quarantine.md).
The task's one-control and stop rules override the debug skill's repeated loops.

## Exact identity and retained failure

- Assigned HEAD `f09e495174cd7b5ea79039530b3e2651de4cb3e6` differs from failed
  main `09134085dce93742d8812821dbe11834dcee7a9c` only in `docs/PROGRESS.md`.
  Working tree was clean before testing. Product, tests, lock, toolchain, CI and
  helpers are unmodified.
- `found.rs` blob `6ffd0edd253fcd1dd083019eac67d5a9106890d4`;
  `found_tests.rs` blob `c2dc569f7ed761cc42f949cec8cb8115d3da13c7`;
  `consensus.rs` blob `aba32c1bcd226f9f45a3b133ebaa48eef3f7239c`;
  `issue.rs` blob `e11d4f6a84d6b56684548c1b1f95538d94134979`;
  Cargo.lock blob `6bb1f1c0840176b4411aa7b88a469c448f34d925`.
- Supplied first-attempt main CI44198128467 failed Windows; Ubuntu/macOS passed.
  Inspected raw log `D:/Temp/arx-pr175-main/failed.log` (48,362 bytes), SHA256
  `50706e4fd294eaf4552c0f8d4165b7a93895f1a9db5a2f52cecca2b2865319ff`.
  Identical retained copy: `D:/Temp/arx-founder-f09e4951-control/hosted-failed.log`.
- Prior green main `10aa891f27906e0c73e48396c5c812cab7d4523a`, CI44177880021,
  632 s wall / 629 s charged, is coordinator-supplied history, not a new run.
  Git comparison confirms PR175 changed four documentation paths only.
  Equal source across green/red observations does not establish a cause.

## Signal inspected before hypotheses

| Required Windows test | Hosted signal | Runner duration |
| --- | --- | --- |
| F6 `f6_adapters` | found_tests.rs:319:32, issue unwrap: `Unavailable` | 6.591 s |
| F7 `f7_clock_guard_order` | :434:9, expected `Err(FoundError::ClockRollback)` | 6.918 s |
| F8 `f8_store_privacy` | :449:32, issue unwrap: `Unavailable` | 8.349 s |

F6 and F8 fail at initial issuance, before their adapter and privacy assertions.
F7's assertion does not print its actual result: do not label it `Unavailable`.
The log reports cancellation after F6 with three tests already running; F7/F8
then fail and F9 passes. Summary: 38 passed, 3 failed, 596/637 not run due to
failure. This is a test red, not an infrastructure-retry diagnosis.

All three create one fresh single-voter actual OpenRaft node with private temp
storage and mem_net. F7 has two sequential scenarios inside its one test.
They are outside the three-node quarantine; ADR0024 explicitly requires all
13 founder tests on Windows. No new ignore or quarantine classification follows.

## Feedback command and actual local outcome

Private evidence root: `D:/Temp/arx-founder-f09e4951-control/`.
No cargo/rustc/nextest/arxd process existed at admission. Copied cached registry
to private `cargo-home/` after checking it for reparse points (none); private
`target/` was new. No shared build target, running process or writable Cargo
cache was used. Toolchain binaries were reused; no install or network request.
`CARGO_HOME=<root>/cargo-home`, `CARGO_TARGET_DIR=<root>/target`,
`CARGO_NET_OFFLINE=true`; no profile override. Cargo/rustc 1.95.0.

Executed exactly once, with the expression below substituted for `<filter>`:

```text
cargo nextest run -p arxd --lib --locked --offline -E <filter> --test-threads 1 --retries 0 --fail-fast
test(=cluster::consensus::issue::found::found_tests::f6_adapters) | test(=cluster::consensus::issue::found::found_tests::f7_clock_guard_order) | test(=cluster::consensus::issue::found::found_tests::f8_store_privacy)
```

Actual: exit 0; one compilation, reported 1m12s, default unoptimized/debuginfo.
Command wall 77.009 s, 2026-11-17 23:30:49.1807307–23:32:06.2342483 UTC
(cache-copy preparation excluded). Run ID `0685ea8c-80d2-40af-bb58-b71e4f1dd92c`:
F6 PASS 0.115 s; F7 PASS 0.176 s; F8 PASS 0.113 s. Summary 3 passed,
630 skipped, 0.405 s. No case repeated; no broad suite or mutation ran.
Sequential scheduling applies between tests; each unchanged test still has
its original four-worker Tokio runtime.

This command exercises the exact assertions and can expose their red signal,
but did **not reproduce** the failure. A deterministic symptom-specific loop
or pinned reproduction rate is not established. Debug phases 2–5 (minimization,
hypotheses, instrumentation, fixes) are not reached; repeat-loop advice is
inapplicable under the one-control grant. All runtime work stopped after green.

Retained SHA256 pins (filenames relative to the evidence root):

- `control.log`: `2ef2c7f3e8e48a86b7cd9725ae01fc425ec9e146ec801c8572997738f7dec060`
- `result.json`: `c43c5f7a6cf89c000a959144e3328b472540e72fa1c49fe623ffb2a8b3b99f86`
- `cleanup.json`: `e2d489c9cd2b43f75c13d17a4454838f29cf81a543de82bbb63d96e7e2af573f`
- `source-pins.txt`: `fb814fe76dbd2cdd570e024094cec6b36baa4577681ff42fdbde22ea53a96fce`
- `base-diff.txt`: `0613b893808b3f3c201ac2bbcccf2328917b1f409f51605fc52481800b3d87ad`
- `prior-green-diff.txt`: `ceb12e0b6b809a2f920a294becb5c16058f0fd77844c7b3dc0b207b73cc1f777`

At 23:32:18 UTC, process inspection found no cargo, rustc, cargo-nextest or
arxd process. No kill was needed. Test runtimes ended with their processes;
private caches, target and logs are retained without deletion. No hosted query
was made. Historical founder mutation/cluster passes were read, not rerun or
credited to this source.

## Standards, Spec and Security implications

Standards: retain the first red and the exact source pins; a local green cannot
clear failed main, qualify hosted timing, or reset an exhausted attempt budget.
No independent publication approval or green required-check claim is made here.

Spec: the required founder assertions and unchanged 20 s wrappers remain binding.
Static inspection finds a 5 s absolute issuance budget and several distinct
paths mapping to `Unavailable` (preflight, initialize/leader wait, proposal,
fresh guards, final elapsed check). Whole-test durations include setup and
cleanup, so the hosted durations cannot identify which path returned it.
Clock-order and privacy coverage remain required; the retained red does not
show a clock violation or a persisted secret.

Security: preserve both fresh authorization guards, private sealed tickets,
clock refusal and fail-closed late-certificate discard. Never increase budgets
or suppress assertions based on this evidence. Any future observation must log
only monotonic timings, phase names and error variants, never CSR/DER, keys,
CA markers, join material, store contents, paths containing private data, or
raw pointers. No host service, VM, owner-admin or old Lab Manager action occurred.

## Bounded next action

Coordinator should seek a separately bounded observation grant, with the
following smallest experiment preregistered **as a future plan, not executed**:

1. One F6 invocation only, sequential, same source baseline/toolchain/isolation,
   existing assertions and budgets, zero retries; no F7/F8 or suite repetition.
2. Before using any temporary test-only probe, submit its exact diff for review
   and next authorization. Probe monotonic entry/exit times and result variants
   for preflight, initialize, leader wait, proposal, guard 1/2 and final deadline;
   record remaining budget and whether a phase was still pending at refusal.
   Prefer a bounded in-memory event buffer, emitted once, to synchronous logging
   in timed work. Never change helpers, admission or runtime scheduling.
3. No injected delay in this first observation: observe natural phase latency.
   Expected useful red is the same F6 initial-issue `Unavailable`; classify a
   different failure separately and stop. A pass is another observation only.
   On any red, stop immediately and preserve source/probe/log hashes and cleanup.
   No rerun, tuning or follow-on injection; report the phase evidence first.

Only a reproduced exact symptom could support later ranked causal hypotheses.
Even phase timing would localize a refusal, not by itself prove a hosted cause.
Any controlled-latency treatment would need a subsequent reviewed grant and
fixed seam/magnitude/control/stop rules; none is selected or executed here.

Sol shared Windows fixes remain **3/3 exhausted**, historical Opus **0/2 unchanged**.
No reset, fourth fix, quarantine expansion, timeout/retry/settings/helper change,
hosted rerun or source probe is authorized by this report. PR176 remains held;
no feature push until fresh green main. M2 open, M3 unchecked; multi-host,
transport and VM paths remain **unverified on real hardware**. PROGRESS is
coordinator-owned and was not edited. Only this report is committed; no push,
PR, status publication, watch, merge, protection change or cache deletion.
