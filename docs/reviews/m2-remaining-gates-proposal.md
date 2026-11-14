# M2 remaining gates: proposed next slices (design only)

Author: Anthropic / claude-opus-5-5 / high, design sub-agent, 2026-11-14.
Revisions: OpenAI / gpt-6.1-sol / medium, `a7b98df` and the subsequent
SP-R1 correction; original architecture authorship remains with Opus.
Status: **accepted by coordinator for bounded local S1, S2a, S2b and F0
preparation, 2026-11-14.** Original Opus design received independent Sol high
review `9b62dd2` (retained `8b8fca3`). Sol revisions address that review and
Opus correction review `ae05533` (retained `f757b3a`); final correction
`1a173a9` passed Opus high recheck `29e2d03` (retained `b4f78a4`).
Coordinator: OpenAI / gpt-6.1-sol / medium. Each implementation still needs
independent source review, current-base integration and green required checks.
No dispatch, historical retry, native acceptance, activation or owner step is
authorized. A13 remains proposed and real-Runner PA6 stays OPEN. Original
Opus architecture authorship is retained without independent Opus credit.

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
  main at merged PR66 `1ac4047`. PR66 passed independent source review
  (`414caaa`) and all five required checks before merging. Main-after-PR66
  run `43527940656` passed attempt 1 in 624s. Production is
  `MechanismUnavailable`:
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
   an owned `Fn() -> Result<(), Unavailable> + Send + Sync + 'static`
   closure invoking `execution_preflight(&layout, &FakeResolver)` on every
   preflight, using the existing test resolver, plus a refusing closure.
   Do not pass an eagerly evaluated Result or a NativeResolver. A wiring pin
   forbids a literal `|| Ok(())` check in `StartupDomain`.

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
- **L-b2.** Missing location plus a refusing live check returns that check's
  exact error token, distinct from `MechanismUnavailable`. This proves
  only that the live check runs first, with zero factory/setup/Runner calls;
  identical later refusal tokens do not prove runner/readiness order.
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

Sequence is mandatory: **S2a** implements the D6 panic latch described below,
with its own meaningful tests, independent source review and green required
checks. It merges first as a separate inert correction PR. **S2b** is the
local decomposition/gate proof PR, re-pinned onto merged S2a. U-a's latch
assertion and U-b's post-panic early refusal depend on S2a; neither is claimed
against the original catch arm. No production constructor is introduced.

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
- **U-b.** Keep three distinct assertions with `limit = 1`:
  1. An ordinary request after the panic latch refuses in preflight.
  2. Before the panic, hold a pre-admitted request at the existing hook,
     after quarantine checking and before slot acquisition. Release it only
     after the panicking call has returned **and its slot is observed free**,
     with residue published and readiness latched. Observe entry into the
     actual in-slot readiness refusal and zero Runner calls. Define
     `in_slot_readiness_refusal_omitted` to remove that readiness refusal,
     not merely the preceding residue check. It must fail this assertion.
     Early preflight, row-G quarantine or `Busy` earns no gate credit.
  3. Independently test the residue-check step with readiness initially
     Ready and residue published while the request is held at the same
     pre-slot hook. This separate controlled case has no panic latch or
     quarantined prior dispatch. After observing the slot free, release the
     request; require the actual residue check to latch Containment and
     refuse with zero Runner calls. Removing only that check
     (`in_slot_residue_check_omitted`) must fail the named assertion.
  Gate evidence relies on the held position, zero Runner calls and absence
  of Busy/Stopping, never the shared `MechanismUnavailable` token alone.
  No assertion may reset readiness, quarantine or the monotone flag. Tests
  must distinguish gate entry and refusal from any unrelated early refusal.
- **U-c.** A source-inventory test with a mutant control. In `unix.rs`,
  `Group` appears only as the local `group` and the `ChildGuard` field.
  There is no `forget`, `ManuallyDrop`, `Box::leak`, `Arc<Group>` or thread
  move of either. In `executor.rs`, `fetch_sub` follows `backend.run` in
  the same closure. This is a lexical guard (as E7 PA1 discloses), not a
  proof.
- **Mutants:** `drop_skips_store`, `catch_outside_slot` (moves `fetch_sub`
  before `backend.run` returns), `group_forgotten` and
  `in_slot_readiness_refusal_omitted` and
  `in_slot_residue_check_omitted`. U-a remains real-Group Drop/bridge
  decomposition evidence; U-c is only a partial lexical guard. Neither
  earns real-Runner, root or cgroupfs evidence.

