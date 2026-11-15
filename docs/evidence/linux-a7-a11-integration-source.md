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


## Authorized bounded integration source (2026-11-15)

OpenAI / gpt-6.1-sol / high, implementation sub-agent, no subagents. The preceding
stop record is retained as historical blocker provenance. The merged binding
budget amendment (PR126, main `a8d5e9fdcaec10b19cf8939915bf10c069f0a912`,
parent checkpoint `29762a3`) resolves that recorded conflict; the coordinator
supplied amendment review PASS (Muse comment6893511474), all five gates and
merged-tree equality to `c00ac78`. These supplied gates were not rerun here.
Source commit: `95a548ab8b7960cbea4ccc3dfa540679124123f1`.

Changed source paths, and no others:

- `crates/arxd/src/exec/disposable/sudo_nnp.rs`
- `tools/unix-exec-validation/driver.py`
- `tools/unix-exec-validation/test_driver.py`
- `tools/unix-exec-validation/mutation_check.py`

One production setup and fixed delivery/view/account admission precede READY.
The fixture checks startup <=15s and uses a local <=5s identity ACK hold before
starting exactly two scoped workers. Each owns one account/Runner domain and
runs the actual production arxd tool-probe (55s RunSpec), then the fixed E4
reentry (10s). Per-call delivery reopening/account/view checks, output4096,
memory64MiB/swap0/pids32, identity/status/records/cleanup oracles remain.
Full synchronous call returns are measured at <=60s/15s, cumulative non-call
case work at <=15s, each entire case at <=90s. Markers are fixed safe records
written outside libtest capture; their write time is charged to the envelopes.

Workers catch panic inside the scope, signal either completion through a channel
and cancel both caller/shutdown domains on failure. The parent receives either
completion before joining; it does not wait on a healthy join while hiding the
other failure. Both explicit joins are collected without short-circuiting, even
when one reports failure. Final join/completion is measured <=10s, including the
completion-marker write. Excess refuses after joining and synchronous cleanup.
No cleanup thread is detached or aborted, and no hard syscall/cleanup/join bound
is claimed. Cooperative cancellation cannot interrupt blocked OS primitives.

The dormant driver branch derives only from the fixed TEST literal. It connects
the single test/bin build and held-fd artifact delivery, controlled policy staging,
handoff, runtime binds and final stop-gated owned cleanup. Partial policy state,
including pending creation with an empty entry list, is checked before deleting
accounts/artifacts or the private stage. Installed package comparison uses only
the existing pure comparison function behind absent profile/payload and false
host/policy admission. No installed adapter is invoked or newly qualified.
There is no host-original-policy query or claim, copied probe, production bridge,
package/helper/workflow/Cargo/ADR/PROGRESS change, or new execution selector.

Driver still starts `--no-block`, then monitors active/runtime/MainPID/executable
and exclusive supervisor placement before ACK. A7-specific startup/ACK checks
are 15s/5s. Terminal success requires ACK, successful systemd result/status,
both case markers, completion marker, the exact successful libtest summary and
observation before the unchanged 120s deadline, followed by owned final cleanup.
Runtime140, packaged startup30, build360 and workflow10min are unchanged.

### Verification actually run

Callgraph audit preceded execution: selected Python suites patch account,
filesystem, process and service effects (the guard CLI/native suites were
excluded). Extracted Rust tests contain scalar/schema/parser functions, fixed
thread scheduling, or the actual case function with all admission/Runner/oracle
boundaries mocked. No actual Runner, LinuxFs, rights probe, ignored fixture,
helper, host proc/sys/policy read or native account path was executed. Source
inventory tests read repository files only. Rust compiler/build commands below
are development tooling, not fixture execution.

- `cargo fmt --all` and final `cargo fmt --all -- --check`: PASS.
- `cargo clippy -p arxd --all-targets --locked --offline -- -D warnings`: PASS
  on Windows; the Linux-only controller is not compiled by this target.
- `cargo test -p arxd --lib --no-run --locked --offline`: PASS, compile only.
- Exact Cargo pure tests (each 1 passed, 0 failed):
  `exec::stage_b_tests::w8_e7_crate_wide_inactive_source_inventory`,
  `exec::stage_b_tests::s13_e4_tokenized_source_and_bypass_controls`,
  `exec::stage_b_tests::w8_e7_test_braces_never_hide_following_production`.
  Each used `cargo test -p arxd --locked --offline --lib <name> -- --exact`.
- Audited `mutation_check.main()`: 91 pure/mock baseline tests PASS and all
  40 existing driver mutations killed by assertions; includes F0/F0b guards.
- `mutation_check.a7_integration_checks()`: five Python regressions PASS;
  five driver mutants killed. Actual extracted Rust
  `a7_a11_scheduling_counterfactuals --exact` PASS (failure/panic from either
  worker, both completions, each envelope and completion gate); seven mutants
  killed. No mutant received credit for a compilation error or timeout.
- `mutation_check.a7_case_mock_checks()`: actual extracted `a7_case` tested by
  `actual_case_sequence --exact` PASS, both account indices, both call errors,
  residue, admission-before-call, probe-before-reentry, exact specs/arguments and
  markers. Five mutations killed by assertions.
- `mutation_check.sudo_nnp_source_checks()` and
  `mutation_check.sudo_nnp_delivery_checks()`: PASS existing mocked suites,
  exact extracted `a7_a11_discriminators`,
  `s14_sudo_nnp_scalar_discriminators`, `a7_a11_record_discriminators`,
  `a7_a11_run_discriminators`, `a7_a11_delivery_schema_rejections`, and all
  associated scalar/record/result/schema/artifact-copy mutants.
- `mutation_check.controlled_policy_mutants()`: PASS mocked controlled suite,
  exact extracted `a7_controlled_policy_discriminators` and all six Python /
  five Rust mutations.
- `mutation_check.rust_mutants()`: seven exact pure setup/A4/A5 tests PASS;
  all eight mutations killed, source fingerprints unchanged.
- `git diff --check`: PASS.

Python entry commands used `python -c` with
`sys.path.insert(0,'tools/unix-exec-validation'); import mutation_check`, then
called the named functions above. Existing and new mutants run only on in-memory
source copies. Initial test development exposed a wrong platform mock, obsolete
text/deadline anchors, a redundant-predicate mutant and insufficient fake clock
sequences; these were corrected and the final relevant checks above passed.

Compile-only limitation: `cargo check -p arxd --tests --target
x86_64-unknown-linux-gnu --locked --offline` FAILED because the existing
Windows toolchain has no `x86_64-linux-gnu-gcc` for libsqlite3-sys. No compiler,
package or dependency was installed; no network fallback or Linux execution
occurred. Full Linux cfg compilation remains required in independent review/CI.

Final driver SHA256:
`6122201890f7080696540948bb4e28168422019deb4ceff26ec32f26b84bcccd`.
Controller SHA256:
`855ebd435cce53a0f65a9d39a180c8db1e1499ac8fbf557191335ccb1591b1b2`.
No affected fixed driver hash constant was found or changed; no manifest, guard,
selection inventory or artifact pin was relaxed.

TEST remains the A4 omitted-kill negative. A7/A11 remains ignored/unselected;
all original None/False delivery/profile/host/policy gates remain closed. D7a-h
remain consumed and the stale A1/A2 owner reply remains closed. No native grant,
actual adapter invocation, root/sudo/helper/account operation, dispatch, variable,
installation, push, PR, merge or watch was performed. Fresh independent Muse
source/exact-head review, required gates and separate reviewed fresh-host native
disposition remain required. A7/A11, M2, full F/G and real-Runner PA6 remain
**unverified on real hardware**.
