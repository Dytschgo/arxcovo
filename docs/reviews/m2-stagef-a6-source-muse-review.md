# Review: M2 Stage F A6 nonnative source (Muse, independent source/security review)

2026-11-14; Muse; independent source/security review of the Sol A6
nonnative implementation plus the coordinator S2b pin refresh.
Read-only except this file: callgraph inspection, `cargo fmt --all --
--check`, `git diff --check`, `cargo clippy -p arxd --lib --tests --locked
-- -D warnings`, seven exact `cargo test -p arxd --lib --locked <name> --
--exact` filters, `python -I -B tools/ci/s2b-native/test_driver.py`
(11 portable tests), `git` CLI reads and SHA-256 pin check of the committed
LF `tests.rs`. No native Fs/Runner/prod-code edit, no mutant re-run, no
native/product CLI/process/system service/account/owner/probe/network/GitHub/
CI/dispatch/subagents.

## Exact pins

- Sol implementation reviewed:
  `f49388bf558379194e837d7c9df85411115d4832`
  (`test(service): join nonnative A6 janitor failure evidence`).
- Retained: `ee0ba797ebdbc384519329d12ce5f4b5f71f1e94` (same message; byte
  comparison below is on this retained delta).
- Coordinator pin maintenance reviewed:
  `21c808565d53e733e06a2dc177603ad4af73c0d1`
  (`test(ci): keep S2b pin aligned with A6 module declaration`).
- Base: `7f046caf25b820c68c548716d62058dd4d944876` (accepted D9a contract,
  PR99 main). `git diff 7f046ca...HEAD --name-only` is exactly five files;
  production-surface diff is empty (verified this session).
- Accepted contract: [ADR 0020 D9a](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\docs\adr\0020-linux-execution-hierarchy-and-janitor.md:669)
  bounded A4/A6 scope, requirement 5 (A6 join + three mutants).
- Brief A6:
  [0020-linux-hierarchy-implementation.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\docs\briefs\0020-linux-hierarchy-implementation.md:609)
  (injected-port LeafRemoval/KillTimeout/VerifyFailed latch quarantine, zero
  runner calls, persisted refusal, shutdown-Stopping preserved; explicitly
  non-native).
- Sol source evidence read:
  [linux-stagef-a6-nonnative.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\docs\evidence\linux-stagef-a6-nonnative.md).
- Scope (5 files, no prod bytes):
  [tests.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests.rs) (+1 `mod a6;`),
  [a6.rs](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests\a6.rs) (new, 336),
  [linux-stagef-a6-nonnative.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\docs\evidence\linux-stagef-a6-nonnative.md) (+82),
  [driver.py](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\tools\ci\s2b-native\driver.py) (one pin value),
  [PROGRESS.md](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\docs\PROGRESS.md) (+2 queue lines).
- Pin refreshed this range (only one):
  `crates/arxd/src/service/unix_bridge/tests.rs` committed-LF SHA-256
  `e500e65fcfd42f30d0492bced7ac6911de9e0dc6619e50e87e82cc94aced7eae`,
  verified this session from committed bytes (`git show HEAD:...` via
  subprocess, LF-only, equal to worktree). All other PINS, MANIFEST,
  MANIFEST_SHA, guards, restoration logic and workflows unchanged
  (driver diff is exactly that one value).

Scope: A6 nonnative source only. This review grants no native credit, no
A4 acceptance, no M2/Stage F/Stage G/PA6 closure; hardware paths
**unverified on real hardware**.

## Standards

PASS, no findings. One-concern test-only delta: the only source additions
are `mod a6;` plus the new `cfg(test)`-reachable `a6.rs`; production
`setup.rs`, `unix_bridge.rs`, `readiness.rs`, `executor.rs`, `exec/`,
packaging, Cargo and workflows have empty base-to-head diffs (verified).
No new dependency, selector API, port trait change, `LinuxFs`/native Runner
construction, or visibility change to the existing private hierarchy
Fake/Manual. `a6.rs` reuses the existing `super::*` test harness
(`StartupDomain`, `e6::Port` counting, `e6::call/refused/activity`,
`private_temp` SQLite) with a closed local `Fs`/`Manual` pair; 336 lines
are proportionate to four joined failure cases sharing one `joined()`
helper. Smell baseline: no Mysterious Name (Fault/Fs/Manual/joined map to
D9a terms), no Duplicated Code beyond the four one-line test entry points
calling the shared helper, no Feature Envy/Middle Man/Message Chains (all
port calls go through the existing `e6` helpers), no Speculative Generality
(no new seam or flag). `cargo fmt --all -- --check`, `git diff --check`
and `cargo clippy -p arxd --lib --tests --locked -- -D warnings` all pass
(exit 0, observed this session).

## Spec

ACCEPT against brief A6 + ADR D9a requirement 5; every clause is
implemented and observed in the pinned source plus this-session reruns.