**D6 disposition proposed for separate implementation review.** On unwind,
`body` exits before its post-run latch (`unix_bridge.rs:94-103`), and the catch arm
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
plus A13 suffices remains subject to a separately reviewed native design
and accepted evidence. Real-Runner PA6 remains OPEN after S2.

Acceptance is the same as S1, and U-a/U-b run on hosted Linux only.

## Stage F: local feedback artifact; any dispatch remains unauthorized

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
- The record has a fixed schema/version, reviewed source and workflow SHAs,
  and run/attempt association in an envelope **inside the same single
  validated 4 KiB JSON line**, not a separate artifact. Bound predecessor count
  and total input as well as the 4 KiB output; reject overflow and malformed
  records rather than truncate them into replay evidence.
- The record has one schema-validated JSON line of at most 4 KiB. It
  contains no path, uid number, name, argv, environment or payload. The
  workflow prints only that validated line.
- **Pure replay loop:** factor the predicate as `decide(triples) ->
  Option[(ordinal, conjunct)]`. A unit test feeds a recorded line into the
  real `decide` and requires the recorded refusal. That makes it
  red-capable once a real line exists. Until then, tests on synthetic
  triples are labelled mechanism tests, not reproduction.
- Tests exercise the real predicate, both initial/final exception orderings,
  hostile payload suppression and every bound. Synthetic tests remain
  mechanism evidence until a captured refusal reproduces through the real
  predicate; no historical red replay exists.
- Known unknown for F0 review: whether the unchanged driver and
  `disposable.rs` still match the post-0019/0020 Runner contract.

**F1.** An independent review of F0: Opus high, which the phase-1 report
requires before any diagnostic proposal runs, and Sol for this
architecture. F0 merges through the normal gates.

**F2 (no dispatch authorized).** A new SHA does not discharge ADR 0019
D7's no-retry restriction or the diagnostic evidence gap. Root must
explicitly disposition both before proposing any observation or experiment.
Any such proposal needs its own reviewed scope, source pins, fresh disposable
environment, technical fences and owner-only dispatch. Diagnostic observation
is not A1-A13 acceptance. F0 artifact/replay comes before hypotheses or fixes;
a fresh captured refusal must reproduce through the real predicate before
diagnosis. Unknown or unconfirmed cleanup permanently excludes that
environment. This proposal authorizes no historical retry or new dispatch.

## Stage G (out of scope here)

Stage G is a separate architecture change, with review and all checks. It
needs ADR 0019 D7 and C2 (G-P1..G-P3), ADR 0020 D10, accepted Stage F
native evidence (A1-A13), and readiness and hardening rendered in
`capability()`. Production stays `MechanismUnavailable` until then. This
proposal adds no approval flow and does not widen agent or user authority.

## Owner decisions and actions actually needed

None are needed for S1, S2 or F0. These are needed later:

1. **Any later observation/experiment.** An explicit owner decision must
   first resolve the no-retry restriction for a separately reviewed bounded
   scope; no request or preapproval is made here. Existing technical fences
   require owner actor and triggering actor, main ref, matching reviewed
   source/workflow SHAs and `UNIX_EXECUTION_REVIEWED_SHA`. Only the owner
   may set that variable, dispatch and clear it after all applicable gates.
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

In order: S1, then S2a, then S2b (one PR each, Sol builds, Opus reviews), then F0
(Opus or Sol builds, the other provider reviews, plus the Opus diagnostic
review). None activates anything.

Failure accounting is unchanged across S1, S2, F0 and every new SHA: three
failed fixes require other-provider handoff with failure evidence, and
KICKOFF's both-provider stop condition remains. No slice resets counters or
waives a gate. Prior test/CI credits stay at their original pins. The drain
waiting, autonomous-release and inspection-only seam advisories remain;
they earn no new hardware credit and authorize no parked-work retry.

## Known unknowns

- Whether hosted `ubuntu-24.04` meets the brief's host floor (systemd ≥ 254,
  kernel ≥ 5.14, cgroup v2, delegation).
- U9: whether systemd removes delegated directories at unit stop.
- Whether the release profile stays unwinding at activation (G-P2). Tests
  cannot show it.
- Every root cause above: `E_PROTECTION`, `ExecutionFailed` and the privacy
  flake.
