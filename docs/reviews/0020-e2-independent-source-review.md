# ADR 0020 E2 independent source review (fake-port setup and janitor)

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-13.
This is a single-agent, two-axis review with no delegates. It reviews the Sol
high (OpenAI / gpt-6.1-sol / high) E2 implementation and its root port. Opus
wrote the ADR 0020 architecture, so this review takes **no independent credit
for the architecture**. It gives independent credit only for the Sol
implementation, its tests and evidence, and the port.

## Verdicts

| Axis | Verdict |
| --- | --- |
| Standards | **Pass with advisories.** No documented-standard breach found. |
| Spec | **Changes required before publication.** Two small test-only blockers (B1, B2). The production driver behavior I checked conforms; no production change is required. |

Blockers: B1 and B2 below. Both are tests that the ADR requires but that are
missing or do not discriminate, and both were shown by compiled mutants that
survive. Neither finding shows wrong runtime behavior in `setup.rs`.

## Target identity and isolation

- Review checkout: a separate T3 worktree
  (`.../.t3/worktrees/Arxcovo/t3-4d7cac1f-...`), outside canonical
  `D:/Code/Arxcovo`. `pwd` and `git rev-parse --show-toplevel` both matched.
  Status was clean. HEAD was `8939188c8c842fa8ea07f25ab4f9790237d4aca3`, the
  root progress commit on top of port `af72d31` / `329e538`.
- Original target: evidence `a4deb6edb0b252333f18be2b671e68bdc5e0f5bb`,
  source `b40e81ab5e088e99db7f8790a630d3281fe7c2cf`, base
  `ab9ea634ff698397a6e98e9773905aa77b19ae5e`. `ab9ea63..b40e81a` is one commit
  that touches only `exec/hierarchy.rs`, the new `hierarchy/setup.rs` and the
  new `hierarchy/fake.rs`.
- Port verification:
  - `064113a^{tree}` equals `ab9ea63^{tree}` (`a3bec9bc...`), so the merged E5
    squash is the reviewed base.
  - Git blobs at `a4deb6e` and at HEAD are identical for all four E2 paths:
    `hierarchy.rs` `f37f9d0f`, `setup.rs` `b33bed7c`, `fake.rs` `536bad25` and
    `docs/evidence/linux-hierarchy-e2.md` `f6a4f28d`. These blobs and the
    manifest SHA-256 values also match the scratch copy
    (`setup.rs` `ab04270e...`, `fake.rs` `3110adb6...`).
  - `git diff b40e81a HEAD -- . ':!docs'` is empty (0 lines). Only
    `docs/PROGRESS.md` and the evidence file differ, and no crate reads
    `PROGRESS`.
  - The port therefore preserves the three source files and the evidence bytes
    exactly.
- Scope: no E3-E7 code and no activation. `setup` has no caller outside its
  module, `fake` is `#[cfg(test)]`, and production stays
  `MechanismUnavailable`.

Read: AGENTS.md, KICKOFF (governing rules), the review skill, accepted ADR 0020
(D1-D5, D9, D10, Bounds), the whole implementation brief (especially E2 H1-H20
and its mutation list), all three source files in full, the evidence and
source manifest, the E1 `parse.rs` API and bound tests, and the Stage B scanner
inventory.

## Standards

No hard breach of AGENTS.md or CONTRIBUTING was found. The commits are small
and conventional. The change contains no secrets and no private owner data:
the fake marker `ARX-STATE-SECRET` is synthetic, and the evidence names no
user paths or emails. Nothing is activated or published.

I ran a manual token audit of production `setup.rs` and `hierarchy.rs`. It
found no `unwrap`, `expect`, `panic!`, formatting, printing, `tracing`,
`set_hook`, `resume_unwind`, slice indexing, `unsafe`, `Command`, `std::fs`,
`std::env`, `/proc`, `/sys`, `kill(`, `killpg(` or `setpriority(`. The only
kill path is the `"cgroup.kill"` file write. `catch_unwind` matches `Err(_)`
and drops the payload unread.

Advisories (judgement calls, not blockers):

