# M2 remaining gates: proposed next slices (design only)

Author: Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-14.
Status: **proposal, not accepted.** It needs an independent Sol high review
before anyone acts on it. I claim no review independence for ADR 0019/0020
text I helped write. This file grants no implementation, run, dispatch,
activation or owner step.

## Isolation and pins

- Cwd and Git top level were the same isolated T3 worktree, on branch
  `t3/m2-completion-route`. HEAD was
  `43ee9108bc4f8a7d98ebbe62c4a13af98abee9ed` (prepared PR66 head) and the
  status was clean before I wrote this. It is not the canonical checkout.
- I read everything through Git objects at `HEAD` unless another pin is
  given: AGENTS, KICKOFF, ADR 0017, 0019 (D7, C2), 0020 (D5-D10 and the
  sequencing clarification), brief 0020 (Stage F/G, privileged A1-A12),
  `docs/reviews/0020-e7-independent-security-review.md` (PA5/PA6),
  `0020-discovery-followup-opus-review.md`,
  `0020-discovery-drain-opus-review.md` (SP-D),
  `docs/evidence/linux-discovery-followup.md`, `docs/dev-host-setup.md`, and
  the Stage F phase-1 report via `git show
  20f39c0:docs/reviews/unix-protection-phase1-evidence.md`. That file is
  byte-identical to `afe5434`, because `git diff afe5434 20f39c0` on it is
  empty.
- I ran nothing beyond `git` reads. I made no external request and touched
  no private state.

## Current state (facts only)

- M1 is accepted and M2 is incomplete. E1-E7 are merged and inactive, with
  main at `5ae645e`. PR66 is a test-only source PASS (`414caaa`), and its CI
  is still outstanding. Production is `MechanismUnavailable`:
  `Execution::configured` builds `Self::native(` (`service/executor.rs`).
- Native, installed and dedicated paths are **unverified on real hardware**.
  The dispatcher privacy flake, `E_PROTECTION` and `ExecutionFailed` are
  unknown. Nothing here attempts a fix or a retry.
- **Open pre-activation obligations:** PA5 (failed `Locate` and the constant
  bridge `check`) and the real-Runner part of PA6 (residue stored before the
  slot is released on unwind). Fake-port PA6 is partial
  (`linux-discovery-followup.md`, "PA6 is partial").
- The Stage F driver, its workflow and `exec/disposable.rs` are
  byte-identical to the failed historical dispatch `5b4f6b2`
  (`git diff --stat 5b4f6b2 HEAD` shows only the README). Re-dispatching
  them would be a **retry**, and ADR 0019 D7 does not authorize one.

## Slice S1: PA5, failed Locate and a real check (cfg(test) wiring only)

ADR 0020 D7 says: "A `Locate` failure constructs the bridge with a refused
runner and readiness `Unavailable(Setup)`." Today `StartupDomain::new` takes
an already-located `Location` (`unix_bridge.rs:365-370`) and passes
`Box::new(|| Ok(()))` as the check (`:389`).

Change (all `#[cfg(test)]` constructors; production bytes unchanged after
cfg stripping):

1. `StartupDomain::new` takes `location: Option<Location>`. The caller gets
   it from one new pure helper: `wiring_location(file: FsResult<ProcFile>)
   -> Option<Location>`. It requires `magic == PROC_MAGIC` and the shared
   `parse::locate`, the same T2/T3 rules setup re-applies at `setup.rs:214-219`.
   Tests feed only fake `ProcFile`s and never read the real `/proc`.
2. `AdmissionDomain::new` and `UnixBridge::new` take `parent:
   Option<PathBuf>`. With `None`, the runner is
   `Err(Unavailable::MechanismUnavailable)`. `derive` and the factory are
   not called, and no placeholder path or `OwnerConfig` is made. The
   constructor-call counts stay at exactly one each, as the W8 and E6 pins
   expect.
3. On `None`, readiness is set with `fail(Cause::Setup)` before
   `handle()` is reachable. The setup closure is stored as already
   consumed, so setup and the janitor never run.
4. `check` becomes a `StartupDomain::new` parameter, the same shape
   `Execution::configured` uses (`executor.rs`, `configured`). Tests pass
   `config.execution_preflight(&layout, &FakeResolver)` (the existing
   `config.rs` test resolver) and a refusing closure. A wiring pin forbids
   a literal `|| Ok(())` check in `StartupDomain`.

Tests (one module):

- **L-a.** Each refusal input: no `0::` line, a v1 line, the wrong unit
  component, more than 4,096 bytes, the wrong magic, and a port `Err`. For
  each, assert: 0 factory calls, 0 setup calls,
  `preflight == MechanismUnavailable`, `start_discovery` writes 0 rows and
  0 port calls, an agent dispatch gets a refused `preflight_failed` row,
  and `set_ready` afterwards cannot reach `Ready`.
