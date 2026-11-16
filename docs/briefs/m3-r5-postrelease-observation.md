# R5 post-release observation contract

Root Sol medium contract review of Opus disposition9075daa,2026-11-16.
This accepts C1-C3 with the restrictions below, replaces C4 and settles C5.
No behavior fix, timeout increase, rerun or quarantine expansion is authorized.
Mainc612152 is red on Windows; PR159 and feature merges stay held.

## Outcome and scope

Prepare one reviewed diagnostic PR so the next natural R5 occurrence identifies
which predicate remains and which read-release stage was reached. This PR must
not be merged while main is red without separately recorded steering. Publishing
for its own checks does not waive that prerequisite. A green PR is not green main.

Owned paths: consensus.rs (cfg(test) stamps only), consensus/three_node.rs (R5
observation only), .config/nextest.toml (new, exact R5 success output only),
tools/ci/test_windows_cluster_quarantine.py (pin the new output-only config),
docs/evidence/m3-r5-postrelease-observation.md, docs/PROGRESS.md. Retain the
Opus disposition and this contract. No other source, workflow, Cargo, helper,
assertion, deadline, retry, scheduling, membership or quarantine change.

## Observation semantics

- Preserve the existing combined two-second wait and assertion byte-for-byte.
  Existing shutdown reports, assertions and gate-release order also stay intact.
- Record snapshots at gate open and wait end: state/log strong counts using Weak
  strong_count only, read permits, both release flags, and read_blocking counts.
  Emit one fixed bounded ASCII line for the R5 variant before unwrapping the
  existing wait result, on both pass and fail. No paths, errors, IDs or free text.
- An observer samples at one millisecond or slower, uses Weak handles plus the
  semaphore Arc and release flags, and never upgrades a store. It must be stopped
  and joined before the assertion, including test failure cleanup; it must not
  hold or extend either store lifetime. First-clear values mean first observed
  clear, not exact release time or permanent absence. Missing is a fixed enum.
- Read-closure stamps are test-only and keyed by this node's own store/fixture,
  never a global shared slot. Record entered, lock-acquired, function-returned,
  store-drop-returned and permit-drop-returned without moving existing work or
  drops. Use bounded elapsed durations plus an explicit before-open value when
  an event precedes the gate anchor. No wall-clock timestamps or addresses print.
- Stamps must not retain store Arcs or a ticket after completion. The closure
  retains only its existing resources and a small test-observation handle.
  Normal uninstrumented reads have no observer work beyond cfg(test) lookup.
- Replace C4: do NOT subtract asynchronously sampled stage/count fields to claim
  an exact detached-engine-holder count. They are not an atomic snapshot. Report
  the raw bounded facts and a fixed unknown classification where ownership is
  ambiguous. A store count after closure completion can suggest another holder;
  it cannot identify which OpenRaft task owns it. No causal label is automatic.

## Visibility and limits

The sole nextest override selects exactly arxd lib
cluster::consensus::three_node::r5_shutdown_read_owners and sets success-output
only. Every other profile/filter/scheduling/retry/timeout setting keeps defaults.
Pin the entire config in the existing Python guard, including rejection mutants
for a broadened filter, scheduling override, retries or timeout. Nextest support
must be checked against installed0.9.151. R5 passing output is already bounded
by its fixed reports and the new line; cap the new line at1024ASCIIbytes.
No raw diagnostic ring is added on pass. No more Windows sampling or workflow
step is added, and the existing exact-eight sampler remains unchanged.

## Evidence and test budget

One focused unmodified-source R5 control with installed locked/offline tools on
the new code; no natural repeat loop. Unit tests exercise rendering and missing,
before-open, observed-clear and overflow values. Throwaway held-store and held-
permit controls must hit the unchanged cleanup assertion and show distinguishable
raw fields. Observe a controlled store-drop stage hold if the existing seam can
support it without production changes. Restore every mutant/hash, record exact
commands/results and do not claim a hosted reproduction. Clippy/fmt and existing
quarantine/sampler/workflow guards run once as relevant. No broad cluster suite
unless a concrete integration concern requires it. Any natural red is recorded,
not rerun. A control failing twice for one reason returns to contract review.

Opus builds diagnostics; Sol independently reviews before publication. At most
two implementation corrections, no Windows behavior fix attempt. Sol3/3 used,
Opus fixes0/2 remain. All five exact-head gates and independent review are still
required; landing while main is red needs the separately settled merge decision.
M2 open,M3 unchecked,eight Windows quarantines unchanged; all hardware paths are
**unverified on real hardware**.
