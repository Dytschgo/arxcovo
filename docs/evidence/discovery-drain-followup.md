# Discovery drain SP1/SP2 regression evidence

Builder: OpenAI / gpt-6.1-sol / high, 2026-11-14. Implementation sub-agent;
no subagents. Test-only bounded follow-up to
[the Opus review](../reviews/0020-discovery-followup-opus-review.md), SP1/SP2,
ADR 0020 D7 and the implementation brief E7/W3e. PROGRESS belongs to the
parent and was not edited.

## Pins and isolation

- Prepared PR #65 head / starting HEAD:
  `45549fe3283a5e128f48c7c0bf71aff77ef0eee3`.
- Merged E7 main pin: `0a95d2806b197431ae7d7c9cc002fdaadaeb145d`.
- Source and mutation controls:
  `1eccbfcb772603db04abb0c85d5aa803c6f0c844`.
- Verified cwd equals Git top level in the prepared isolated T3 worktree,
  outside the canonical checkout (Git common directory belongs to that
  separate canonical checkout). Initial HEAD matched the requested pin and
  initial status was clean. No checkout was created or switched.
- Windows x86_64-pc-windows-msvc; rustc 1.95.0
  (`59807616e 2026-04-14`), cargo 1.95.0 (`f2d3ce0bd 2026-03-21`).

## What the regression observes

`sp1_sp2_actual_drain_precedes_sequence_and_empties_workers` holds the startup
discovery's real tracked dispatch worker using the existing E6 admission
barrier. It starts application drain while the sequence is awaiting that
worker. No caller hook label is inspected or used to release the gate.

A new cfg(test) snapshot inside actual `Execution::drain` records entry after
the real JoinSet transfer, its actual `len()`, and completion after the join
loop with its actual final `len()`. A cfg(test) accessor also reads the real
registration/replacement JoinSet length. These are observations of the sets,
not a standalone worker counter. While held, the test requires actual drain
entry, one transferred worker, no completion, and zero replacement workers.
It releases the worker, joins application drain, then requires actual drain
completion and both real sets empty. It also checks zero port calls and the
daemon row's refused/shutdown result.

The new worker gate has a one-second receive bound; disconnection releases
it during failure cleanup. Async waits are at most one second, with a
500 ms real-drain-entry bound. The gate is released and application drain is
joined before the counterfactual assertion. There is no new executor
admission barrier; E6's exactly-one source pin passes unchanged.

All executor additions are individually cfg(test): snapshot type, field,
initializer, accessor and the two observation blocks. A read-only comparison
removed those exact additions and compared the result to the starting Git
blob via `git cat-file`: exact baseline bytes after LF normalization.
The other Rust changes are in existing test-only modules. Application source
is unchanged. Production remains inactive and unchanged after cfg(test)
stripping.

## Checks actually run

- `cargo fmt --all -- --check`: passed after mutation restoration.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo test -p arxd --lib --locked sp1_sp2_actual_drain_precedes_sequence_and_empties_workers`:
  1 passed (0.03 s).
- `cargo test -p arxd --lib --locked service::unix_bridge::tests`:
  34 passed, 1 ignored (5.12 s). Includes E6, E7, production selection pins
  and the normal T10 parent. The existing purpose-built ignored helper was
  used only through that parent in the normal workspace.
- `python scripts/test_discovery_drain_mutations.py`: both compiled named
  assertion kills, followed by exact SHA-256 byte restoration:
  - `exec_drain_after_sequence_hook_kept`: moves only the real execution
    drain call after the sequence join while leaving all caller labels in
    place. Killed by the new actual-drain-entry assertion.
  - `drain_leaves_tracked_worker`: skips the real JoinSet join loop, leaving
    the transferred worker tracked. Killed by the new regression; the
    accepted named assertions require waiting for the held worker and an
    empty actual drained set.
- One final normal `cargo nextest run --workspace --all-targets --locked`:
  **387 passed, 3 skipped**, 8 binaries, 13.001 s, exit 0. No retries.
- `git diff --check`: passed before the source commit.

No compile error counted as a mutant kill. The initial new test compile
needed an explicit channel element type; this was corrected before the
passing focused runs. A multi-filter `cargo test` invocation was rejected
before running tests and replaced with the single bridge-module filter.
Ad hoc production-byte comparison attempts had pattern/encoding mistakes;
the final comparison used explicit UTF-8 bytes and `git cat-file`, and passed.
No source change was made to satisfy those diagnostic scripts.

SHA-256 at the source pin (LF bytes):

| File | SHA-256 |
| --- | --- |
| `crates/arxd/src/service/executor.rs` | `9b73880a50ad439a93ee64c9c92c6d7f5b720d5bcbe6be23c4ab26ce319a2f34` |
| `crates/arxd/src/service/executor/tests.rs` | `599a92bd51d3398a53f12d19abdcecef8c811c4334fbdb6c5e266d30f446186a` |
| `crates/arxd/src/service/unix_bridge/tests/e7.rs` | `cb4cec1f09626a1a20f88e004fd18a3e3f7799f5bab511aa5a8822b659196dea` |
| `scripts/test_discovery_drain_mutations.py` | `ae3f85aab982258a1693ab74e7d3365d630eecc31b71dcd21790bfb870021d51` |

## Limits and delivery boundary

SP1/SP2 have local regression evidence, pending independent Opus high review.
PA5 (failed Locate bridge/readiness) and real-Runner PA6 unwind
store-before-slot-release remain **OPEN**. This adds no readiness, startup,
native constructor, real Runner unwind, Stage F or Stage G implementation.
M2 remains incomplete. Native, installed and dedicated paths are
**unverified on real hardware**; fake ports on Windows establish no Linux
or macOS runtime behavior.

The historical dispatcher privacy flake (65762554) did not recur in the
single final workspace run. Its cause remains unknown. `E_PROTECTION` and
`ExecutionFailed` remain unknown; no diagnosis, fix or retry is claimed.
No private database or path was printed or inspected for that flake.

No NativeRunner, owner/session/account/service, proc/sys/cgroup/root/VM,
private-state, experiment, ignored native fixture, installation, Cargo/CI
policy, GitHub push/PR/watch/merge, retry or settings action was taken.
No secrets or private payloads were logged. Work is local only. Publication
waits for the parent's PR #65 merge, exact re-pin, independent Opus high
review and all five required gates. No acceptance or merge credit is implied
by the parent's supplied Windows-running/source-PASS status.
