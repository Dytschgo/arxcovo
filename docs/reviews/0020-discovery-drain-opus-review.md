# Discovery drain SP1/SP2 test-only follow-up: Opus review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-14. I was the only
review sub-agent and used no subagents, so this is a single-agent review of
both axes. It covers Standards and Spec, including security, on separate
axes. It is independent of the Sol (OpenAI / gpt-6.1-sol / high) builder. It
is not a GitHub approval and does not grant publication or merge.

## Verdict

- **Standards: APPROVED.** 0 blockers, 3 advisories.
- **Spec and security: APPROVED.** 0 blockers, 4 advisories. SP1 and SP2
  from `1ad362e` are **closed**. PA5 and the real-Runner part of PA6 stay
  **OPEN**.

Production stays inert and byte-identical after cfg(test) stripping. Nothing
here grants activation, Stage F/G, NativeRunner, or a CI, Cargo or policy
change.

## Isolation and pins

- The review ran in an isolated T3 worktree outside the canonical checkout.
  Its cwd and Git top level were both that worktree, on branch
  `t3/review-discovery-drain`. HEAD was `220b996`, and status was clean
  before and after every run, including each mutation run.
- Original target: evidence `711b11b` on source
  `1eccbfcb772603db04abb0c85d5aa803c6f0c844`, parent `45549fe`.
- Port: `00f5038` (source), `288024d` (evidence) and `220b996` (PROGRESS
  only) sit on merged PR #65 main
  `5ae645e6dc49d551298b32d5f72c871acb007864`. The tree of `5ae645e` equals
  the tree of `45549fe` (`dcc484b`).
- **Byte identity, verified.** The four source files have the same Git blob
  at `1eccbfc` and at HEAD: `executor.rs`, `executor/tests.rs`,
  `unix_bridge/tests/e7.rs` and `scripts/test_discovery_drain_mutations.py`.
  The evidence file has the same blob at `711b11b` and HEAD. The patch from
  `45549fe` to `1eccbfc` touches exactly those four files. I recomputed all
  four LF SHA-256 values and each matches the evidence manifest.
- `git diff 5ae645e HEAD` touches only those four files, the evidence file
  and `docs/PROGRESS.md`. No Cargo, lockfile, CI, policy or AGENTS change.
  `git diff --check` exits 0. The diff has no private paths, addresses,
  tokens or owner data. The evidence carries no inherited private path.
- Inputs read: AGENTS, ADR 0020 D7 (shutdown/drain clause), brief E7 and
  W3e, prior review `1ad362e` (SP1/SP2), and
  `docs/evidence/discovery-drain-followup.md`. I did not watch hosted CI;
  current-main run `43524629307` is pending per PROGRESS and is not credited
  here.

## Production behavior and test-only seams

- `executor.rs` gains 35 lines and loses none. Every code item is
  individually `#[cfg(test)]`: the `DrainSnapshot` type, the `drain_snapshot`
  field, its initializer in `native`, `drain_snapshot_for_test`, and the two
  observation blocks in `drain`. The only ungated added line is a comment.
- Strip comparison: I removed exactly the inserted lines from the HEAD blob
  and compared the result with the `5ae645e` blob after LF normalization.
  The diff has only insert operations, and the stripped file equals the
  base. Production `drain`, `dispatch` and admission ordering are unchanged.
- `executor/tests.rs` adds one initializer line inside the existing
  `#[cfg(test)] mod tests`. `e7.rs` adds one test inside the cfg(test) bridge
  test module. `application.rs` is unchanged.
- **One admission seam.** `executor.rs` still reads and invokes
  `admission_barrier` at exactly one site, after `preflight` and before
  `active.fetch_add` (lines 390-392). The new test installs its gate through
  the existing `admission_barrier_for_test`. No second barrier exists.
- The snapshot is written after the real `std::mem::take` of the worker
  `JoinSet` with that set's real `len()`, and again after the join loop with
  its real final `len()`. The accessor reads the real replacement set's
  `len()` under its lock. It is an observation of the actual sets, not a
  standalone counter, and no caller label is read: the test passes a no-op
  `seam` to `app`.

## Standards

0 blockers. Advisories are judgement calls:

