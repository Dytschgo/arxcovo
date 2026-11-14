# M2 S1 failed Locate and live check: independent source review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
This is a single-agent review with no subagents. It reviews the
OpenAI / gpt-6.1-sol / medium S1 implementation, so it is a
different-provider review. Standards and Spec are reported separately.
It grants no publication, merge, activation, experiment, S2/F0 work or
M2 acceptance.

## Isolation and pins

- Cwd and Git top level were the same isolated T3 worktree
  (`<isolated-worktree>`), not the canonical checkout. Branch
  `t3/m2-s1-source-review`, status clean, HEAD
  `a2c34b5d3d3022897693652f2b6f7a38ac2caa07` before any work.
- Base: `f56972832ef3e491994d5d29fadc30f0c7ad3e17`.
- Source: builder `be19717e78bcdc516c60ec7cd6cdd263a5350ffc`, retained
  `3caa8d043e0cabd28922a52bebd1acfdd952121b`. Same parent, same tree
  `4071b71f5878bf637ae9cc9588d00ecb63dde08d`.
- Evidence: builder `7822b61e3d9bd0853d77ebead4502ae934a19b7f`, retained
  `79e1a803a5d4f7096f39b55ac0a77c9da9e2a70b`. Same tree
  `93dded1f45244a005c174bc59e544f9088d07d5b`.
- `a2c34b5` adds only two PROGRESS lines queueing this review.
- Spec: accepted S1 in `docs/reviews/m2-remaining-gates-proposal.md`.
  Final narrow recheck `29e2d03`, retained `b4f78a4` (same tree
  `d32ecfd173f6d598ac0a27db4b05c2f0ea095435`); its SP-N1 and SP-N2
  advisories were checked against the tests.
- Standards: AGENTS, KICKOFF, CONTRIBUTING and the review smell baseline.
- Changed source: `unix_bridge.rs`, `unix_bridge/tests.rs`,
  `unix_bridge/tests/e6.rs` and `unix_bridge/tests/e7.rs` (+219/-31),
  plus `docs/evidence/linux-failed-locate-s1.md`.
- The four source blobs at HEAD match every byte count and SHA-256 in the
  evidence manifest. All five files have 0 CR bytes. `git diff --check
  f569728 a2c34b5` passed. No raw owner path, secret or private data is in
  the diff or evidence.

## Production-byte equivalence

I checked this a different way from the builder. Each of the four hunks in
`unix_bridge.rs` sits inside a `#[cfg(test)]` item: `UnixBridge::new`
(attribute at line 58), `impl StartupDomain` (366, with the `StartupSetup`
alias at 364) and `impl AdmissionDomain` (433). There is no new
non-test import or item. `wiring_location` and the other three files are
inside the existing `#[cfg(test)] mod tests`. Non-test code is unchanged,
which agrees with the builder's stripped-stream hash. `Execution::configured`
is untouched and still builds the native `MechanismUnavailable` path.

## Standards

0 blockers, 2 advisories.

- **ST-1 (advisory, possible hidden coupling).** `live_check(config)` in
  `e7.rs` always builds `Layout::for_test(Platform::Linux)` and ignores the
  layout its caller gives `StartupDomain::new`. In W2, the macOS and Windows
  domains get a check that uses the Linux layout. Those iterations never
  run preflight, so nothing goes wrong today. Passing the same layout to
  both would remove a silent divergence.
- **ST-2 (advisory, Mysterious Name / flag argument).** The S1 `domain(...,
  refused: bool, ...)` helper changes the check's behaviour with a bare
  boolean. Passing the check closure, or a small enum, would make the call
  sites say what they mean.

What passed: small, single-concern conventional commit, with every new
behaviour tested. The W8 `StartupDomain::new(` inventory moves from 3 to 4
for exactly one new call site. The wiring pins (one `AdmissionDomain::new`,
one `Readiness::pending()`, no `Runner::new(`) are unchanged, and a new pin
forbids `|| Ok(())`. The crate-wide inactive-source inventory passes with
its scanner unchanged. The evidence records the earlier scanner and Clippy
failures and how they were fixed without weakening either guard.

## Spec

0 blockers, 3 advisories.

Requirements met:

1. **Optional Location / pure helper.** `StartupDomain::new` takes
   `Option<Location>`. `wiring_location` takes a fake
   `FsResult<ProcFile>`, requires `magic == PROC_MAGIC`, and calls the
   shared `parse::locate`, which is the same rule `setup.rs` re-applies.
   There is no `/proc` read and no native resolver.
2. **None does no derive or factory work.** `parent.ok_or(MechanismUnavailable)`
   runs before `derive` and the factory. No placeholder path or
   `OwnerConfig` is built. One constructor call each is still pinned.
3. **Terminal Setup before handle.** `readiness.fail(Cause::Setup)` runs
   before `AdmissionDomain::new`, so it happens before any handle exists.
   The setup slot is `None`, and `TestHooks::start_discovery` takes it, so
   setup and its janitor cannot run.
