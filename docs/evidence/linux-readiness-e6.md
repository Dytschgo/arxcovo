# ADR 0020 E6: inactive readiness and admission fencing

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-13. Local implementation only;
no subagents. Fresh Opus 5.5 high review is required before publication.

Initial cwd and Git top-level matched the supplied isolated worktree, outside
the canonical checkout. Initial HEAD was clean at
`c5e3e7a7e71d8e3eee19a0a7a02e7099cd249894`. No canonical checkout changes.
Read AGENTS, KICKOFF, ADR 0020 (including D5/D6/D8/D10 and A5) and the full
implementation brief, including E6/T6/T9/T11-T20, mutations and E7 boundary.
The implementation uses the engineering skill. PROGRESS belongs to the coordinator
and is unedited here.

Dependency pins supplied by the coordinator: main `af9289a`, reviewed E4 PR62
head `9937c16`, plus native-evidence documentation in prepared root `c5e3e7a`.
E1/E2/E3/E5 are merged. E4 source/native evidence passed all 11 mutations at
UID 1001 in run `43504012486`; that is prerequisite evidence, not a run by this
builder. E4 Windows checks/merge were pending at delegation. No remote status
was queried. Publication waits for E4 merge, exact re-pin, fresh independent
review and all normal gates. Accepted sequencing permits this inactive local work.

## Source identity

Source commit: `87957ddd4d1f58a9833d14e4ce204a046324e497`.
SHA-256 values describe committed LF bytes, not worktree newline conversion.

| File | Git blob | SHA-256 |
| --- | --- | --- |
| `crates/arxd/src/exec/stage_b_tests.rs` | `37f2e146301807b0ad336f21cf10aadc53a24cdb` | `12fa7c260a8b3423cf9d5dc1e27aa91fe6c45c712f2bab7baa61e8a7850b771e` |
| `crates/arxd/src/service/executor.rs` | `a41af320517c2013566934dd07cf7791be77545b` | `f417d664488182c7866a8c436c2232d06f61f995524e6165a565816ac8642a83` |
| `crates/arxd/src/service/executor/tests.rs` | `7f978f55ea1b3fdab59ff45d5f89e0de54db8829` | `e587f6c97301613c6234576861125150de4333122fef792a216d9dc0737eb84e` |
| `crates/arxd/src/service/unix_bridge.rs` | `a7730406b0e99ada54c476a168c20f9286482350` | `01be705ded08419d17f7c4bad70ec67c4c5941354fe036ea04167ac92d2e78a6` |
| `crates/arxd/src/service/unix_bridge/readiness.rs` | `1ff55b22f810e1907e21a2edc9c9df07be868f92` | `b9c8e297873233f52d3655f03d8543c663a81ff2a19cffdbaa112233bd0935ad` |
| `crates/arxd/src/service/unix_bridge/tests.rs` | `7de7b9fa6d682faf078e592dcfcc9160e32bee72` | `08a3957a3f86d9c49282ef1c58fc1d316d8217721879eb68f8ea2050701acb7a` |
| `crates/arxd/src/service/unix_bridge/tests/e6.rs` | `1134fcc09922a0d08c7ea2cc601bcbf9e4b89429` | `4770db217214e60b0a02418186f32d1a3ca592024d6f0e5b3780e23316722b95` |
| `scripts/test_readiness_e6_mutations.py` | `4a6ecf102b68e22752b099cc4f8638e87cc1c21d` | `ffe62e7b4fa99eb90ed953a1a3510f8b88014eddd9978191661ec35a5c485ce2` |

## Behavior and boundaries

The module-private Readiness shares one AtomicU8 by Arc. Pending-to-ready,
pending failure and ready-to-latch transitions use compare_exchange; the first
writer persists. SetupFailure mapping is exhaustive and carries no payloads.
Preflight preserves config/construction/readiness precedence. The bridge body
checks residue/readiness inside the acquired execution slot, then latches
Containment for residue, Containment or Resource before returning the unchanged
report. Cancellation with clear residue remains available across six calls.

Discovery requires ordered TOOLS rows plus exactly one final strict rights line.
Malformed tables retain row K. Valid failing rights classifications latch Rights
and retain tools/row N. Rights facts are neither released nor stored. The
inactive Linux-standard constructor derives only standard configuration, so
classification uses Standard hardening after readiness; it does not observe the
host or claim a fresh hardening check. Pure admin table tests remain in E1.

