# Inactive discovery pre-activation coverage follow-up

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13. Implementation sub-agent;
no subagents. Local test coverage only. Fresh Anthropic Opus 5.5 high review
is required; this evidence is not independent review or publication approval.

## Isolation and pins

Cwd and Git top level were both
`C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-af8951a3-554ed6a00fb07e0f`,
branch `t3/discovery-regression-tests`, outside the canonical checkout.
Initial HEAD was `4eaad1dd4969d2a3207630680bbf002bb0d932a5`, clean. Its only
difference from PR #64 reviewed head
`9f44c0ab0fc228255d18120d5c9b1fd7b40548b5` is the parent's PROGRESS line.
This task did not edit PROGRESS.

Source and final normal workspace run pin:
`99a1fd75f4ad578e55ed5196019cf5e3ecaa7733`.

Read AGENTS, KICKOFF, ADR 0020 D7, the E7 brief, the independent E7
security review (PA1-PA6), the drain/brace/generic final rechecks, and
[PR #64 source PASS comment](https://github.com/Dytschgo/arxcovo/pull/64#issuecomment-6866653579).
The comment pins the exact reviewed head and carries the pre-activation
obligations forward. No hosted status refresh or claim is made here.

## Coverage and limits of each proof

- PA1: `pa1_actual_callers_reverse_finish` drives the actual startup task
  and rmcp `discover_tools`/`list_hosts` callers over in-memory duplex IO.
  Startup's earlier call is held in the counting fake port while a later
  MCP call finishes. A second case completes startup first, then overlaps
  two MCP calls in reverse finish order. It replaces an existing cache,
  checks the winning ticket and public tools, and asserts exact port counts.
  It never allocates an Activity observation ticket itself. Both caller
  completion-time allocation mutations are killed.
- PA2: W3 observes the wiring's actual Readiness handle after drain.
  Janitor wait, S9 and S10 give `Stopping` (the mapped `Stopped` cause),
  no rows and no calls. S10 holds the computed Ready setup outcome before
  the wiring's cancellation guard, so unconditional `set_ready` fails.
  At the existing post-Ready barrier, readiness correctly remains Ready;
  the independent shutdown token prevents rows and calls. No claim that
  shutdown rolls an already Ready state back is made.
- PA3: W1 logs entry to the actual execution drain, sequence join and
  blocking drain through the existing test-only Hooks callback, and asserts
  that order. Moving the real sequence-join block ahead of execution drain
  compiles and fails the ordering assertion. No extra E6 executor barrier.
- PA4: the app fixture passes one parsed Location into StartupDomain,
  compares setup components with those captured from it, and compares the
  factory's `OwnerConfig.cgroup_parent` with that Location's exact `runs()`.
  An unrelated bridge parent is killed. No path is opened at this boundary.
- PA6: the existing normal `t10_reexec_diagnostics` parent runs the added
  fake-port panic-plus-residue case inside its purpose-built isolated child,
  with both backtrace modes. The fake publishes residue and panics. The
  response and stored row honestly report failed/cleanup_unconfirmed,
  cleanup false and stop failed. A subsequent direct bridge gate observes
  residue and terminally latches; clearing the fake flag cannot reset it.
  Four subsequent Execution calls refuse, and the port count remains one.
  Activity withholds the payload; the normal T10 parent asserts exactly one
  fixed panic marker in captured child diagnostics. Raw child output is
  never printed by the mutation script.

PA6 is **partial**: this proves adapter reporting, residue gating and
terminal admission refusal with a fake RunnerPort. It does not exercise
real Runner cleanup during unwind or prove its residue store occurs before
slot release. The fake sets its own flag, and the observation after the
response cannot prove that real temporal property. There is no safe real
Runner unwind seam in this bounded task; preserve that obligation for a
future contract test. No synthetic fake ordering is credited as that proof.

PA5 remains a **future contract gate**: failed Locate must construct a
refused bridge with Unavailable(Setup), and the constant bridge check must
be replaced by the required checks before activation. Nothing here wires
failed Locate, activation, Stage F/G or NativeRunner.

Only test files and the existing cfg(test) wiring/Hooks seams change.
Production behavior and inertness are preserved. No CI, Cargo, lockfile,
policy, AGENTS, native installation or architectural change.

## Commands and results

All commands ran locally on Windows x86_64-pc-windows-msvc, rustc 1.95.0
(59807616e, LLVM 22.1.2). Final source bytes match the source pin above.

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0;
  final run after mutation restoration, 2.73 s.
- `cargo test -p arxd --lib --locked -- service::unix_bridge::tests::e7 service::unix_bridge::tests::t10_reexec_diagnostics`:
  10 passed, 0 failed, 1.72 s during development.
- `cargo test -p arxd --lib --locked -- service::unix_bridge::tests exec::stage_b_tests::p_scan service::application service::tests::stuck_drain_is_bounded_and_never_claims_stop_audit`:
  final focused selection 49 passed, 1 ignored, 2.99 s.
- `python scripts/test_discovery_followup_mutations.py`: seven compiled,
  named-test kills; exact SHA-256 restoration of every mutated source.
  `mcp_ticket_at_completion`, `app_ticket_at_completion`,
  `ready_despite_stop`, `drain_sequence_before_execution`,
  `bridge_parent_not_location`, `panic_claims_no_creation`,
  `residue_gate_omitted`. The first five reproduce the reviewer's surviving
  mutation behaviors. Compile failures are not counted as kills. LF bytes
  are written explicitly; no newline artifact is credited.
- `cargo nextest run --workspace --all-targets --locked`: **386 passed,
  3 skipped**, 12.867 s, one final normal workspace run at `99a1fd7` after
  the meaningful tests settled. No retries. The historical privacy flake
  did not recur in this run; no fix or diagnosis is claimed.
- `git diff --check`: exit 0.

Original E7 **383 passed / 3 skipped** credit stays at `cea3038` only.
The seven original LF mutation kills stay at `09cc061`; those historical
credits are not relabeled as new runs here. This Windows suite does not
establish Linux or macOS behavior and is not hosted CI.

Development failures, corrected before final validation: Location has no
Clone implementation (compilation failure, not a mutation kill); W1's log
slice initially omitted the second port event; the first PA6 assertion
incorrectly expected Err rather than the ordinary failed Value response.
A broad temporary text replacement of an existing E6 expectation and a
Unicode encoding artifact in tests.rs were reverted before final checks.
One attempted diagnostic command addressed the ignored fixture executable
directly, contrary to the requested entry boundary; it failed with WinError
2 before any process started and was not repeated. Every executed fixture
run used the normal T10 parent. No payload was exposed by that failed launch.

## Delivery boundary

Local only. No push, PR creation, CI watch, merge, retry or settings change.
Parent must merge PR #64, re-pin this follow-up, and obtain fresh Opus high
review before publishing. No independent review is claimed here.

M2 remains incomplete. Native, installed and dedicated paths are
**unverified on real hardware**. `E_PROTECTION` and `ExecutionFailed` remain
unknown, with no diagnosis or retry. No owner/admin/account/service,
`/proc`, `/sys`, cgroup, root, VM, private-state or experiment operation,
native installation, or direct execution of an ignored fixture occurred.
The normal T10 parent exclusively owns its child fixture invocation.

## Source manifest

Git blobs and SHA-256 of the committed UTF-8 bytes at `99a1fd7`:

| Path | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/service/application.rs` | `7976171bd2a67d194770e71f33b3f0a347e94a18` | `683345d29a3e48ea17552ff1800a4d0962eaa55281265604610d75a8abce0dd1` |
| `crates/arxd/src/service/unix_bridge.rs` | `05140839546e1930ea187031906fbf99a4fc667a` | `560d0b918c20e75770a1f99367e9816f18a18b9d77872f605360237a5d743f43` |
| `crates/arxd/src/service/unix_bridge/tests.rs` | `469fa6e3c3b2aeef904611a66a5a4ecc80b9c3d0` | `1935ae47d2d056f999f2c50e1c875ea276d8cba10d5fecaae6c5f1e38efedd9a` |
| `crates/arxd/src/service/unix_bridge/tests/e6.rs` | `bf2d031b77b90f2bbfadb0db0419030cf720bde4` | `675ca66e4ce977f9953e80cec2e50681eb1fc398bbe29abbf58cd3fa61aed5d4` |
| `crates/arxd/src/service/unix_bridge/tests/e7.rs` | `696a475ed2fa7d8109cb36648550e033f95a7209` | `edb057eed09832abaf29f5665965a8e0f9c67efdc95c93f7ea00babf7d63edb7` |
| `scripts/test_discovery_followup_mutations.py` | `962e9e4f0d06e84755b5a87ef3332c1e265c30a9` | `6c06cb3464f2451a4af07880ccc28ac829eb9ce6b70f2b1ad51be444f3ea7796` |