- **SA1 wildcard in a security mapping.** `SetupFailure::cause` ends with
  `_ => SetupCause::Setup` (setup.rs:118). A future variant would be silently
  classified as `Setup` (`mechanism_unavailable`) rather than forcing a
  decision. The brief already requires T15 in E6 to have no wildcard; use
  explicit arms here too.
- **SA2 stale module doc.** hierarchy.rs:1 still says "E1 is pure data parsing
  only." That is no longer true now that `setup` exists.
- **SA3 Primitive Obsession (tests).** Raw `d_type` numbers
  (`10, 2, 6, 1, 12, 8, 0`, fake.rs:544) and node ids (`dir == 1/3/5`) carry
  domain meaning. Named constants would make the H8 and H3 intent reviewable
  without decoding.
- **SA4 scanner coverage gap until E6.** The CI P-SCAN inventory
  (stage_b_tests.rs:490-498) does not yet include `hierarchy.rs`, `parse.rs` or
  `setup.rs`. The builder's standalone audit lives in an ignored
  `target/e2-audit.rs` and cannot be reproduced from the tree. My manual audit
  above is clean. E6 must add these files with the `kill(`, `killpg(` and
  `setpriority(` denylist and its mutant self-tests, as D10 requires.
  Until then, nothing in CI enforces payload privacy for E2. This is not a
  scanner bypass.

## Spec

### Verified conformant (independent)

- **Anchors before any write (D2, H10).** S0 hardening runs first, then T1-T8,
  and the first write is the S5 unit enable. H10 has 21 cases, each with its
  exact fixed failure and zero `write`/`mkdir`. H10b proves S0 runs before T1:
  no `identity` call appears in the log.
- **Top-down order (D3).** The operations run unit enable, then `mkdir runs`,
  then `runs/cgroup.kill`, then runs enable, then S9. H1 pins the four
  mutations in that order. A runs enable before the unit enable gets `EINVAL`
  from the fake's inheritance model.
- **Kill before list and wait (D4.1-2).** The one subtree kill precedes every
  listing. The wait polls the strict multi-key `events` parser, a malformed
  file gives `Postcondition`, the deadline is `KillTimeout`, and a manual clock
  is used. H3 and H4 cover this. An early `rmdir` is caught by the leaf
  `events` check and by the fake's populated `EBUSY`.
- **Validate all leaves before any removal (D4.4).** Two separate loops do the
  work: the first checks every name, trust and nested/refused listing, and the
  second removes. H6 places invalid names on both sides of a valid leaf in
  BTreeMap order, so interleaving checks with removals is caught.
- **Unknown or nested entries are never deleted, but their processes are
  killed.** H6 and H7 assert both `no_removal` and `!populated(runs)`.
- **Bounds include the dots (Bounds table, H9).** The fake lists `.` and `..`.
  512 records pass and 513 fail through every listing of `runs`. 65 children
  fail. A 4,097-byte control file fails at its own step. E1 separately pins the
  4,096/8,192 parser bounds with valid-shaped inputs.
- **Fresh postcondition listings (D4.6, S9).** The verify step and S9 each
  make their own `list` call. H18 (a child appears after the last `rmdir`), H19
  (population changes before verify) and H20 (a child or a refused entry at
  S9) each discriminate as intended: the skip-verify mutant fails with a
  different error.
- **Cancellation (D3).** Shutdown is checked before every step, on each loop
  iteration, and immediately before `Ready`. `Stopped` is never `Ready`, and
  the S5/S6 and S9/S10 boundaries are tested. See B1 for the wait-case gap.
- **Panics (D5, D10, H15).** The fixed literal appears exactly once, from the
  default hook, in a re-executed child of the current test binary only. The
  state canary is absent. My payload-printing mutant (H below) is killed.
- **Error classification for E2's subset of D5.** It matches the D5 table.
  `LeafStructure` and `KillInterface` map to `Setup`. `KillTimeout`,
  `LeafRemoval`, `VerifyFailed` and `Panicked` map to `Janitor`. `Stopped`
  maps to `Stopped`. Evidence holds only `u8` and `bool` values.
