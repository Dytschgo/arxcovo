# Inactive discovery pre-activation coverage follow-up: Opus review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. I was the only
review sub-agent and used no subagents. The review covers Standards and Spec,
including security, on separate axes. It is independent of the Sol (OpenAI /
gpt-6.1-sol / high) builder. It is not a GitHub approval and does not grant
publication or merge. It earns no architecture-independence credit for any
Opus ADR.

## Verdict

- **Standards: APPROVED.** 0 blockers, 2 advisories.
- **Spec and security: APPROVED.** 0 blockers, 4 advisories. Three
  pre-activation obligations stay **OPEN**: PA5, the real-Runner part of PA6,
  and a stronger PA3 proof (SP1).

Production stays inert. Nothing here grants activation, Stage F/G,
NativeRunner, or a CI, Cargo or policy change.

## Isolation and pins

- The review ran in an isolated T3 worktree outside the canonical checkout.
  Its cwd and Git top level were both that worktree, on branch
  `t3/review-discovery-coverage`. HEAD was `36f6952`, and status was clean
  before and after every run.
- Original target: `52dfcc8` (evidence) on source
  `99a1fd75f4ad578e55ed5196019cf5e3ecaa7733`. Its parent is `4eaad1d`, which
  is PR #64 head `9f44c0a` plus one PROGRESS line.
- Port: `e2bf3e6` (source) and `0e63830` (evidence) sit on merged E7 main
  `0a95d2806b197431ae7d7c9cc002fdaadaeb145d`. The tree of `0a95d28` equals
  the tree of `9f44c0a` (`5a6cf52`).
- **Byte identity, verified.** All six source files have the same Git blob at
  `99a1fd7` and at HEAD: `application.rs`, `unix_bridge.rs`, `tests.rs`,
  `tests/e6.rs`, `tests/e7.rs` and the mutation script. The patch from
  `4eaad1d` to `99a1fd7` touches exactly those six files. I recomputed the
  SHA-256 values for the script and `e7.rs`, and both match the evidence
  manifest. `52dfcc8` differs from `99a1fd7` only by adding the evidence file.
  `0e63830` adds the same file, with the owner worktree path redacted to the
  placeholder `<prepared-worktree>`.
- Inputs read: AGENTS, ADR 0020 D7, prior review `4728c3d` (PA1-PA7), and the
  PR #64 PASS comment 6866653579, which I read once through the API. That
  comment carries PA1-PA6 forward as pre-activation obligations.

## Production behavior and test-only seams

- `application.rs` adds 5 lines: three `discovery_barrier` calls inside
  `drain`. `drain_execution` and `drain_blocking` each carry their own
  `#[cfg(test)]`. `drain_sequence` sits inside the existing `#[cfg(test)] if
  let Some(task)` block. `Hooks::discovery_barrier` is itself `#[cfg(test)]`
  with an empty default. Release builds are unchanged.
- `unix_bridge.rs` adds 2 lines: a `readiness` field and its clone on
  `StartupDomain`, which is `#[cfg(test)]`. Production is unchanged.
- No Cargo, lockfile, CI, policy or AGENTS change. No NativeRunner, no
  production `start_discovery` override, and no Stage F/G wiring.
  `git diff --check 0a95d28 HEAD` exits 0. The diff contains no private paths,
  addresses or tokens.

## Standards

0 blockers. Advisories are judgement calls:

- **SA1 (possible Data Clumps).** The `app()` fixture now returns a five-tuple
  `(App, Arc<Activity>, Arc<Execution>, Arc<AtomicUsize>, Readiness)`. Four
  call sites destructure it by position with `_` placeholders. A small named
  fixture struct would make the PA2 readiness handle self-describing and stop
  future positional drift.
- **SA2 (carried from SA-b).** The new PROGRESS lines repeat the compressed
  spacing, for example "focused49" and "final386/3". This is cosmetic only.

The seams follow the established E6/E7 pattern: `cfg(test)` hooks, the
existing `Hooks` callback, and the T10 isolated child. The PA6 helper is
`pub(super)` in `e6.rs` and only the T10 fixture calls it. The mutation
script follows the precedent of `test_discovery_e7_mutations.py`. It requires
a unique anchor, counts only compiled named-test failures as kills, writes LF
explicitly, and restores sources with a SHA-256 check in `finally`. The
committed script and evidence contain no CR bytes.

## Spec and security

Each PA item is checked against `4728c3d`.

- **PA1: closed.** `pa1_actual_callers_reverse_finish` drives the real
  startup task and the real rmcp `discover_tools`/`list_hosts` handlers over
  duplex IO. It never calls `discovery_observation`, and no Activity ticket
  is placed by hand.
  - Case A (`startup_first`): startup's port call is held while a later MCP
    call finishes. The ticket is recorded before the gate opens. Startup's
    later completion must not replace it. The tools differ by synthetic
    index, and `list_hosts` returns the newer tools.
  - Case B: completed startup creates a cache, which a later MCP call
    replaces. Two overlapping MCP calls finish in reverse order. This also
    meets the old W7 "replace an existing cache" request.
  - Port counts are exact (2 and 3). `done()` joins the startup task, and
    `cache_discovery` does not consult the stop token, so the startup-path
    kill is deterministic.
- **PA2: closed.** The test observes the actual `Readiness` handle:
  - `wait`, `s9` and `s10` give `Err(Stopping)`.
  - `ready` stays `Ok(())`. This is a useful control: it shows that
    `Stopping` comes from `fail(Stopped)`, not from executor shutdown.
  - The new `s10` seam runs after the fake setup returns Ready and before the
    cancellation guard. That kills unconditional `set_ready`.
  - The evidence correctly makes no rollback claim for the already-Ready
    case.