- **SA1: positional accessor tuple.** `drain_snapshot_for_test` returns
  `(DrainSnapshot, usize)`, read as `.0.entered` and `.1` (e7.rs:740, 748).
  This is possible Primitive Obsession. A named `registered` field on the
  snapshot read would make the assertions self-describing.
- **SA2: the post-loop `remaining` is structural.** After
  `while let Some(..) = workers.join_next().await`, `len()` is 0 by
  construction. "actual drained JoinSet empty" (e7.rs:773) therefore fails
  only when the loop is removed or short-circuited. That is its value as a
  mutation guard. The evidence should say so instead of implying a separate
  observation.
- **SA3: one unchecked script anchor.** In the mutation script, the label
  rewrite at lines 27-29 uses `str.replace(..., 1)` without the
  `count == 1` assertion that guards every other anchor. A drifted label would
  silently skip the `let _ = e;` insertion. The mutant would still compile
  with a warning, so this cannot produce a false kill. It is a consistency
  nit.

Tests are bounded and sleep-free. The worker gate is
`recv_timeout(1 s)` and is released by `drop(release)` or by unwinding. The
async waits are at most 1 s, plus a 500 ms entry deadline. The gate is
dropped and application drain is joined before any counterfactual
assertion. Every failing path ends within about 1 s: a missed `reached`
unwinds and drops the sender, an entry timeout still releases and joins, and
a join timeout detaches only an async task whose blocking worker is already
released.

## Spec

0 blockers.

Closed:

- **SP1: closed.** The test holds the startup sequence's real tracked
  worker at the E6 seam, starts application drain, and requires the actual
  `Execution::drain` to have entered with 1 transferred worker and 0 in the
  replacement set, while the sequence cannot finish. The committed mutant
  `exec_drain_after_sequence_hook_kept` moves only the real call and keeps
  every label. It is killed by "actual Execution::drain before sequence
  join". My `app_skips_execution_drain` is killed by the same assertion.
  This is the effect-based proof I asked for in `1ad362e`.
- **SP2 / W3e "leaves the executor set empty": closed.** After release and
  join, the test requires completion, the actual drained set at 0 and the
  real replacement set at 0. It also checks zero port calls and the daemon
  row as refused/`shutdown`. `drain_leaves_tracked_worker` is killed. My
  `worker_untracked` mutant, where dispatch uses a detached
  `tokio::task::spawn_blocking`, is killed by "real worker registered".

Advisories:

- **SP-A: "not completed while held" does not prove waiting.** `!held.completed`
  (e7.rs:765) is one sample, taken in the iteration that first sees
  `entered`. The poll loop contends on the snapshot mutex, so it reliably
  reads the state between the two writes. My mutant
  `join_replaced_by_detach_all` replaces the join loop with
  `workers.detach_all()`. That empties the set without awaiting the worker,
  and it **survived the new test 15/15**. Three tests that already existed kill it:
  `busy_cleanup_precedence_quarantine_and_completion_withholding`,
  `late_dispatch_during_drain_preserves_gated_accepted_worker` and
  `t7_dropped_request_cancels_running_fake_and_joins` (lib run 344 passed, 3
  failed, T10 skipped). So D7's "drain owns all spawned work" stays
  protected by the suite, which is why this is not a blocker. However, the
  evidence sentence "the accepted named assertions require waiting for the
  held worker" overclaims. The committed `drain_leaves_tracked_worker` kill
  comes from `remaining == 1`. Suggested fix: re-sample while the worker is
  still held, for example with a bounded yield loop of 100 ms or less
  immediately before `drop(release)` that asserts `!completed` and
  `remaining == 1`. Add `detach_all` to the script.
- **SP-B: the 1 s autonomous release.** For correct code, self-release can
  only cause a false failure, after a stall of about 1 s between
  `reached` and the first snapshot read. A false pass is narrow but possible
  for the move-after-sequence regression. If the test stalls for about
  0.5 s or more before polling, the worker self-releases inside the 500 ms
  window. The moved drain then enters, and contention sampling sees
  entered/not-completed with the application drain still running. Suggested
  fix: record whether `recv_timeout` returned `Timeout`, then assert that the
  worker was released by disconnect. I observed no flake: 30/30 sequential
  runs, plus 48/48 runs in three rounds of 16 concurrent test-binary
  processes.
