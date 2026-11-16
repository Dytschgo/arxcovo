# S2b readiness gate flake repair: independent Opus review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-16.
Single-agent review, no subagents. It reviews an OpenAI / gpt-6.1-sol / high
builder delta, so it counts as a different-provider review. Standards,
Spec and Security are reported separately.

**Verdict: PASS. No blockers on Standards, Spec or Security.** The
coordinator may publish this exact repair for the five required hosted gates.
This review does not grant a merge, approve a GitHub head, confirm C1 on
main or establish the natural hosted cause. Merge still needs a fresh
exact-head publication review, all five gates green and coordinator action.
Feature work and the sampler stay held until a later exact-main CI run is
green, with no manual reruns. M2 remains open, A7 NO-GO, A12 consumed
without retry and M3 unchecked. The Windows quarantine is unchanged.
Windows and native paths are **unverified on real hardware**.

## Target

- Branch `work/m3-c1-main-confirmation`, reviewed in an isolated T3
  worktree on a local review branch. HEAD
  `c1902d816e42424ac73c2479409fb34ace708522`, base main
  `9074a85a4e36ba86dadd836e2508a0484ef6eba2`. The status was clean before
  and after review. Temporary review probes were reverted with
  `git checkout`, and the status was confirmed clean.
- Builder commits: `711b454` and `eaae28f` share the same source (2 paths),
  and `b0fee0a` and `c1902d8` share the same evidence (1 path).
- `git diff --name-only 9074a85a..HEAD` returns 7 paths. Two are source:
  `crates/arxd/src/service/unix_bridge/tests/s2b.rs` and
  `tools/ci/s2b-native/driver.py`. Five are docs: the S2b evidence, the
  inherited C1 timing disposition and acceptance, the C1 probe evidence
  and PROGRESS. No production, Cargo, workflow, manifest, ADR or C1 source
  changed. `git diff --check` is clean.

## Failure and mechanism (verified against source)

`Execution::dispatch` (`service/executor.rs:388-416`, unchanged) runs the
`#[cfg(test)]` admission seam on a `spawn_blocking` worker. That runs after
the worker passes `preflight()` and before `active.fetch_add`. The seam is
cloned out before the call, so the mutex is not held while it blocks.

In the old fixture, a slow coordinator let the held request expire. The
barrier's `recv_timeout(5s)` panicked inside the worker, so the request
never reached `backend.run`, and line 178 then saw 1 entry instead of 2.
This matches the reported main signal exactly (expected panic 106, Timeout
132, `left: 1, right: 2`). The coordinator's own work includes the panic
dispatch, preflight and the ordinary refusal call. The 5s limit therefore
covered unrelated coordinator latency, not synchronization.

## Standards

- One concern, test-only, small conventional commits. The comments say
  why the code is written this way, not what each line does.
- `driver.py` changes only the `s2b.rs` pin. I recomputed it from the
  committed LF blob (`.gitattributes` `* text=auto eol=lf`, 0 CR):
  `a5dfde49...1f7b` matches the pin. The six other pins and `MANIFEST_SHA`
  `28f05948...0f94` match the HEAD blobs unchanged, including
  `executor.rs` `45da7396...` and `unix_bridge.rs` `323fa965...`. The only
  `s2b.rs` mutation, `group_forgotten`, is anchored in the unchanged
  `Holding` region, and the native harness test name is unchanged. This
  does not weaken the manifest or native gates.
- The blob IDs `5920727...` and `2239703...` match the evidence file.
- Smell baseline: nothing material. The `after_panic_delay` parameter is
  used only on the panic path. The non-panic caller passes `ZERO`, which is
  harmless.

## Spec (repair contract)

- `recv_timeout(5s)` became `recv()`. No numeric deadline replaces it, and
  no production deadline changed. The barrier is now explicit-release only.