4. **Owned live check.** `check` is an owned
   `Fn() -> Result<(), Unavailable> + Send + Sync + 'static`. `live_check`
   calls `config.execution_preflight(&layout, &FakeResolver::default())`
   on every call. This matches the closure shape in `configured`, which
   uses `NativeResolver`. Nothing evaluates eagerly. L-b's per-call counter
   reaching 1 and then 2 shows the check is live.
- **L-a.** The test covers six inputs: empty (no `0::` line), a v1 line,
  the wrong unit, 4,097 bytes, magic 0 on a valid line, and
  `Err(FsError::Io)`. For each it asserts: 0 factory calls, no setup slot,
  exact `MechanismUnavailable` preflight, no daemon rows after
  `start_discovery`, one refused agent `preflight_failed` row, 0 setup or
  janitor calls, and an error after `set_ready`. The fake port panics if
  called.
- **L-b / L-b2.** A refusing `AccountIdentityChanged` check, distinct from
  `MechanismUnavailable`, wins with or without a location, on two
  consecutive preflights. With a location there is 1 factory call;
  readiness stays pending until setup, setup makes it Ready, and the check
  still wins afterwards. Without a location there are 0 factory and 0
  setup calls. The test message and the evidence claim only that the check
  runs first, so the SP-N1 overclaim is gone.
- **L-c.** W1 builds its domain through `wiring_location` and `live_check`.
  Its event-log, same-parent and single-domain assertions are unchanged and
  pass.
- **Mutants.** The evidence names all five required mutants, each killed by
  a named assertion with an exact restore hash. I recompiled
  `none_leaves_pending` and got the builder's exact mutant SHA-256
  (`318c6fb1…`), killed at "L-a never Ready". The `readiness.check()`
  observable is used rather than the shared token, which follows SP-N2.

Advisories:

- **SP-1 (advisory): the L-a oversize input does not isolate the size
  limit.** `vec![b'x'; 4097]` already fails `strip_prefix("0::/")`. I
  removed the `> 4096` guard as a reviewer mutant and L-a still passed.
  The existing `parse::tests::bounded_valid_inputs_and_total_byte_data`
  killed that mutant, because its 4,097-byte input is otherwise valid. The
  limit is pinned, but not by L-a. A padded valid line in L-a would
  isolate it.
- **SP-2 (advisory): which failure cause is set is not pinned.** I swapped
  `fail(Cause::Setup)` for `fail(Cause::Stopped)` and L-a still passed.
  The stored runner error comes first in preflight, and L-a only checks
  `state.check().is_err()`. Asserting
  `state.check() == Err(MechanismUnavailable)` would kill a swap to
  `Stopped`. A swap to `Hardening` or `Janitor` cannot be told apart by
  token, since causes deliberately carry no diagnostics.
- **SP-3 (advisory): L-b's "runner is not called" holds by construction,
  not by observation.** L-b only calls preflight, which never reaches
  `run`, so the panicking port proves nothing extra. A refused dispatch
  through the located domain would observe it.

## Runs (this review, local Windows, HEAD `a2c34b5`)

- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: exit 0.
- `cargo test -p arxd --lib --locked service::unix_bridge::tests`: 36
  passed, 0 failed, 1 ignored, 316 filtered. Both `e7::s1` tests passed.
- `cargo test -p arxd --lib --locked exec::stage_b_tests::w8_e7`: 2
  passed.
- Reviewer mutants. Each compiled, used one exact replacement, and was
  restored with a matching SHA-256; the worktree was clean afterwards:
  - `wiring_check_constant`: `Box::new(check)` in `StartupDomain` replaced
    by `Box::new(|| Ok(()))`. Killed by L-b ("L-b live check token FIRST
    only") and by W8 (`!wiring.contains("|| Ok(())")`).
  - `none_leaves_pending`: killed, exact builder mutant hash.
  - `setup_cause_swap`: **survived** L-a (SP-2).
  - `oversize_bound_removed`: **survived** L-a and was killed by the parse
    bound test (SP-1).

Builder runs credited at their exact source pin, not repeated: the full
locked nextest (389 passed, 3 skipped) and the other four builder mutants.
The evidence session IDs and mutant hashes are consistent with the source
manifest. Repeating the full suite would have added nothing to review the
change.

## Limitations

- Windows only. No Linux compile or run of Linux-gated code, and no hosted
  CI, was done or credited. Hosted Linux and the five required checks are
  still needed before delivery.
- Native, installed and dedicated paths are **unverified on real
  hardware**. M2 is incomplete. Real-Runner PA6 is still OPEN.
- The historical `E_PROTECTION`, `ExecutionFailed` and privacy-flake
  failures were not retried or diagnosed.
- Nothing native, root, `/proc`, `/sys`, cgroup, service, account, VM,
  owner, private-state or ignored-fixture was touched. There were no
  installs, GitHub writes, pushes or CI actions.

## Verdict

**Standards: PASS. 0 blockers, 2 advisories; the worst is ST-1, a
hard-coded Linux layout in `live_check`.**
**Spec: PASS. 0 blockers, 3 advisories; the worst is SP-2, the Setup cause
is not pinned.**
The advisories can be fixed later or in this PR and do not block S1
publication. S1 still needs current-base integration, hosted Linux and the
normal required checks. This review authorizes no activation, experiment,
S2a/S2b/F0 work or M2 acceptance.