- **SP-C: no automated exactly-one pin.** The evidence says "E6's
  exactly-one source pin passes unchanged". I found no test that counts the
  executor seam. Brief E6 requires "exactly one cfg(test) barrier seam", and
  inspection confirms it holds (see above). The evidence should say this was
  verified by inspection.
- **SP-D: carried obligations.** PA5 (failed `Locate` bridge/readiness) and
  real-Runner PA6 (unwind and store before slot release) remain **OPEN**.
  The fake-port PA6 stays partial. SP3/PA-b from `1ad362e` (cached
  `tool_discovery: "unavailable"`, start-time `tools_observed_at`) is
  unaffected. This change adds no readiness, startup, native constructor,
  Runner unwind, or Stage F/G work.

Security: test-only. There is no production byte change, no new seam, no
`unsafe`, no new I/O, and the store is touched only through existing
cfg(test) temp fixtures. There is no logging of private data and no
external request.

## Credits and limits

Builder runs at `1eccbfc` are credited by the verified byte identity above.
Earlier credits stay where they were earned.

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- focused: 1 passed
- `service::unix_bridge::tests`: 34 passed, 1 ignored
- two compiled named assertion kills with exact restore
- one final normal `cargo nextest run --workspace --all-targets --locked`:
  **387 passed, 3 skipped**, no retries

The disclosed failure history is plausible and honest: a channel type
compile fix, a rejected multi-filter invocation, and corrected ad hoc
comparison scripts. None was counted as a check or a kill.

My runs at HEAD `220b996`, Windows x86_64-pc-windows-msvc, rustc and cargo
1.95.0:

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `cargo test -p arxd --lib --locked sp1_sp2_actual_drain_precedes_sequence_and_empties_workers`:
  1 passed, 0.03 s.
- `cargo test -p arxd --lib --locked service::unix_bridge::tests`: 34
  passed, 1 ignored, 2.15 s.
- `python -I scripts/test_discovery_drain_mutations.py` (committed, run
  once): both COMPILED ASSERTION KILLED, then EXACT SHA-256 RESTORE.
- A reviewer mutant script, kept in the session scratchpad and not the
  repository, with exact SHA-256 restore and a clean status afterwards:
  - `worker_untracked`: killed, "real worker registered".
  - `app_skips_execution_drain`: killed, "actual Execution::drain before
    sequence join".
  - `join_replaced_by_detach_all`: **survived 15/15** on the new test, and
    was killed by 3 existing tests in one mutated `arxd` lib run with T10
    skipped (SP-A). That run credits nothing for the unmutated suite.
- Flake probe of the unmutated test: 30/30 sequential runs, and 48/48 runs
  in three rounds of 16 concurrent processes.
- I ran no full workspace suite. The bytes equal the credited pin, and no
  finding called for one.

Limits:

- A Windows run does not compile or run `cfg(target_os = "linux")` code. All
  ports are fakes. Nothing establishes Linux or macOS behavior, and this is
  not hosted CI.
- M2 is incomplete. Native, installed and dedicated paths are **unverified
  on real hardware**.
- The historical dispatcher privacy flake, `E_PROTECTION` and
  `ExecutionFailed` remain unknown. No fix, diagnosis or retry is claimed.
  Stage F and Stage G remain separate and inactive.
- No NativeRunner, owner, session, account, service, `/proc`, `/sys`,
  cgroup, root, VM, private-state, experiment, direct ignored-fixture call,
  installation, GitHub push/PR/CI-watch/merge/retry or settings action was
  taken.
- No product or PROGRESS edit. This file is the only change.

## Summary

Standards: 0 blockers, 3 advisories. The worst is SA2, a post-loop `len()`
that is 0 by construction. Spec: 0 blockers, 4 advisories. The worst is
SP-A: the held-state `!completed` sample does not prove Execution waits, and
a `detach_all` mutant survives the new test, though existing tests kill it.
PA5 and the real-Runner PA6 proof remain OPEN pre-activation gates.