- All original checks are preserved verbatim: pre-admitted while Ready, the
  panic dispatch returned with `calls == 1`, ordinary post-panic preflight
  latched, the early refusal does not enter the bridge, the slot is free
  before release, the gate is entered exactly once after admission, Runner
  calls do not increase, the latch stays set and the refusal returns
  `MechanismUnavailable`, followed by drain. The independent residue
  variant is retained.
- The new `s2b_latched_readiness_gate_delayed_release` replays a 5200 ms
  coordinator stall through the real scenario. It is a meaningful
  regression test: it fails before the fix and passes after it.
- The evidence labels the natural hosted cause as unknown and the seam as
  compatible only. This is one fix with 0 failed attempts, a separate
  budget from the stopped cluster's three-fix budget.

## Security and robustness

- Test-only. The `#[cfg(test)]` seam, readiness latch, post-admission
  gate, Runner and executor are unchanged and pinned. No native execution,
  grant, secret or unsafe path is involved.
- **Owner-channel cleanup.** The coordinator future owns the only
  `release` sender. The barrier closure holds `tx` and the `held`
  receiver, not `release`. A coordinator assertion unwinds `block_on`, which
  drops the test future and `release`. `recv()` then returns `RecvError`
  and the worker exits. That happens before Tokio's blocking-pool shutdown
  waits on that thread. **Verified by probe:** I temporarily inserted a
  coordinator panic before `release.send`. Both panic-path tests failed in
  5.30 s with `RecvError` at the barrier, with no hang, and the probe was
  reverted.
- **Whole-fixture timeout: not a blocker.** A hang now needs the
  coordinator itself to block forever without panicking. The old 5 s
  barrier did not protect against that either, because a hung `call()`
  would still block. Such a hang fails loudly at the CI job bound
  (`ci.yml` `timeout-minutes: 12`, within the 15-minute budget). It cannot
  pass falsely. No nextest per-test terminate is configured. Adding one
  would be a separate CI concern and is not needed for this repair.
- **Residual risk (advisory).** The separate 5 s arrival observation
  (`rx.recv_timeout` at line 136) is unchanged. It bounds whether the
  worker reaches the seam, which is a legitimate rendezvous bound.
  Extreme hosted starvation could in principle still trip it. That would
  be a different signal (`Timeout` at 136 before the panic dispatch) and
  would be a new item, not this repair.

## Reviewer-run checks (own target root, Windows, locked, offline)

- `cargo test -p arxd --lib --locked --offline service::unix_bridge::tests::s2b`:
  exit 0, **4 passed**, 5.27 s. The binary hash prefix was `31b51c85`.
- Independent fail-before check: I restored only the old `recv_timeout(5s)`
  and kept the new test. `s2b_latched_readiness_gate_delayed_release` exited
  101 in 5.30 s with the expected panic at 106, `Timeout` at 131 and the
  assertion at 178 (`left: 1, right: 2`). That is the exact main signature.
  The probe was reverted.
- The cleanup probe passed, as described above.
- The E6, E7 and S2b filters: exit 0, **27 passed**, 5.50 s.
- A natural loop of the two original gate tests: **30/30 passed**, 3 s.
- `cargo fmt --all -- --check`: exit 0.
- S2b Python harness (`unittest`, `-I -B`): **11 passed**, OK.
- `git diff --check` from base to HEAD: clean.

Not rerun by me: workspace Clippy and the 28-guard bundle. I accept the
builder's Clippy result as recorded and do not independently claim it. I
did not run the broad workspace suite, which has the known owner-account
baseline failures. No ignored, native, M2, VM, admin or old Lab Manager
action was taken.

## Advisories (non-blocking)

1. PROGRESS "Current work" still reads "await FIRST exact-main CI after C1
   merge". The new blocker line and task-history entry record the failure.
   Refresh that line at publication so the two do not conflict.
2. The delayed regression adds about 5.2 s of wall time, which is
   negligible under nextest parallelism.
3. The next exact-main run after merge is the only C1 confirmation. A C1
   revert is warranted only if install time exceeds 79 s, which was not
   observed.
