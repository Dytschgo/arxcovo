# A7/A11 fixture phase and budget correction proposal

2026-11-15, OpenAI / gpt-6.1-sol / high. Documentation only; correction review
and normal gates/merge pending. Retain the accepted
[Muse review](m2-a7-a11-integration-budget-muse-review.md) (`ffa16f5`, retained
`e99b1c2`) except its two blocking pins, resolved below. The
[source blocker](../evidence/linux-a7-a11-integration-source.md) describes current
source; no implementation has changed.

1. Complete fixed handoff/package/profile/view/account admission and one
   production setup before READY. Keep packaged `TimeoutStartSec=30s` and every
   hardening byte unchanged. READY means setup readiness, not acceptance.
   Send READY, then complete MainPID/exe/supervisor identity-ack before ANY
   Runner child. Start exactly two scoped workers, one owned account/Runner
   domain each; each runs production arxd tool-probe then fixed E4 reentry.
   Retain per-call artifact/view/account checks; no generic worker framework.
2. **Exact start pin:** the driver calls `systemctl start --no-block`, then
   starts the 120s monitor poll. On observing `active`, it verifies runtime
   unit/drop-in bytes, checks MainPID, executable and exclusive supervisor
   cgroup membership, then writes `identity-ack`. Only afterward may workers
   run; only afterward may terminal `inactive`/`failed` with `Result=success`,
   `ExecMainStatus=0`, both complete case oracles, fixed completion marker and
   exact successful libtest summary count as success. READY or ACK alone earns
   no acceptance. This is not a blocking start returning READY. See
   [driver.py:1282-1349](../../tools/unix-exec-validation/driver.py#L1282),
   [runtime validation:1455-1470](../../tools/unix-exec-validation/driver.py#L1455)
   and [start:1571-1572](../../tools/unix-exec-validation/driver.py#L1571).
3. Retain fixture-only RunSpec **55s probe / 10s reentry**, output4096,
   memory64MiB/swap0/pids32. Actual probe: nine sequential tools with 5s version
   loops plus the 5s rights deadline, nominal **50s** plus 5s probe headroom;
   reentry has its 5s rights deadline plus 5s headroom. These are configured
   loop budgets, not hard spawn/wait/syscall bounds:
   [TOOLS:20-30](../../crates/arxd/src/development/mod.rs#L20),
   [probe/child guard:207-284](../../crates/arxd/src/unix_shim.rs#L207),
   [rights deadline:20](../../crates/arxd/src/unix_shim_rights_linux.rs#L20).
4. **Exact cleanup pin:** allocate **10s per case, 5s per Runner call**, as a
   new fixture acceptance allowance, not an existing native cleanup bound.
   `run_phased` returns synchronously through `unix::run`
   ([mod.rs:200-226](../../crates/arxd/src/exec/mod.rs#L200)). On the normal
   Linux path it calls `kill_tree`, whose cgroup confirmation loop has a 2s
   deadline, then a separate 2s child-reap loop; it drains output and drops
   guards before returning
   ([unix.rs:422-449](../../crates/arxd/src/exec/unix.rs#L422),
   [guards:481-504](../../crates/arxd/src/exec/unix.rs#L481),
   [cgroup.rs:124-163](../../crates/arxd/src/exec/cgroup.rs#L124)). Successful
   group removal makes Group::drop a no-op confirmation. Each 5s allowance
   reserves 2+2 seconds of polling and 1s of acceptance margin. Failed removal
   can cause another 2s confirmation attempt in Group::drop; early errors and
   unwinds still run guards. Those paths refuse even if a later drop succeeds;
   they are not promised to finish within 5s.

   Future fixture enforcement must measure `Instant` elapsed around each
   entire `run_phased` call, including returned errors and native guard drops.
   Require elapsed <= **60s probe / 15s reentry** (RunSpec+5s); also require
   case total <= **90s** and cumulative admission/check/oracle work <=15s.
   Full-call measurement conservatively charges pre-timer work and cleanup
   against the envelope; it cannot separately measure cleanup without Runner
   instrumentation and does not prove cleanup itself took <=5s. A quick child
   may leave unused RunSpec time for cleanup. This is an elapsed/refusal
   contract, not a hard cleanup interrupt. Preserve cleanup confirmation and
   monotone residue checks; any error, excess or uncertainty refuses. No
   production Runner change is authorized.

   Accepted-case arithmetic is **15 admission/checks + 55 probe + 10 reentry
   + 10 cleanup allowance = 90s**, with 2s aggregate cleanup margin already
   inside that 10. Three property queries have individual 5s polling
   deadlines; local work and deadline overshoot must fit the measured 15s,
   not silently add time. Their final child.wait() is synchronous and not
   hard-bounded: [query:542-613](../../crates/arxd/src/exec/disposable/sudo_nnp.rs#L542),
   [rechecks:799-820](../../crates/arxd/src/exec/disposable/sudo_nnp.rs#L799).
   Parallel case envelopes do not sum.
5. The unchanged **120s monitor includes startup** after `--no-block`.
   Reserve at most **15s** for pre-READY setup/startup and polling overhead
   outside these envelopes: **5s** READY-to-ACK, **90s** concurrent cases,
   **10s** final cancellation/join/completion. 15+5+90+10=120; terminal
   observation must precede the monitor deadline. Measure/refuse excess at
   phase boundaries; the A7/A11 ack wait must use the 5s fixture envelope.
   Current shared [hold_identity_ack:423-454](../../crates/arxd/src/exec/disposable.rs#L423)
   waits up to120s and is not an existing 5s bound. Packaged30s startup stays
   unchanged; a slower startup may be valid for the package yet refuse fixture
   acceptance. RuntimeMaxSec140, build360s and workflow10min stay unchanged.
   No other fixture inherits these values.
6. Parent retains caller/shutdown tokens for BOTH workers. On either failure,
   panic or elapsed excess, cancel both domains immediately and join BOTH,
   collecting both join results even if the first panicked. Catch worker panic
   within scoped work so cancellation precedes scope's implicit join; do not
   block on a healthy worker before observing the other's failure. A measured
   join >10s refuses after joining; never time out by detaching cleanup,
   forcibly abort a run thread, skip a guard or print success before both joins.
   Synchronous OS calls cannot be interrupted by elapsed checks: cancellation
   is cooperative and a blocked syscall can exceed any envelope. No bounded
   in-process join or native completion guarantee is claimed. Outer monitor/
   runtime fences refuse/stop through the existing driver; final unit stop,
   cgroup/UID and owned-only cleanup, reservation/quarantine on uncertainty
   remain required. See [Runner shutdown:186-193](../../crates/arxd/src/exec/mod.rs#L186)
   and [run timer/poll:343-420](../../crates/arxd/src/exec/unix.rs#L343).

The [binding delivery amendment](m2-a7-a11-delivery-contract.md#binding-budget-amendment)
is the publication surface. Source ceiling remains the ignored controller,
fixed driver strict both-case oracle/mapping mocks and pure scheduling/cancellation
counterfactuals. Tests must reject missing/one case, early READY/ACK, budget excess,
failure/panic with either join missing, and false cleanup/residue evidence. No
copied/shortened probe, package change, generic framework or product concurrency.

D9b/source gating stays: no implementation before this amendment's independent
review, required gates and merge; no execution without a separate native grant
pinning both cases/source/artifacts/workflow/profile/bounds and a fresh host.
D7a-h remain consumed. The stale A1/A2 owner reply is closed. A7/A11, M2, full
F/G and real-Runner PA6 remain **unverified on real hardware**.