- **Actual production setup, three failures.** Each case calls the real
  `setup(&fs, location, &clock, token)` exactly once inside the real
  `StartupDomain` setup closure
  ([a6.rs:194-219](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests\a6.rs:194)).
  Busy faults only at the validated `arx-1-0` rmdir; Timeout holds
  populated=1 to the unchanged 5 s boundary (`Manual` asserts 5 ms sleeps
  and `elapsed == 5 s`); Verify removes `arx-1-0` then observes a fresh
  `arx-2-0` on the verification listing. Expected outcomes asserted as
  `SetupOutcome::Unavailable(LeafRemoval|KillTimeout|VerifyFailed|Stopped)`.
- **Readiness latch.** After setup the real readiness latch must report
  Quarantined (Stopping for Shutdown), including after `set_ready`;
  `set_ready cannot clear failure` is asserted
  ([a6.rs:271-282](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests\a6.rs:271)).
- **Zero runner calls, same-domain refusal.** All port observations check
  Pending plus `calls == 0` during setup; after the latch, command/discovery
  requests through fresh handles of the retained domain refuse with zero
  fake calls, and repeated requests plus `stopping` rows are covered
  ([a6.rs:283-292](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests\a6.rs:283)).
- **Committed quarantined SQLite, reopen.** Owned temp SQLite is closed and
  reopened; all three terminal refusal rows persist as quarantined alongside
  the original Pending/`preflight_failed` row; every row is a committed
  terminal row
  ([a6.rs:293-318](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests\a6.rs:293)).
- **Shutdown stays Stopping.** The fourth case cancels on the first manual
  janitor sleep and requires Stopped -> Stopping -> persisted shutdown
  including subsequent requests and `set_ready`; no quarantine override
  ([a6.rs:176-176](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\service\unix_bridge\tests\a6.rs:176)).
- **Executed feedback (this session, Windows).** Callgraph inspected before
  running: selected tests -> `StartupDomain` -> `AdmissionDomain` ->
  `UnixBridge` with counting `e6::Port`; `setup` -> local `Fs` + `Manual`
  only; no `LinuxFs`/native Runner/resolver/process/service/account/owner/
  network/GitHub/CI path (text search for those tokens in `a6.rs` empty).
  Then ran exactly the seven allowed filters, each `--exact`, one test each,
  all passing: `a6_nonnative_validated_leaf_busy_join`,
  `a6_nonnative_populated_deadline_join`,
  `a6_nonnative_fresh_verification_residue_join`,
  `a6_nonnative_shutdown_during_janitor_stays_stopping`,
  `e6::a5_pending_through_janitor_and_s9`,
  `e6::t15_exhaustive_setup_failure_dispatch`,
  `e6::t16_monotone_first_cause`. Portable `test_driver.py` passes 11/11.
  The four compiled assertion-kill mutants (swallow rmdir error, skip
  verify predicate, early `set_ready`, discard setup failure) were verified
  by wording + production-code inspection only (not re-run; prod edit out
  of scope): the blamed lines exist at
  [setup.rs:288](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\exec\hierarchy\setup.rs:288)
  (`map_err(|_| LeafRemoval)?`) and
  [setup.rs:291-294](/C:\Users\DylanFerraro\.t3\worktrees\Arxcovo\t3-ab032d3d-b9149b5ac0f4c9b0\crates\arxd\src\exec\hierarchy\setup.rs:291)
  (`VerifyFailed` predicate), and the evidence records exact byte restore
  with empty final `setup.rs`/`unix_bridge.rs` diffs — that restoration
  claim is corroborated by the empty prod diff in this range. No compiler/
  tool error credit is taken.
- **Limits (honest, non-removable).** This is explicitly nonnative failure
  evidence: no `LinuxFs`, no real cgroupfs, no nonremovable-leaf induction,
  no hardware/Native/real-Runner claim. A4 native present-residue mechanism
  is separately blocked and does not block this A6 review. No A9/U9, crash
  provenance, Stage F/G, real-Runner PA6, or M2 closure follows.

No new Spec findings; no change beyond the five reviewed files is required.

## Security

No blocker. The fault injection lives entirely in the closed test-local
`Fs` (object labels, in-memory state, manual clock) and the counting fake
port; it cannot touch the host (no path, fd, mount, xattr, kill, spawn,
socket, account, service, or persistence outside the owned temp SQLite that
the test itself creates and reopens). Production failure mappings are
unchanged, `SetupEvidence`/enums carry counts only, and nothing new reaches
a response, row, log, or panic payload. The single S2b pin refresh changes
a hash value only, with guards/restoration/workflows untouched.

## Verdict

Standards: 0 findings (worst: none). Spec: 0 findings (worst: none; A6
join, latch, zero-call, persistence, shutdown-Stopping all implemented and
rerun). Security: 0 blockers (worst: none). A6 nonnative source is ACCEPTED
as nonnative failure evidence only — not native, not nonremovable, not
hardware, not A4 acceptance, not M2/F/G/PA6 closure. A4 architecture remains
separately blocked and does not block A6. This commit adds only this
review; no PROGRESS, source, push, or PR action taken.
