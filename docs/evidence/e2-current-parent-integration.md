# E2 current-parent integration evidence — 2026-11-12

Historical disposition at 000835d: **HELD**. Local preparation only; independent Opus assessment of
this integrated head and unresolved failure evidence, coordinator publication
disposition and required hosted gates remain outstanding. A green integration
run is not a bug fix or a waiver. M2 incomplete; native/installed/dedicated paths
remain **unverified on real hardware**.

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent; no delegation.

## Exact boundaries and scope

- Supplied isolated-worktree base: `fbb1813fd0455f7208e84557ec18c8e9d1bc298e`.
- Original verification main inherited after PR43: `b3e1d13888ae5967b095d4151144aa240169ed77`.
- Tested implementation head: `2986cad1fe425169b09917fcf6eab70292e915ce`. The subsequent evidence commit changes
  documentation only; the final delivery SHA is returned to the coordinator.
- Original E1 source base: `4a5919e61542ee38507a9ba461549a61e4b3a67d`.
- Reviewed E2/report head: `1103a660222849f9a6d4ac9d4b2cdf9491703ca4`.
- Prior current-parent port: `780ca4164494f6c3da00a340f4c4c67e3ac9d305`.

Direct byte comparisons using `git show REF:path` prove all six final crate
files equal both 1103a66 and 780ca41. The four existing files also matched E1
base4a5919e before the port. Original reviewed and final E2 source deltas are
identical. **No mechanical source divergence.** The entire remaining crate tree,
Cargo/dependencies, executor, application, native, local fixtures/store, client,
CI and tools remain identical to the supplied base. E1/N3/N2/client/artifact CI
are preserved. PR44 SYSTEM refusal and state-exit candidate8e2a823 are excluded.

Git blob manifest (identical at reviewed1103a66, prior780ca41 and tested head):

| Source file | Git blob SHA-1 |
| --- | --- |
| `crates/arxd/src/service/mod.rs` | `afbecd8ceeb3a4a83657add8023b9c16ba716a6a` |
| `crates/arxd/src/service/owner_install_windows.rs` | `20b149c31877a7be2f48789698a7b0903ccaa19f` |
| `crates/arxd/src/service/owner_install_windows_tests.rs` | `6334aac5b9d86be697ca32d2392b08820809ebb3` |
| `crates/arxd/src/service/owner_scm.rs` | `ef89261450f00dd38850e9117a6794711be2f4c6` |
| `crates/arxd/src/service/owner_windows.rs` | `c467ed1f414931e2f986a52dd687899c164d2f85` |
| `crates/arxd/src/service/owner_windows_tests.rs` | `d7a1b7d941817cca738b85200b22381f6b2e0324` |

OwnerGuard fields and Local pointer stay private, with only the reviewed narrow
borrow/consume methods. Source fence and binary/observe/pin/restore/create/lock/
classify/register ordering are unchanged; registration never starts the service.
No CLI wiring, production install invocation or activation was added.

## Retained credit and scoped documentation

The full independent Opus high security/spec/standards review was committed at
`42718df0005e89c184c75368b4ec8bf2087ad1f1`, reviewing d28cbd7 against E1.
It required SP-B1/ST-1 corrections. The independent Opus authority recheck of
3ad3c80 against42718df was retained at1103a66 and passes those corrections.
Both reports, the correction report, junction report/patch and original E2
implementation evidence were copied byte-for-byte from1103a66; they also equal
780ca41. Their run IDs and mutations belong to their original heads, not this
integration. Carried advisories and real-hardware limitations remain open.
Architecture credit stays with the prior Sol independent review of Opus design;
Opus conformance review is not independent review of its own design.

ADR0016 and brief receive scoped status additions; current E1 history is retained.
The installation guide receives only the reviewed Windows step4 delta. The
foundation guide receives the reviewed E2 section with current hold wording,
appended after current content. The old foundation hunk did not apply at its
historical E1 context; only its E2 addition was extracted. No old document tree
or PROGRESS overwrite. Existing history, last merge, provider rows and the
single queue priority remain. No new design or ADR deviation.