- **The fake models kernel semantics.** Child `cgroup.controllers` equals the
  parent's `subtree_control`. An enable write is all or nothing, with `EINVAL`
  on an unavailable token. Resident processes give `EBUSY`. Kill is recursive
  and ignores the frozen state. Kill takes effect only after N polls of the
  killed directory's `events`, or never. `rmdir` gives `EBUSY` while the
  directory is populated or has children. Listings include the control files
  and the dots. Faults can be injected per operation, a hook runs before each
  operation, and every operation is logged.

### Blockers (required before publication; test-only)

- **B1. The H14 kill-wait case does not test shutdown inside the wait.**
  - Requirement: brief H14 says "during the kill wait gives `Stopped` with no
    `rmdir`", and ADR D3 says the token is checked "inside every wait".
  - Problem: h14 uses the default `kill_delay = Some(2)`. The post-loop
    `stopped(token)?` (setup.rs:273) therefore also yields `Stopped`, so
    removing the in-loop check (setup.rs:263) survives the whole fake suite
    (mutant A).
  - Consequence: with a kill that never completes, that mutant turns shutdown
    into a 5 s logical wait that ends in `KillTimeout` (`Janitor` /
    `quarantined`) instead of `Stopped`.
  - Fix: set `kill_delay = None` in that case, and assert `Stopped`, one poll
    of clock advance and no `rmdir`. A scratch probe of exactly that test
    passes on the original and kills mutant A.
- **B2. Nothing tests that the walked unit is bound to the T2/T3 anchor.**
  - Problem: `setup` receives a caller `Location` and walks it, but takes its
    T2/T3 evidence from `proc_cgroup()`. The equality
    `located.unit_components() == location.unit_components()` (setup.rs:217)
    is the only link between them. Removing it survives the suite (mutant B).
  - Consequence: setup would then mutate a caller-supplied unit that the
    process's own cgroup file does not name.
  - Fix: add an H10 case with a valid but different `proc_cgroup`
    (`0::/system.slice/arxd.service/supervisor` against a supplied
    `/arxd.service/supervisor`). It must give `Locate` with zero
    writes/mkdir. A scratch probe passes on the original and kills mutant B.

### Advisories (not blocking)