- **PA3: closed for the named mutant.** The W1 log asserts
  `drain_execution`, then `drain_sequence`, then `drain_blocking`. Moving the
  sequence block, label included, is killed. See SP1 for the limit.
- **PA4: closed.** The W1 factory asserts `owner.cgroup_parent ==
  Some(location.runs())`. Setup asserts the same `unit_components` captured
  from the one parsed `Location`. Both run on that single `Location`, and no
  path is opened.
- **PA5: OPEN, not implemented, correctly disclosed.** A failed `Locate` must
  still construct a refused bridge with readiness `Unavailable(Setup)`, and
  the constant bridge `check` must be replaced before activation.
- **PA6: partial, honestly scoped.**
  - Proven, with a fake port in the purpose-built T10 child under both
    backtrace modes: the panic returns an honest `failed` result with
    `cleanup_confirmed` false and a `cleanup_unconfirmed` row. A direct
    `b.run` hits the in-slot residue gate and latches `MechanismUnavailable`.
    The latch survives cleared residue. Four `Quarantined` refusals follow,
    and port calls stay at 1. Activity withholds the payload. The parent
    asserts exactly one `ARX-PA6-FIXED-PANIC` marker and never prints raw
    child output.
  - **OPEN:** the real Runner storing residue before releasing its slot
    during unwind. A fake that sets its own flag cannot prove that temporal
    property, and the evidence says so without overclaiming.

Advisories:

- **SP1: the PA3 proof pins label position, not drain execution.** My
  compiled mutant `exec_drain_after_sequence_hook_kept` leaves the
  `drain_execution` label in place but moves `Execution::drain()` after the
  sequence join. It **survived** all 9 E7 tests. The evidence wording "logs
  entry to the actual execution drain" is accurate but narrow. Before
  activation, observe the effect instead. For example, at the
  `drain_sequence` barrier, assert that Execution has already drained (zero
  workers, or refusing). This is not a blocker: source order matches D7, and
  the obligation I named in `4728c3d` is killed.
- **SP2: W3e is still unasserted.** "Leaves the executor set empty" is still
  only implied by a clean drain. It was the second half of the old PA2, and
  the evidence does not claim it. Carry it with SP1.
- **SP3: PA-b from the PR #64 comment is unaffected.** Once cached,
  `list_hosts` still reports `tool_discovery: "unavailable"`, and
  `tools_observed_at` is the start time. Activation must state this.
- **SP4: PROGRESS summary.** "PA1-PA4 tests added; PA5 and real Runner
  unwind ... remain open" is accurate. It omits that a partial PA6 fake-port
  test was added. Optional wording only.

## Credits and limits

Builder runs at `99a1fd7` are credited by the verified byte identity above:

- fmt
- workspace Clippy `-D warnings`
- focused 49 passed, 1 ignored
- seven compiled named kills
- one final normal `cargo nextest run --workspace --all-targets --locked`:
  **386 passed, 3 skipped**, no retries

The failure history is plausible and honest:

- the `Location` clone compile error was not counted as a kill
- the W1 slice and the PA6 Err/Value expectation were corrected
- a reverted E6 text replacement and a Unicode artifact
- one failed direct launch of the ignored fixture (WinError 2) that started
  no process and was not repeated

Earlier credits stay where they were earned: 383/3 at `cea3038` and seven LF
kills at `09cc061`.

My runs, at HEAD `36f6952`, Windows x86_64-pc-windows-msvc:

- `cargo test -p arxd --lib --locked -- service::unix_bridge::tests::e7 service::unix_bridge::tests::t10_reexec_diagnostics`:
  10 passed, 1.41 s.
- `python scripts/test_discovery_followup_mutations.py` (committed, run
  once): 7 of 7 COMPILED KILLED, then EXACT RESTORE. An independent
  `sha256sum -c` over `application.rs`, `mcp.rs`, `unix_bridge.rs` and
  `tests.rs` passed.
- Reviewer counterfactual script, kept in the session scratchpad and not the
  repository, with exact restore and `sha256sum -c` OK:
  - `control_pa6_removed`: the T10 parent passes. The control is valid.
  - `panic_unknown_without_pa6` and `residue_omitted_without_pa6` both
    **survive** once the PA6 case is removed. So the new PA6 case is the sole
    killer of `panic_claims_no_creation` and `residue_gate_omitted` in T10,
    and kill attribution holds.
  - `setup_other_location`: killed, by 5 E7 tests. Tests that already
    existed also catch it, so the bridge-parent assertion is the new PA4
    value.
  - `exec_drain_after_sequence_hook_kept`: survived (SP1).
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: clean. This
  is package-scoped, not the workspace.
- I ran no full workspace suite. The bytes are identical to the credited pin,
  so no material concern called for one.

Limits:

- A Windows run does not compile or run `cfg(target_os = "linux")` code. All
  ports here are fakes. Nothing establishes Linux or macOS behavior, and this
  is not hosted CI.
- M2 is incomplete. Native, installed and dedicated paths are **unverified on
  real hardware**.
- The historical dispatcher privacy flake, `E_PROTECTION` and
  `ExecutionFailed` remain unknown. No fix, diagnosis or retry is claimed.
- No NativeRunner, owner, session, account, service, `/proc`, `/sys`,
  cgroup, root, VM, private-state, experiment, ignored-fixture, installation,
  GitHub push/PR/CI-watch/merge/retry or settings action was taken.
- No product or PROGRESS edit. This file is the only change.

## Summary

Standards: 0 blockers, 2 advisories. The worst is SA1, the positional
five-tuple fixture. Spec: 0 blockers, 4 advisories. The worst is SP1: the
PA3 drain-order test pins hook labels rather than drain execution, and a
reviewer mutant survived. PA5 and the real-Runner unwind store-before-release
proof remain OPEN pre-activation gates.
