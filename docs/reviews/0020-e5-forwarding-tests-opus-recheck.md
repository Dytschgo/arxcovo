# ADR 0020 Slice E5 tests-only correction: independent Opus recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. Review sub-agent,
no subagents. This is a narrow recheck of the Sol medium tests-only
correction. It covers only whether S1 and B1 from the first review are closed
(`4e1d6b8`, kept at `7ab6f3a` in
`docs/reviews/0020-e5-independent-security-review.md`). I wrote that first
review, so this recheck is independent only of the Sol correction.

**Standards: APPROVED.** S1 is closed. `exec/tests.rs` and the new
`exec/residue_tests.rs` are back under the `cargo fmt --all --check` gate.
A misformatted probe in either file now makes the check exit 1. No new
Standards blocker. Two advisories follow.

**Spec: CHANGES REQUESTED.** B1 as written is closed. Both B1 mutants and the
PA3 reset mutant now fail the portable pin. One hop of the same flag path is
still unpinned: inside `Group::create`, the flag stored in the returned
`Group`. That is the flag `Group::drop` signals after a run. Two compiled
mutants there survive (B2 below). By source reading, Linux R1/R2 and Linux
Clippy would not catch them either. B2 has a one-assertion, tests-only
repair, and I checked in scratch that the assertion kills both mutants. No
production byte needs to change.

This recheck grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance.

## Pins and isolation

- Review target `da1762dcbd60b649d44e77eb8060ec6a539c3d37` (evidence). Source
  correction `c46304e95d043b8b2b82937ace6ffb5c9304c224`, parent `7ab6f3a`.
  The original E5 source is `f866af45a31779abc30d7674a07aaa943dee940a`, with
  evidence `70aa4b0`. `019ff5f` has the same tree as `f866af4`.
- Checkout: the prepared T3 worktree. Its top level is not the canonical
  checkout, and its status was clean. HEAD is coordinator progress commit
  `de599f5`, which descends from the target and changes only
  `docs/PROGRESS.md`. Builds and mutants ran in a private `git archive`
  export of `da1762d` in my scratch area, with a separate `CARGO_TARGET_DIR`.
  I created and switched no checkout, and the canonical `D:/Code/Arxcovo` was
  not touched.
- Scope: `git diff --name-only f866af4 c46304e` lists exactly
  `exec/mod.rs`, the new `exec/residue_tests.rs`, the E5 evidence and the
  first review. `git diff --check` exits 0.
- Production identity from `f866af4` to `c46304e`, `da1762d` and `de599f5`:
  - The `unix.rs` blob `0d087e3`, the `cgroup.rs` blob `d85544b` and the
    `tests.rs` blob `c2b4130` are unchanged at all four commits. `tests.rs`
    also equals the pre-E5 base `3875466`.
  - The text of `mod.rs` before its first `#[cfg(test)]` (line 365) has the
    same SHA-256 at `f866af4` and `c46304e`: `d7d5d869…c345`.
  - The `mod.rs` diff touches only the test-module declarations after
    `mod disposable;`.
  - New blobs: `mod.rs` `7d63eee9ae4c78d5f963b92976bd1f7ba22c338c` and
    `residue_tests.rs` `b8bfa8b5589399d4cba8f66a9a712929d5038edd`.
- Read: AGENTS, the first review, and the E5 evidence (manifest and the new
  correction section). I also read the source of `mod.rs`, `unix.rs`,
  `cgroup.rs` (create, drop and R1/R2), `residue_tests.rs`, and the E1
  `mod.rs` delta on main `2fbc647`. For KICKOFF, ADR 0020 and the brief, I
  relied on the clauses quoted in the first review. Their bytes are unchanged
  from that review, and this narrow recheck adds no new clause.

## Standards

### S1 closure: verified

- `#[cfg(test)] mod tests;` is out-of-line again. The two E5 tests moved
  verbatim into `#[cfg(test)] mod residue_tests;`. The only edit is that
  rustfmt rewrapped one `assert!`. As a child of `exec`, the module can
  still read private `Runner` fields and `super::tests::config()`.
- No inline `mod tests {` remains in `mod.rs`. The Stage B scanner tests
  (`p_scan_*`) pass.
- Format negative control in the export: I appended
  `fn   fmt_probe( ) {   let x=1 ;let _=x; }` to `tests.rs`, and
  `cargo fmt --all --check` exited **1**. The same probe in
  `residue_tests.rs` also exited **1**. Both files were restored, their blob
  hashes re-verified, and fmt then exited 0. At `f866af4`, the first review
  saw exit 0 for the same `tests.rs` probe.

