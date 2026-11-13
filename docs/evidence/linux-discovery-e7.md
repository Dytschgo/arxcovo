# ADR 0020 Slice E7: inactive startup discovery and memory cache

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13, Windows x86_64-pc-windows-msvc.
Local implementation sub-agent; no subagents, publication or activation.

## Isolation and dependencies

Verified cwd and Git top level both equal the isolated T3 worktree ending
`t3-a01ab304-7f4eec60f0def16f`, outside the canonical checkout. Initial HEAD
`338d12ce64296b505d560595297f611be82be5c7`; initial status clean. Main pin
`f4fb126826ea0aafe3bc28ee4a7c3c1cc2edb730` is the declared dependency base.
Read AGENTS, KICKOFF, ADR 0020 and its E6/E7 implementation brief, ADR 0017,
and retained E6 independent security review. Read PR63's exact comment
[6865132901](https://github.com/Dytschgo/arxcovo/pull/63#issuecomment-6865132901)
using `gh api repos/Dytschgo/arxcovo/issues/comments/6865132901`: independent
Opus 5.5 high PASS at this exact prepared head. That comment credits source
identity and review only; its hosted status is historical. No new hosted pass
or E6 merge is claimed here.

## Implementation and evidence map

- Test-only StartupDomain consumes one runner factory, allocates its own
  readiness, and retains the E6 AdmissionDomain. Its setup closure is consumed
  once. Handles clone the same Arc<Execution>, including slots and readiness.
  The same Location supplies the bridge parent and setup verification.
- Activity allocates the ticket counter alongside its memory-only cache before
  startup. Tickets fail closed on overflow. The entire cache value is replaced
  only for a strictly newer ticket and a succeeded dispatch response. Neither
  startup nor agent discovery reads Evidence to populate it.
- The test-only application sequence tracks setup, row insertion, dispatch and
  cache update. Drain awaits execution, then the sequence, then Blocking.
  Native has no discovery override; non-test start_discovery spawns nothing.
- W1/W4/T20b checks exact setup/S9/Ready/row/port ordering, one factory call,
  daemon/arxd row and agent invisibility, cache success, cloned handles and a
  shared terminal containment latch. W2/W3 checks hardening and nonstandard
  refusal, janitor-wait shutdown with a manual clock, S9/S10, Ready/row,
  row/dispatch, the exact E6 executor barrier and already-admitted cancellation.
- W5 dispatches K/M/G/refusal reports and preserves the old cache; it tests
  strict greater-than tickets, whole-value replacement and two overlapping
  successful discoveries finishing in reverse start order. W6/W7 uses the MCP
  SDK in memory to discover and list the cache, compares identity to response
  metadata, and checks private rights-line markers are absent from the response.
  The no-cache rendering block is pinned to d44035d in the golden fixture
  (LF-normalized source, identical rendering statements).
- W8 pins Native/configured/capability inertness, the cfg(test) startup body,
  private single-domain construction and clone-only handles. These are lexical
  source regression controls, not semantic proofs. An explicit rg inventory of
  Runner::new calls found only existing cfg(test) modules: exec/tests.rs,
  stage_b_tests.rs, residue_tests.rs and disposable.rs. No production Runner
  construction was added. Hierarchy calls added here are test-only; the
  pre-existing pure parse imports in the bridge remain unchanged.
- Historical W9 at cea3038 used a test-local ten-second timeout and Store
  fallback. Review4728c3d established that this did not test the production
  timeout or exit mapping. The correction below replaces it with a short
  liveness/ownership check; production bounds use the existing paused test.

## Failure history

Initial three startup tests passed. Expanding tests exposed a compile-only
assertion mistake (Failure has no Debug); corrected with fixed Option assertions,
without changing Failure. The initial overlap fixture had a one-slot limit,
so the second dispatch honestly refused; changed the fixture to two slots.
Its attempted false-to-true marker substitution was a no-op because all tool
rows were already true; corrected to distinct validated version strings.
The golden source fixture initially preserved CRLF while rustfmt wrote LF;
normalized the fixture to LF. None of these failed checks was called a pass.

The first mutation run killed dispatch-before-Ready but did not prove the
row-on-setup-failure control: immediate drain masked the mutation with shutdown.
Added a bounded wait for sequence completion before the zero-row assertion.
That control must now fail on the incorrectly inserted row. The mutation script
restores original bytes in finally and requires both successful compilation and
the named assertion test failing; compile failures are never kills.

The first normal workspace run failed the unchanged E6 ownership source pin:
its handle-only tail scan included the newly appended E7 factory. Result:
313 passed, 1 failed, 3 skipped; fail-fast left 69 unrun. Moved the E7 factory
before the E6 domain, preserving E6 source and its test byte-for-byte. This is
an E7 placement correction, not a weakened pin. The final normal workspace
run below is after this settled correction.

The identity mutation uses a fixed unredacted-evidence marker in place of the
response identity. Actual Evidence is inaccessible at the cache update boundary;
this is a structural substitute, credited only for killing a response-identity
mismatch, not as an experiment extracting a real private identity.

## Limits and handoff

No PROGRESS edit (coordinator owns it), CI/Cargo/policy change, GitHub push, PR,
watch, merge, settings change, toolchain installation or WSL operation. No
NativeRunner, owner/session operation, account creation, service installation,
/proc or /sys observation, cgroup/root write, VM, private owner state,
experiment or ignored native fixture. Only existing purpose-built ordinary
full-suite fixtures may run in the final normal workspace suite.

Production remains MechanismUnavailable. M2 is incomplete; native, installed
and dedicated paths are **unverified on real hardware**. The historical
dispatcher privacy flake 65762554, E_PROTECTION and ExecutionFailed remain
unknown; no diagnosis, fix or experiment retry is claimed. E6 must merge and
this stack must be re-pinned and reviewed before publication. Fresh independent
Opus high architecture/security review and normal hosted gates remain required.

## Exact source pin and final checks

Source commit: `cea3038b4db55ea450d4d64168d32708e40784f1`.
All final checks below ran on these settled source bytes before the source
commit; the evidence-only follow-up changes no source. Git blob SHA-1 values:

| Path | Blob |
| --- | --- |
| crates/arxd/src/exec/hierarchy.rs | e47ced34b2f516a59bfc662949040d5d65e6d2a6 |
| crates/arxd/src/exec/hierarchy/fake.rs | a45304aa60ca3cc970653d00feca39cf3da4169d |
| crates/arxd/src/mcp.rs | 02257943dc351bf3988383ffa549e21dbb48cad9 |
| crates/arxd/src/service/application.rs | 6580faffb0d3f3917181184bcc15058bb38b811e |
| crates/arxd/src/service/executor.rs | c5f0d96a1d7dafe868a506d15ea8c4d83e263b49 |
| crates/arxd/src/service/unix_bridge.rs | 55a197afeb5bfd7ec404aa7f24b85f41addc444e |
| crates/arxd/src/service/unix_bridge/tests.rs | e0531b18d4142f7826e09fca5c5c321caa63d09f |
| crates/arxd/src/service/unix_bridge/tests/e7-no-cache-golden.txt | 1263f1c1bd5752c6ba981d928e4bfd36eb14919c |
| crates/arxd/src/service/unix_bridge/tests/e7.rs | a78cbade68cb334c80267f2bc13b1ff239326ab1 |
| crates/arxd/src/store.rs | 51b90a302cc9429627495e65d78ccef63dd50841 |
| scripts/test_discovery_e7_mutations.py | 14595d0c9cc2d6d7a0c86c5233fc95e601674dcd |

Commands and results, Windows only:

- `cargo test -p arxd --lib --locked service::unix_bridge::tests::e7`:
  8 passed at the pre-placement checkpoint. The final focused command below
  reran all eight after the placement correction.
- `cargo test -p arxd --lib --locked -- service::unix_bridge::tests exec::stage_b_tests::p_scan`:
  35 passed, 1 ignored (child-only fixture), 311 filtered. The ordinary t10
  parent ran its purpose-built child normally. The E6 source pin was green.
- `python scripts/test_discovery_e7_mutations.py`: final settled source,
  7/7 compiled assertion kills, followed by EXACT RESTORE. Named controls:
  dispatch_before_ready, row_on_setup_failure, shutdown_after_ready_ignored,
  cache_row_k, completion_order_overwrite, identity_metadata_bypassed,
  production_start_discovery_spawn. Each invokes its named cargo test; see
  the script for exact substitutions and filters. The successful final run
  occurred after the E7 factory placement correction. Earlier successful
  mutation runs are not substituted for this final run.
- `cargo fmt --all -- --check`: exit 0 after final mutation restoration.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0
  after final mutation restoration (3.12 s).
- `cargo nextest run --workspace --all-targets --locked`: final normal run,
  **383 passed, 3 skipped**, 14.544 s test time. Exit 0. Fixed local capture:
  `target/e7-workspace-final.log` (untracked/ignored). The earlier failed
  workspace capture is `target/e7-workspace.log`; it is not green evidence.
- `git diff --check`: exit 0. E6 tests, readiness.rs, setup.rs and
  docs/PROGRESS.md have no diff. No historical privacy-flake recurrence was
  observed in the final run; that does not diagnose or resolve it.

Outcome: local E7 implementation and evidence ready for independent review.
No architectural deviation from the accepted inactive contract is claimed.
The layout is passed into the private wiring factory, so non-Linux standard
and Linux admin configurations are refused before setup/Ready; a failed
runner factory also prevents Ready. Readiness cannot be injected from another
StartupDomain. There is no replacement/reset/rebind/second-runner method.

## First review correction: SB1/PB1

Opus high review4728c3d (retained d6e3f37) required shorter W9 and crate-wide
source inventory. Sol medium correction `09cc061cc5933c0bd8fd8433868bc73fccaa0c24` changes W9 to a100ms
timeout asserted as a timeout only, retaining started-row, no-interruption,
late completion/join and cache assertions. Production10s/Store mapping is
covered by existing paused-time stuck_drain_is_bounded_and_never_claims_stop_audit,
which was run. W9 now completes around0.16-0.20s. No production drain change.

The source-inventory test walks every src/**/*.rs without links/reparse
points, bounds files/bytes, verifies the declarations for reviewed test-only
files, strips explicit cfg(test) items lexically and allows the two existing
non-test pure imports. The cfg(test) locate and E6 test SetupFailure imports
complete the four reviewed pure imports. It forbids remaining Runner::new,
hierarchy::, start_daemon_discovery and both wiring constructor calls in
non-test contexts. Positive and negative controls include production after
a test module. It is a lexical regression fence, not semantic Rust analysis.
New aliases/macros or changed module wiring still require review.

Two disconnected source probe files (not compiled modules) containing Runner
and hierarchy calls each caused the actual crate-wide test to fail with
exit100, and were removed. These are source-inventory controls, not compiled
production-behavior mutants. The initial inventory implementation failed on
a cfg(test) closure argument delimiter; this was corrected, not counted as
a product failure or successful control.

Fresh Windows fmt and locked workspace/all-target Clippy pass. Final restored
selection10/10 passed in0.452s, aed18f66-c732-4557-8eed-ab997249331f, including
all8 E7 tests, the new inventory and existing paused drain test. No full-suite
repeat; original383/3 credit remains at cea3038. The Activity comment was
moved back from ToolCache. Mutation script now writes explicit UTF-8 LF bytes;
an LF-only no-op control passes W8, all7 genuine compiled mutants were killed
and exact restoration passed. No CRLF-only change earns credit. No native action.

Updated correction manifest (committed bytes):

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/stage_b_tests.rs` | `0b1686dca99f64e869c9809600b236cb3c02e3d4` | `6be923963d9846c5a6371aed247f86ec1cf80b4e26d08399a262ca3596209e26` |
| `crates/arxd/src/service/unix_bridge/tests/e7.rs` | `498fdfa3cbb9d11f243079bba052a0621d2f54fb` | `d6faaabe4c5adc9203410936df58c04aaaaba5ec078832e156bbebe27d6a7dd8` |
| `crates/arxd/src/store.rs` | `265e3533481e288c691ac6b94b4f94669a75d1cf` | `6bfe7db1b5a32f94a462290444eae04389003263563f9f49dcf0e712636cbcb1` |
| `scripts/test_discovery_e7_mutations.py` | `1ac80cdf093fda548fe641ba6933824fcfb0c233` | `d66938a2cae43084d9d22e8204e6716f60d6a5a6564abfa6cc2b7663ed525a4a` |