The private test wiring factory binds a runner once to an owned temporary
subtree and retains one Arc<Execution>. Handles clone that Arc; they cannot
attach, replace or reset a runner or readiness. T20 checks pointer identity,
one factory invocation and latch retention after dropping/recreating handles.
No ticket allocator, startup task, daemon row, cache or activation is added;
those remain E7. The sole executor cfg(test) barrier is after preflight and
before active.fetch_add. Existing production admission ordering and AcqRel
slot operations remain unchanged; W3e can reuse that same seam later.

P-SCAN includes all four hierarchy production files and readiness in addition
to the existing shim/rights/runner set. The existing tokenizer scans those
sources, checks the reviewed panic payload inventory, excludes fake.rs by name,
and rejects hierarchy kill, killpg and setpriority calls with negative controls.
Existing rights-module policies and hook restrictions remain intact.

`git diff --exit-code c5e3e7a --` E4 shim/rights files, E3 hierarchy files,
E5 exec/{mod,unix,cgroup}.rs, Cargo manifests/lockfile and .github returned 0.
No native Runner constructor, owner/session/account/service, real procfs/cgroup,
VM, private-state or experiment operation was introduced or executed by E6 tests.
Tests use counting fake ports, pure state and owned temporary storage. No
installation, WSL/compiler setup, push, PR, CI action or settings change occurred.

## Local checks

Host: Windows x86_64-pc-windows-msvc. rustc 1.95.0 (59807616e),
cargo-nextest 0.9.151 (6b3f99457). No Linux/macOS execution is claimed.

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0,
  including a final-source run after ownership-test refinement.
- `cargo test -p arxd --lib --locked service::unix_bridge::tests -- --skip t10_reexec_diagnostics --skip t8_private_inputs_all_phases`:
  20 passed, 1 existing fixture ignored (intermediate focused source).
- `cargo test -p arxd --lib --locked service::unix_bridge::tests::e6`:
  final-source 9 passed, 0 failed; covers T11-T20 plus ownership/source pins.
- `cargo test -p arxd --lib --locked p_scan`: 3 passed.
- `cargo test -p arxd --lib --locked l6_e3`: 1 passed; includes expanded hierarchy inventory.
- `git diff --check` and `git diff --cached --check`: exit 0.

- Final normal `cargo nextest run --workspace --all-targets --locked` at
  `87957ddd4d1f58a9833d14e4ce204a046324e497`: exit 0; **375 passed, 3 skipped**,
  8 binaries, 11.728 s test wall time (18.75 s compile). Default profile;
  no retries or explicit ignored-fixture selection. The historical dispatcher
  privacy flake did not recur; this pass does not establish its cause.

## Compiled mutations

`python scripts/test_readiness_e6_mutations.py` ran on final source. Each case
executes `cargo test -p arxd --lib --locked <named-test>`, requires successful
compilation and the named test's failure (a compile error cannot count), and
restores both source files byte-for-byte in finally. All 11 were COMPILED KILLED;
final restoration assertion passed.

| Mutant | Killing test suffix |
| --- | --- |
| gate_removed | t18_stale_preflight_gate_limit_one |
| gate_residue_ignored | t13c_existing_residue_gate |
| post_residue_ignored | t11_t12_t13_t13b_reports_and_latches |
| containment_latch_removed | t11_t12_t13_t13b_reports_and_latches |
| cancelled_latched | t11_t12_t13_t13b_reports_and_latches |
| rights_tools_withheld | t14_t17_rights_tools_and_private_bytes |
| rights_untrusted_ignored | t14_t17_rights_tools_and_private_bytes |
| janitor_wrong_token | t15_exhaustive_setup_failure_dispatch |
| nonmonotone_store | t16_monotone_first_cause |
| rights_released | t14_t17_rights_tools_and_private_bytes |
| domain_rebuilt | t20_shared_domain_handles_never_replace_runner |

Failure history: the first compile found three accidental bool-to-Readiness
substitutions in existing test tuples; these were corrected. The initial T17
assertion rejected the existing public rights metadata field; it was corrected
to check private probe fields/raw rights line. The first post-residue mutant
survived because the next gate could mask a missing post-call latch. The test
now observes readiness immediately after completion, before a second dispatch.
A subsequent mechanical test edit omitted its observed-state binding and caused
a compile failure; this was corrected before mutation validation. No compile
failure was counted as a kill. Two complete mutation runs subsequently killed
all 11 cases, the last on the committed source. No product failure was diagnosed
or worked around.

Production remains MechanismUnavailable. M2 acceptance is incomplete. All
privileged/native/hardware behavior is **unverified on real hardware** by this
slice. The historical dispatcher privacy flake (reference `65762554`), E_PROTECTION
and ExecutionFailed remain unknown; no unrelated fix, native retry or diagnosis
is claimed. No architecture deviation is intended.
