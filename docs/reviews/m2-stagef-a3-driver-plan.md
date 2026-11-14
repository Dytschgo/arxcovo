# Stage F A3 driver plan

2026-11-14; OpenAI / gpt-6.1-sol / high; design sub-agent; source preparation only.
Basis: AGENTS/KICKOFF, brief 0020 A3, ADR 0020 D8 R4, current driver/launcher,
packaged unit/profile, A1/A2 hold, both ignored A3 tests and F0/F0b regressions.
PR89 source `cd561e0` prepares assertions, not native permission. D7b is consumed.
Historical A1/A2 evidence remains valid only for its recorded run; do not rewrite it.
This plan grants no dispatch, retry, installation, production activation or acceptance.

## Closed selection and future source writes
Keep the existing `TEST` as the **one source-selected plan constant**. Validate it
against exactly `exec::disposable::disposable_linux_a1_a2`,
`exec::disposable::disposable_linux_a3_standard`, and
`exec::disposable::disposable_linux_a3_none` before any effect. Derive profile
presence from that exact value: present for A1/A2 and A3 Standard, absent for A3 None.
No independent profile switch, arbitrary test/unit/drop-in argument, environment
selector, new CLI mode or workflow input. Keep A1/A2 as the preparation default;
its exact test remains selectable and behavior intact. Selecting an A3 case is a
one-line source change and therefore needs its own immutable reviewed source pin.
Each invocation runs **one case** on a fresh disposable host; no in-place profile
toggle, two-case orchestration, reused failed host or extra approval framework.
A later execution disposition can cover both fixed selections together.

Future implementation writes only `tools/unix-exec-validation/{driver.py,
test_driver.py,mutation_check.py}` and `crates/arxd/src/exec/disposable.rs`;
adapt `test_f0_blockers.py` / `test_f0b_transport.py` only where mocks need alignment.
Extract A1/A2's existing READY/private identity-ack hold into a test-local helper;
call it after A1/A2 postconditions and after each A3 assertion. Preserve its
120-second monotonic deadline, no-follow bounded 0600/root/nlink-one ack validation,
absent-ack check and main-PID notification. Do not add setup to the A3 tests.
No production parser/setup/Runner, package, Cargo, workflow, ADR or PROGRESS writes.

## Exact runtime writes and attribution (future execution only)

Shared setup retains existing gate-before-probe/mutation ordering, root check,
systemd >=254, kernel >=5.14, cgroup-v2 memory/pids preflight and bounded scan.
Use `/var/lib/arx-ci-<run>-<attempt>` (root 0700), protected ancestor/copy rules,
source/workflow/reviewed-SHA equality, existing offline locked prebuild and exactly
one protected `source/target/debug/deps/arxd-<hex>` executable. No build in unit.
Keep the existing private artifact/PID/output/ack/progress and feedback files.

Both modes exclusively create `/run/systemd/system/arxd.service` from unchanged
`packaging/linux/arxd.service` bytes and own `arxd.service.d` (0700).
Standard additionally creates `50-arxcovo-standard-rights.conf` from the unchanged
packaged bytes; None never creates it. In both, create the fixed fixture drop-in
`90-disposable-a1a2.conf` using existing `fixture_override`: only ExecStart reset
and protected staged Python launcher, `Restart=no`, `RuntimeMaxSec=140`.
Retain its historical name; introduce no user choice. Retain Type=notify,
NotifyAccess=main, delegation/supervisor, KillMode, umask and start/stop timeouts.
None omits the whole standard profile; never simulate absence with a weakening
drop-in or override NNP/capabilities. Standard keeps **all** profile directives.
Before start, require manager FragmentPath and exact DropInPaths set to match the
owned runtime files for that mode; compare their bytes to the staged package and
fixed fixture override. Repeat attribution while held and before owned cleanup.
Reject extra fragments, aliases or drop-ins, including identical foreign copies.

Reuse `service -> harness -> execve`: Python records its PID, exclusively opens
0600/no-follow capture, dup2s stdout/stderr and execs exact selected ignored libtest
argv with existing minimal disposable/NOTIFY_SOCKET environment. No fork, Cargo,
timeout wrapper or extra unit process. A3 uses the existing bounded production
procfs reader/parser: Standard requires NNP=1 and CapBnd=00000000000000c1;
None requires Hardening::None. R4 remains exact, including subset refusal.
After the assertion, READY/hold permits outer checks of MainPID=recorded PID,
/proc/PID/exe=protected artifact and exact supervisor membership; bounded read of
supervisor/cgroup.procs must contain that PID alone. Never synthesize placement,
user.delegate or runs. Outer ack releases the same live process; require actual
systemd success/zero exit plus exact selected test-ok and one-pass/zero-failure
summary. READY alone, a wrong test, zero tests or an early exit cannot pass.
Keep 1 MiB capture cap, 120-second monitor, 140-second runtime, 360-second prebuild,
5/30-second existing command bounds, 40-second stop and ten-minute job bound.

## Shared reservation, collision and cleanup lifetime

Both modes and A1/A2 share existing `/run/arxcovo-disposable-a1a2.lock` EXCL/flock;
reserve before collision inspection and retain through staging, start, stop,
snapshot, runtime removal/reload and stage cleanup. Reject existing loaded/failed
unit, alias, any search-path fragment/drop-in, /run/arxcovo or fixed cgroup.
Collision owns nothing: never stop/adopt/remove it. Trusted fresh-host containment
excludes competing privileged writers; this is not a hostile-root defense.
Capture every created runtime identity; validate the whole owned set before any
removal. Stop only the proven owned fixed unit, snapshot F0b in existing stop-finally
position, require cgroup absence, remove owned runtime files, reload, safely remove
captured protected stage, then release reservation. Partial install/replacement,
stop failure or uncertain cleanup quarantines and retains reservation/evidence.
No A3 account or legacy fixture cleanup. Preserve F0b call order, phase/first/final,
mutation lower bounds, conditional reads, channels, precedence, closed codes,
1024/4096-byte bounds and single outer `run` feedback line; no mode field/schema.

## Verification and disposition
Future mocked tests must cover each test/profile pairing, invalid selection before
effects, exact file bytes/sets, extra drop-in refusal, sole PID and hold/exit order,
wrong result/timeout/overflow, collision nonownership and replacement quarantine.
Extend existing focused mutation assertions for swapped/omitted profile, wrong TEST,
skipped identity and cleanup fences; preserve historical replay/F0b regressions.
Independent design/source review and green required checks precede coordinator merge;
execution disposition remains separate, with existing default-off workflow fences.
No build, tests, native/service/process/account/owner operations, network or dispatch
ran for this document. A3, remaining F/G, M2 and PA6 remain open;
**unverified on real hardware**. No implementation choices require owner input.