### Advisories

- **SA1 (carried, non-blocking).** The cgroup pins still match exact
  newline-and-indent literals, which is brittle under rustfmt changes. A
  mismatch fails closed. The new checks use counts and a
  whitespace-stripped `store(false,` scan, which is more robust.
- **SA3 Evidence manifest not refreshed.** The manifest table in
  `linux-residue-e5.md` still lists `mod.rs` as `fad2ef7` and has no
  `residue_tests.rs` row. The correction section describes the move in prose
  but gives neither new blob. At the re-pin, add rows for `mod.rs`
  `7d63eee` (or its re-pinned successor) and `residue_tests.rs` `b8bfa8b`.
  The first review asked for that manifest entry.
- The test-module declarations are reordered (`stage_b_tests`,
  `residue_tests`, `tests`) instead of the base order. This is cosmetic and
  rustfmt accepts it.

## Spec

### B1 closure: verified for the stated path

The pin added in `residue_tests.rs` runs over production slices (text before
the first `#[cfg(test)]`). It requires:

- exactly one `self.residue.clone()` in `mod.rs` (line 85);
- exactly one `Arc::new(AtomicBool::new(false))` in `mod.rs`;
- exactly two `residue` tokens in production `unix.rs` (line 91), plus the
  existing `Group::create(c, residue)` substring;
- no whitespace-insensitive `.store(false,` in production `mod.rs`,
  `unix.rs` or `cgroup.rs` (line 94).

The mutants in the table below kill on these lines.

If someone later adds a `#[cfg(test)]` item above `Runner`, the slice
shrinks and the pin fails closed (a false alarm, not a fail-open). A
deliberate evasion would still pass, for example a dead `let _ =
self.residue.clone();` next to an `Arc::default()` argument, or
`swap(false)` / `fetch_and(false)`. A source pin catches accidental
decoupling, not adversarial edits. That limit is accepted.

### Blocking

**B2. Nothing pins the flag that `Group::create` stores in the `Group`.**
`create` uses the `residue` parameter in two ways. It borrows the parameter
for both `release_leaf` calls, and it moves the parameter into
`Ok(Self { …, residue })`. The pin checks the borrows but not the move.

- Compiled mutant **C1** replaces that field with
  `residue: Arc::new(AtomicBool::new(false))`.
- Compiled mutant **C2** replaces it with `residue: Arc::default()`.

Both pass every focused test: exit 0, 17/17. By source reading:

- Linux R1/R2 build `Group` as struct literals, or call `release_leaf`
  directly. They never call `create`.
- The only other caller of `Group::create` is `unix.rs:220`. The ignored
  `disposable` tests never read `cleanup_residue()`.
- The parameter is still used by `release_leaf`, so neither rustc nor Clippy
  would warn.

This is *source inference*: no Linux build or run happened here. With either
mutant, the after-run "kill unconfirmed" signal from `Group::drop` goes into
a dropped flag. That is the only residue path after spawn. The create-failure
path still signals. `cleanup_residue()` would then miss residue from row-H
style runs, and the D6 gate and latch would fail open for that case. B1
described the same failure. The first review's repair step 2 asked for "no
fresh `Arc::new(AtomicBool::new(false))` in production `cgroup.rs`", which
would have caught C1. The correction applied that check to `mod.rs` only. I
also did not name this hop explicitly in the first review.

**Repair (tests only).** Add one assertion to
`cleanup_source_pins_release_sites_and_legacy_errno_order`, using the
existing `create` slice:

```rust
let create_compact: String = create.split_whitespace().collect();
assert_eq!(
    create_compact
        .matches("removed:std::cell::Cell::new(false),residue,}")
        .count(),
    1
);
```

I checked this in the export. With the assertion, the baseline passed (2/2
`residue_tests`). C1 and C2 each failed at that assertion: exit 100 in both
cases. All bytes were then restored. A whitespace-insensitive ban on
`Arc::new(`, `Arc::default` and `AtomicBool::new(` in production
`cgroup.rs`/`unix.rs` would be a reasonable second guard, but the assertion
alone kills both mutants.

### Verified unchanged (source reading)