- **A1 H1 "mkdir at 0700" is vacuous, and the evidence overclaims it.**
  `mkdir_runs(&Dir)` has no mode parameter (this matches the brief's trait),
  and every fake node defaults to `0o040700` (fake.rs:32). The H1 assertion
  `nodes[3].mode == 0o040700` therefore tests nothing, yet the evidence row for
  H1 claims "mkdir mode". The 0700 mode is an adapter obligation, and none of
  the E3 tests L1-L10 covers it. Correct the evidence wording, and add an E3
  owned-directory test of the created mode (or a mode argument on the port).
- **A2 Two `cause()` arms are untested in E2.** `LeafRemoval` and `Panicked`
  can be remapped to `Setup` without failing any test (mutants D and E). E6
  T15 is the designated exhaustive table, but `cause()` lives here. Assert all
  19 variants now (this pairs with SA1).
- **A3 The driver's defence-in-depth bounds are unexercised.** The checks
  `records.len() > RECORD_MAX` (setup.rs:162) and `bytes.len() > TEXT_MAX`
  (setup.rs:138) never fire, because the fake enforces both bounds itself and
  returns `TooBig` (mutant F survives). Add an over-returning fake mode, or
  document these checks as redundant guards.
- **A4 Some S9 checks are redundant in the fake.** The S9 `cgroup.procs`
  emptiness check is untested (mutant G survives); populated `false` already
  implies it in both the model and the kernel. The `pids.max` and individual
  `memory.max` presence checks are covered only by an injected `ro` fault on
  `memory.max`.
- **A5 The populated-leaf check is pinned only through the log.** Removing it
  (mutant C) is killed only by h3's log-position `unwrap` (fake.rs:494). No
  outcome test makes a leaf report populated after `runs` reported unpopulated.
- **A6 Fake fidelity notes.**
  - The fake rejects `-ctrl` tokens with `EINVAL`, but the kernel accepts
    disable tokens. H1's exact bytes and H11's no-`-` assertion still pin the
    contract.
  - The created mode is not modelled (see A1).
  - The 64-child pass boundary is not tested, so a `>=` off-by-one would
    survive. This is by inspection and was not run; the brief requires only
    the 65-child failure.
- **A7 The H15 child has no timeout.** `Command::output()` would block until
  the outer nextest timeout.
- **A8 A partial removal followed by shutdown reports `Stopped`.** Checking
  the token between removals is stricter than D3 requires. A restart reruns
  the janitor, so this is acceptable.

## Command credits

My runs, on Windows x86_64-pc-windows-msvc (cargo 1.95.0, nextest 0.9.151),
used an owned scratch extract (`git archive HEAD`) with a scratch
`CARGO_TARGET_DIR`. The review worktree was never mutated.

- `cargo nextest run --locked -p arxd -E 'test(exec::hierarchy) | test(service::unix_bridge) | test(exec::stage_b_tests)'`:
  **58 passed, 303 skipped**. No run ID was retained.
- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed
  (32.91 s).
- Mutants: one `setup.rs` mutation each, run with
  `cargo nextest run --locked -p arxd --no-fail-fast -E 'test(exec::hierarchy::fake)'`
  (23 tests). After every row, `setup.rs` was restored and its SHA-256 checked
  as `ab04270e...`.

| Mutant | Result |
| --- | --- |
| A: in-wait `stopped` removed | survived (B1) |
| B: location equality removed | survived (B2) |
| C: leaf `empty_events` removed | killed, h3 fake.rs:494 (A5) |
| D: `Panicked` mapped to `Setup` | survived (A2) |
| E: `LeafRemoval` mapped to `Setup` | survived (A2) |
| F: driver record-count check removed | survived (A3) |
| G: S9 `empty_procs` removed | survived (A4) |
| H: payload `downcast` + `eprintln!` | killed, h15 count assertion fake.rs:846 |

- Two probe tests were appended to the scratch `fake.rs` only, matching the B1
  and B2 fixes. Both passed on the original; probe 1 killed A and probe 2
  killed B. `fake.rs` was then restored (SHA-256 `3110adb6...`).
- Builder credits accepted as reported and **not re-executed by me**, all at
  the original pins `b40e81a`/`a4deb6e` on Windows:
  - fmt and Clippy;
  - focused 58 (`d4e43613-...`);
  - the 14-mutant ledger;
  - **one full 360 passed / 3 skipped** workspace run
    (`c3e5869b-b957-477b-a616-7f16cf1ce4da`).

  The full run is credited only at `b40e81a`. HEAD's non-doc bytes are
  identical to it, but no full run is claimed at HEAD.
- I did not run a full suite, because no new concern required one.
- Dispatcher privacy flake `65762554`
  (`service::executor::tests::adr0017_dispatch_durable_matrix`): not in my
  filters and not observed. It remains a separate **UNKNOWN**.

## Limitations

- This is a review of a fake port only.
  - The real descriptor behavior is not exercised here: no-follow, the root's
    trusted ancestors, fresh open file descriptions (L9), real entry kinds,
    the 4,096/8,192/2-byte reads and the created mode are E3.
  - Linux and macOS compile and runtime are pending, and hosted CI was not
    observed.
  - All Linux/native paths are **unverified on real hardware**.
- No native, filesystem-adapter, `/proc`, `/sys`, account, root, cgroup,
  service, owner, VM, experiment, private-state, ignored-fixture or native
  runner call was made. The only child process was H15's self re-execution.
  No toolchain was installed. No GitHub, push, PR, CI, watch or merge action
  was taken.
- M2 is incomplete. `E_PROTECTION`/`ExecutionFailed` remain unknown, with no
  retry. Main CI run `43479932347` was not observed.

**Summary:** Standards has 4 advisories and no blocker; the worst is SA4 (no
CI payload scanning of E2 until E6). Spec has 2 blockers and 8 advisories; the
worst is B1 (the H14 wait case does not discriminate, so an in-wait shutdown
regression would quarantine instead of stopping).
