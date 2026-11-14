# A6 joined nonnative failure evidence

2026-11-14; OpenAI / gpt-6.1-sol / high, implementation sub-agent.
Base: merged PR99 main `7f046caf25b820c68c548716d62058dd4d944876`.
Contract: ADR0020 D9a and brief A6, with all five requirements retained by
`m2-a4-a6-amendment-muse-recheck.md` (coordinator-supplied Muse6884944598).
Fresh Muse source review and normal required checks remain required. No review,
PR, hosted run or milestone acceptance is claimed here; no subagents used.

Only source changes: new `service/unix_bridge/tests/a6.rs` and its module
declaration in `tests.rs`. Git blobs respectively:
`781b3b36577794c60a12175e2e843296d9e3ece8` and
`f5136186085e03c4225249b569e99debdad6aaa9`.
No production, port trait, exec, package, Cargo, workflow, ADR or PROGRESS delta.

The closed test-local CgroupFs uses object labels, in-memory state and a manual
clock. Existing hierarchy Fake/Manual are private, so no visibility change is
needed. Each test calls actual production `setup()` exactly once inside the real
StartupDomain setup closure. Busy occurs only at the validated `arx-1-0` rmdir;
the verification case successfully removes that leaf before returning a fresh
`arx-2-0` on the next runs listing. The timeout case keeps populated=1 indefinitely
and observes KillTimeout at the unchanged five-second boundary, without real sleep.

Every port operation checks Pending and zero fake runner calls. After setup, the
real readiness latch must report Quarantined, including after `set_ready`.
Repeated command/discovery requests through fresh handles of the same retained
domain refuse, with zero fake runner calls. Owned temporary SQLite is closed and
reopened; all three terminal refusal rows must persist as quarantined, alongside
the original Pending/preflight_failed row. The fourth case cancels on the first
manual janitor sleep and requires Stopped -> Stopping -> persisted shutdown,
including subsequent requests and `set_ready`; no quarantine override.

Windows x86_64-pc-windows-msvc, rustc/cargo 1.95.0. Final-source checks, all exit 0:

- `cargo fmt --all -- --check`; `git diff --check`.
- `cargo clippy -p arxd --lib --tests --locked -- -D warnings`.
- Seven separate `cargo test -p arxd --lib --locked <name> -- --exact` invocations,
  each exactly one passed test. Prefix `service::unix_bridge::tests::`:
  `a6::a6_nonnative_validated_leaf_busy_join`,
  `a6::a6_nonnative_populated_deadline_join`,
  `a6::a6_nonnative_fresh_verification_residue_join`,
  `a6::a6_nonnative_shutdown_during_janitor_stays_stopping`,
  `e6::a5_pending_through_janitor_and_s9`,
  `e6::t15_exhaustive_setup_failure_dispatch`, `e6::t16_monotone_first_cause`.

Deliberate regression red -> restored-source green: four compiled assertion kills
were run before the final seven green checks. An initial Busy baseline had also
passed; this is injected regression evidence, not a pre-existing product bug.

| Temporary mutation | Exact A6 test suffix | Observed red assertion |
| --- | --- | --- |
| setup: replace rmdir error propagation with `let _ = fs.rmdir(...)` | validated_leaf_busy_join | VerifyFailed instead of LeafRemoval |
| setup: replace the VerifyFailed check predicate with true | fresh_verification_residue_join | Postcondition instead of VerifyFailed |
| StartupDomain: set_ready immediately before calling setup | validated_leaf_busy_join | Pending check sees Ready; caught assertion yields Panicked instead of LeafRemoval |
| StartupDomain: discard setup failure instead of state.fail | populated_deadline_join | MechanismUnavailable instead of Quarantined |

All four compiled successfully and ran the named exact test to assertion failure;
no compiler/tool error counted. The temporary runner lived outside the repository,
saved original file bytes, restored in finally after each case and asserted exact
byte identity for setup.rs and unix_bridge.rs; final Git diffs for both were empty.
No permanent mutation harness was added.

Callgraph checked before execution: selected tests -> StartupDomain ->
AdmissionDomain -> UnixBridge with counting `e6::Port`; setup -> this local Fs and
Manual only; Execution::unix_bridge_for_test replaces its inert backend with that
bridge. No LinuxFs or native Runner construction, resolver probe, process re-entry,
product CLI, service/account/owner operation, network, GitHub or CI action ran.
Real runner calls are excluded by construction; fake calls are measured at zero.
This is **nonnative failure evidence**, with **no native nonremovable-leaf claim**.
Native/hardware paths are **unverified on real hardware**. No A4 credit, A9/U9
conclusion, crash provenance, Stage F/G closure, real-Runner PA6 or M2 closure.