No production behaviour differs from `f866af4` (see the byte identity above).
The forwarding chain I read is: `Runner::new` mints one flag, then
`run_core` passes `#[cfg(target_os = "linux")] self.residue.clone()`, then
the `unix::run` parameter goes to `Group::create(c, residue)`, which stores
it in the `Group`. The only stores are `true`/`Release` in `release_leaf` and
in `Group::drop`. The only load is `Acquire` in `cleanup_residue`. The first
review's verified items still hold. On main, `2fbc647` (E1) adds only
`pub(crate) mod hierarchy;` after the `use std` block. It touches no residue
token and no `#[cfg(test)]` line. The pin counts should therefore survive a
re-pin (*source inference*). Hunk adjacency to the E5 `use` addition may
still need a manual merge.

## Commands and results (this recheck)

All commands ran on Windows `x86_64-pc-windows-msvc`, rustc 1.95.0
`59807616e`, cargo-nextest 0.9.151, in the private export. Logs stay local.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `cargo nextest run --locked -p arxd -E 'test(exec::)'` | exit 0; 17 passed, 310 skipped; run `72f8668b-208a-41b4-8819-0e69d1ab2e2f` |
| `cargo clippy --locked -p arxd --all-targets -- -D warnings` | exit 0 (Windows targets only; I did not repeat the workspace-wide run) |
| fmt probe in `tests.rs` / `residue_tests.rs` | exit 1 / exit 1; restored |

Each mutant ran the focused `exec::` filter with `--no-fail-fast`. Each file
was then restored from its `da1762d` blob and its hash re-verified. At the
end, every `exec/*.rs` file in the export matched `da1762d`.

| Mutant | Change | Result |
| --- | --- | --- |
| S1-mut | `run_core` passes `Arc::new(AtomicBool::new(false))` | **killed**, exit 100 at `residue_tests.rs:85` (run `26c7bd36`) |
| S2-mut | `unix::run` shadows `residue` with a fresh flag before `Group::create` | **killed**, exit 100 at `:91` (run `3aed2d39`) |
| S3-mut | `release_leaf` adds `else { residue.store( false , …) }` | **killed**, exit 100 at `:94` (run `6a1184fe`) |
| K1 | `Drop` condition becomes `!kill_and_confirm() && false` | **killed**, exit 100 (run `5a715797`) |
| C1 | `Group` field `residue: Arc::new(AtomicBool::new(false))` | **survived**, exit 0, 17/17 (run `c35cebe8`) — B2 |
| C2 | `Group` field `residue: Arc::default()` | **survived**, exit 0, 17/17 (run `0c43d984`) — B2 |
| C1 + proposed assertion | — | **killed**, exit 100 (run `6a0224f1`) |
| C2 + proposed assertion | — | **killed**, exit 100 (run `c96b11a0`); baseline with the assertion passed (run `83fde411`) |

## Credit and limits

- **Credited to `c46304e`/`da1762d`:**
  - Standards S1 closure.
  - Spec B1 closure for `run_core → unix::run → Group::create(c, residue)`.
  - The PA3 portable reset ban.
  - The surviving K1 kill.
  - Windows fmt, Windows Clippy for `arxd` all-targets, and the 17/17
    focused run.
  - The root-reported runs (`c40ef903`, `2d5a2342`, `9c1f6532`, the
    workspace Clippy and the misformat control) are consistent with what I
    reproduced. I did not rerun them by ID.
- These are **source-pin and formatting proofs, not Linux runtime proofs.**
  Linux R1/R2 execution, Linux and macOS compilation, and Linux Clippy all
  remain outstanding until hosted CI runs. A local Linux typecheck was not
  attempted, because the first review already found it blocked by a missing
  cross C compiler.
- No real cgroup, root, native, owner, service, account, VM, Lab Manager,
  experiment, private-state or ignored-fixture action. No full workspace
  test run. No PROGRESS or product edit. No GitHub write, push, PR, watch,
  merge, settings change or retry.
- E5 publication still needs:
  - the B2 test repair and a narrow recheck of it;
  - a re-pin onto main `2fbc647`, which carries E1's `mod.rs` addition;
  - independent integration review on the merged base;
  - all hosted gates.
- M2 is incomplete. Native, installed and dedicated paths remain
  **unverified on real hardware**. `E_PROTECTION` and `ExecutionFailed`
  remain unknown and unresolved, with no fix or retry.

## Summary

Standards: **APPROVED**. S1 is closed, with two advisories (SA1 carried,
SA3 manifest refresh). Spec: **CHANGES REQUESTED**. B1 is closed for its
stated path. B2 remains: the `Group` field hop inside `Group::create` is
unpinned, and C1/C2 survive. The repair is one tests-only assertion, already
shown in scratch to kill both mutants.