## Checks actually run

All commands used the tested source head above. No instrumentation, environment
concurrency override, timeout change, ignored-fixture invocation, mutation or
full-suite retry was used.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | PASS |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | PASS, 32.26s |
| `cargo nextest run --locked --workspace --all-targets --no-fail-fast -E 'test(service::owner_windows::) \| test(service::owner_install_windows::) \| test(service::owner_scm::)'` | 54 passed, 207 filtered/skipped; 0.447s; run b7e3c2ba-480e-4ff0-ad41-112396511a15 |
| `cargo nextest run --workspace --locked --no-fail-fast` | **259 passed, 0 failed, 2 skipped**, 9.908s; run d78206ad-1919-4183-8614-0098d379edd2 |
| `git diff --check`, changed-file strict UTF-8/no-BOM, exact source comparisons | PASS |

Exactly **one** normal full workspace invocation, nextest default profile and
concurrency. The two subprocess-only ignored fixtures ran through their parents,
never directly. Focused tests use owned-temp namespaces and fake SCM/clock/restore
callbacks. The existing junction fixture retains the split System32/cmd.exe path
and operates only in owned temp. Guide commands were not executed.

Local complete runner logs are under ignored `target/e2-integration-evidence/`
(`focused.log`, `full.log`); committed evidence contains bounded summary only.

## Unresolved red and remaining gates

Original uninstrumented run `bdbf032b-658a-4123-a279-303cb40566ed` at780ca41:
116 passed,1 failed,132 not run,2 skipped. Overflow subcase in
`development::windows::tests::results_count_dropped_output_and_report_nonzero_exit`,
windows.rs956:67, returned ExecutionFailed (output_bytes100). Failing phase,
Win32 error and cause remain **UNKNOWN**. The source file is unchanged here,
including relative to1103a66. The three prior instrumented249-pass runs did not
capture recurrence and are neither fix nor waiver. See
[retained observations](../reviews/development-execution-failure-observations.md).
The diagnostic patch was not applied. Debugging remains phase1; no reliable red
loop, hypothesis, production fix or regression repair is claimed. The current
single green run supplies the requested new-parent signal only.

Fresh independent Opus assessment must evaluate this integrated source boundary
and unresolved evidence before coordinator publication disposition. No GitHub
write, push, PR, status, watch, dispatch, merge or settings action occurred.
No owner admin, production OwnerGuard/install/SCM/LSA/account/privilege/service,
VM/S4U/native experiment, old Lab Manager/state, private-state JSON or babysitter
action was performed. No source mutation evidence is newly claimed.

## Complete delivery file manifest

- `crates/arxd/src/service/mod.rs`
- `crates/arxd/src/service/owner_install_windows.rs`
- `crates/arxd/src/service/owner_install_windows_tests.rs`
- `crates/arxd/src/service/owner_scm.rs`
- `crates/arxd/src/service/owner_windows.rs`
- `crates/arxd/src/service/owner_windows_tests.rs`
- `docs/PROGRESS.md`
- `docs/adr/0016-windows-install-provisioning-and-owner-entry-points.md`
- `docs/briefs/windows-install-provisioning.md`
- `docs/evidence/e2-current-parent-integration.md`
- `docs/reviews/0016-e2-authority-correction-recheck.md`
- `docs/reviews/0016-e2-authority-correction.md`
- `docs/reviews/0016-e2-independent-security-review.md`
- `docs/reviews/0016-e2-junction-debug.md`
- `docs/reviews/0016-e2-junction-fixture.patch`
- `docs/reviews/0016-e2-local-implementation-evidence.md`
- `docs/reviews/development-execution-failure-observations.md`
- `docs/service-foundation.md`
- `docs/service-install.md`

Current publication disposition and parent are in [the refresh evidence](e2-publication-refresh.md).
