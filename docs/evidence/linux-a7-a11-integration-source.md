# A7/A11 integration source: stopped on startup-bound conflict

OpenAI / gpt-6.1-sol / high, 2026-11-15. Prepared parent `e544c41`,
actual main baseline `47b440a5faeaa24434e155b81d5d03a5c977e4e9` (PR124).
No integration implementation or native acceptance is claimed.

The task explicitly requires stopping on a contract-bound conflict without
raising timeouts or changing hardening. Static call-graph inspection found:

| Existing source/contract | Consequence |
| --- | --- |
| `packaging/linux/arxd.service:7,13`: `Type=notify`, `TimeoutStartSec=30s` | READY must arrive within the packaged 30-second startup window. |
| `driver.py::fixture_override`: `RuntimeMaxSec=140`, no startup override; `controlled_override` adds only four read-only binds | Neither override extends the startup window. The 120-second monitor is an outer bound, not the manager's startup deadline. |
| `sudo_nnp.rs::disposable_linux_a7_a11`: two accounts, sequential `run_phased` for tool-probe and E4 reentry per account, `RunSpec.timeout=20s` | Four calls have an aggregate configured allowance of 80 seconds, before setup/admission/cleanup overhead. |
| Same controller: `cases == 2` assertion precedes `READY=1`, then `hold_identity_ack()` | The four calls occur before READY; the packaged 30-second bound does not cover their configured allowance. |
| `unix_shim.rs::probe` and `probe_version`: sequential TOOLS loop, five-second version deadline per tool; `unix_shim_rights_linux.rs::DEADLINE=5s` | Nine tools plus rights have a documented 50-second probe budget (ADR0020 Bounds), exceeding even the individual 20-second RunSpec deadline. |

These are conflicting configured budgets, not evidence that every execution
takes 80 seconds or that a fast host cannot pass. Actual installed-tool timing
has not been collected. A successful fast run would not settle the bound mismatch.
Moving READY earlier, replacing the real tool probe, shortening existing probe
contracts or extending the package/Runner deadlines would select a new contract;
none is implemented or recommended as an implicit fix here.

Read AGENTS/KICKOFF, ADR0020 D9b and Bounds, accepted A7/A11 plan, fixed delivery
contract including its four binding corrections, delivery source review,
controlled-policy decision/mapping resolution, package provenance/reviews and
D7h FILE-leg assessment. FILE-leg evidence grants no reusable host authority.
Actual TEST remains the A4 omitted-kill negative; all False/None gates remain.

Verification performed: static reads/searches of the source paths above,
source-only assertions of tool count, deadlines, call/READY ordering and override
contents, and `git diff --check`. No behavior changed; portable suites, mutants,
inventory, fmt, Clippy and Linux compilation were not run for this stop record.
No native/helper/ignored test, LinuxFs, proc/sys/host query, account, sudo, admin,
network, installation, dispatch, variable, push, PR or subagent action ran.
Only this evidence file changes; parent checkpoint documentation is untouched.

Implementation requires a settled disposition of the startup/probe budgets
before reopening this slice. Fresh independent Muse source/exact-head review,
normal gates and a separate fresh-host native grant remain required.
A7/A11, M2, full F/G and real-Runner PA6 remain **unverified on real hardware**.