- **L-b.** A located input with a refusing check: preflight refuses with
  that check's token, the runner is not called, and Ready is still
  reachable only through setup.
- **L-c.** A located input with the real `execution_preflight` and the fake
  resolver: the W1 log is unchanged.
- **Mutants**, compiled, with an exact SHA-256 restore:
  `none_calls_factory`, `none_leaves_pending`, `none_runs_setup`,
  `check_ignored` and `magic_unchecked`. Each must be killed by a named
  assertion.

Acceptance: fmt, workspace Clippy `-D warnings`, the focused module, a full
locked nextest, the five mutants killed, and hosted Linux CI running the
Linux-gated parts. A Windows run proves nothing about them. Then an Opus
high review (Sol builds) and the normal five checks. **Non-goals:** no
production `Locate` read, no change to `configured`, no Stage F/G.

## Slice S2: PA6, residue stored before the slot is released on unwind

**Can ordinary owned temp/test seams prove the order for the real Runner?
Not end to end without weakening a fence.** The fences come before any
`Group` exists. `unix::run` refuses a non-root caller first (`unix.rs:167`),
then runs `account(c)`. `Group::create` requires `trusted_root` on every
ancestor (uid 0, no group/other write; `unix.rs:531-545`), plus `fstatfs`
cgroup2 magic, uid 0 and mode on the parent (`cgroup.rs:52-67`). An
unprivileged real Runner therefore returns `ServiceIdentity` without holding
a `Group`. I reject a cfg(test) path that injects a temp `Group` into
`unix::run` or skips these checks: it would put a fence-bypass branch in the
privileged module and still not exercise cgroupfs.

**What ordinary seams can prove, by decomposition.** The order is a
single-thread program-order property. `Group::drop` stores `residue`
(Release) inside the unwinding `unix::run` frame (`cgroup.rs`, `impl Drop`).
`catch_unwind` in `UnixBridge::run` sits inside the slot (`unix_bridge.rs:151`).
`active.fetch_sub(AcqRel)` follows `backend.run` returning, in the same
closure (`executor.rs:395-406`).

- **U-a.** A real `Group`, built from ordinary temp files with the existing
  cfg(test) struct-literal pattern (`cgroup.rs` tests, made available as one
  `#[cfg(all(test, target_os = "linux"))]` constructor), is moved into a
  test `RunnerPort`. That port reports the Group's own `residue` Arc and
  panics while holding it. The temp leaf holds an owned file, so
  `unlinkat` fails. The test calls `Executor::run` on a real `UnixBridge`
  directly. When it returns, assert `Failed{Unknown}` and that the flag is
  already true. The flag is set by real `Drop` code during a real unwind,
  not by the fake. This differs materially from the current fake PA6.
- **U-b.** Through `dispatch` with `limit = 1`, the next request reaches the
  gate, latches at step 1 (`unix_bridge.rs:86-87`), refuses, and makes 0
  port calls.
- **U-c.** A source-inventory test with a mutant control. In `unix.rs`,
  `Group` appears only as the local `group` and the `ChildGuard` field.
  There is no `forget`, `ManuallyDrop`, `Box::leak`, `Arc<Group>` or thread
  move of either. In `executor.rs`, `fetch_sub` follows `backend.run` in
  the same closure. This is a lexical guard (as E7 PA1 discloses), not a
  proof.
- **Mutants:** `drop_skips_store`, `catch_outside_slot` (moves `fetch_sub`
  before `backend.run` returns) and `group_forgotten`.

**Finding for the reviewer (not silently resolved).** On unwind, `body`
exits before its post-run latch (`unix_bridge.rs:94-103`), and the catch arm
maps to `Failed{Unknown}` without latching. ADR 0020 D6 says "stores the
flag, then latches, before its slot release". On this path only the store
precedes release. The bound still holds through gate step 1 at the next
slot holder, which is the behaviour the fake PA6 observed. I recommend
aligning code to the ADR rather than rewording it: in the `Err(_payload)`
arm, latch `Containment` if `cleanup_residue()` is set. That is one inert
line in non-activated code. U-a then also asserts the latch.

**Exact remaining real-host evidence (PA6-native).** Run real `run_core`
as root on a disposable Linux host with cgroup v2 and a delegated,
protected `runs/` holding `memory pids`. Induce a fixed-literal panic while
a real `Group` is held and its cleanup is unconfirmed. Observe that the
residue flag is visible when the bridge returns and that the next call is
refused. This needs a test-only panic point in the privileged module, and
it runs only from the ignored disposable fixture. I propose adding it as
brief item **A13**, under Stage F's separate design and review. It stays
**unverified on real hardware**. S2 closes only PA6-local. Whether PA6-local
plus A13 replaces "PA6 open" is a decision for the Sol review.

Acceptance is the same as S1, and U-a/U-b run on hosted Linux only.

## Stage F: a bounded feedback artifact for a future fresh proof

The phase-1 facts stand. Only the fixed code `E_PROTECTION` was retained.
The 13 `protected` call sites (`driver.py:99`…`388`) are not
distinguishable, and the escaping exception may come from `finally`. There
is no trace, no replay and no root cause. I state **no hypothesis and no
fix.**

**F0 (local, unprivileged, reviewable code).**

- `protected(path, site)` takes a fixed site token, one per call site. A
  refusal raises a structured `Refusal(code, site, phase, ordinal, kind,
  owner_zero, write_bits)`. Here `kind` is one of dir, link or other,
  `owner_zero` is a bool, and `write_bits` is `st_mode & 0o022`. A trace
  intended for replay also keeps the ordered predecessor triples, so it
  reaches the same ancestor.
- The top level records the first and the final exception separately, plus
  fixed mutation tokens (none, staged, account, service, cgroup) and a
  cleanup token (confirmed, unconfirmed, not_reached).
- The record has one schema-validated JSON line of at most 4 KiB. It
  contains no path, uid number, name, argv, environment or payload. The
  workflow prints only that validated line.
- **Pure replay loop:** factor the predicate as `decide(triples) ->
  Option[(ordinal, conjunct)]`. A unit test feeds a recorded line into the
  real `decide` and requires the recorded refusal. That makes it
  red-capable once a real line exists. Until then, tests on synthetic
  triples are labelled mechanism tests, not reproduction.
- Known unknown for F0 review: whether the unchanged driver and
  `disposable.rs` still match the post-0019/0020 Runner contract.

**F1.** An independent review of F0: Opus high, which the phase-1 report
requires before any diagnostic proposal runs, and Sol for this
architecture. F0 merges through the normal gates.

**F2 (fresh proof, not a retry).** A new reviewed SHA is dispatched once
under the existing gate. It produces new evidence and claims nothing about
run 43091339273, which stays unknown permanently. If the outcome is
refused, the trace drives the debug loop (red replay, then hypotheses),
with no automatic second dispatch. If it passes, it supplies inputs for
A1-A13 only after their own design. Cleanup that is unknown or unconfirmed
ends that environment's use.

## Stage G (out of scope here)

Stage G is a separate architecture change, with review and all checks. It
needs ADR 0019 D7 and C2 (G-P1..G-P3), ADR 0020 D10, accepted Stage F
native evidence (A1-A13), and readiness and hardening rendered in
`capability()`. Production stays `MechanismUnavailable` until then. This
proposal adds no approval flow and does not widen agent or user authority.

## Owner decisions and actions actually needed

None are needed for S1, S2 or F0. These are needed later:

1. **F2 dispatch.** The workflow requires `github.actor ==` the owner
   account and a `UNIX_EXECUTION_REVIEWED_SHA` repository variable
   (`.github/workflows/unix-execution-experiment.yml`). The owner sets the
   variable to the reviewed SHA, dispatches once, then clears it.
2. **The M2 acceptance host.** A real host where the owner installs `arxd`
   and creates the dedicated standard build account (KICKOFF M2; ADR 0009).
   Which OS and host are used is not settled by the documents I read
   (**known unknown**).
3. **Only if Stage F uses a local VM instead of a hosted runner:** the
   `dev-host-setup.md` items (dedicated standard account, WHP, QEMU, a
   200 GiB cap with 50 GiB free, and a 16 GiB RAM / half-CPU reserve), plus
   an Ubuntu image. Brief 0020 says "never the owner's machine", so whether
   a guest on it qualifies needs review first. I recommend the existing
   hosted-runner pattern.

## Smallest work possible before any owner step

In order: S1, then S2 (one PR each, Sol builds, Opus reviews), then F0
(Opus or Sol builds, the other provider reviews, plus the Opus diagnostic
review). None activates anything.

## Known unknowns

- Whether hosted `ubuntu-24.04` meets the brief's host floor (systemd ≥ 254,
  kernel ≥ 5.14, cgroup v2, delegation).
- U9: whether systemd removes delegated directories at unit stop.
- Whether the release profile stays unwinding at activation (G-P2). Tests
  cannot show it.
- Every root cause above: `E_PROTECTION`, `ExecutionFailed` and the privacy
  flake.
