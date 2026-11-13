# ADR 0020 Slice E5 B2 returned-Group flag repair: independent Opus recheck

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. Review sub-agent,
no subagents. This narrow recheck covers only the Sol medium tests-only B2
repair: the 7-line `create_compact` assertion in
`crates/arxd/src/exec/residue_tests.rs`. The B2 finding came from my earlier
recheck (`2c47ec9`, kept at `011332d` as
`docs/reviews/0020-e5-forwarding-tests-opus-recheck.md`). This recheck is
independent only of the Sol repair.

**Standards: APPROVED.** The change is one small conventional `test(exec)`
commit that touches only the test file. fmt and `arxd` all-targets Clippy pass
on Windows. The SA3 manifest refresh is closed. No new blocker.

**Spec: APPROVED (narrow B2 scope).** B2 is closed as specified. The
assertion is byte-for-byte the repair I proposed. It runs on the production
`create` slice and pins the returned `Group` field to the incoming `residue`
binding. Both B2 mutants, C1 and C2, fail at the new assertion with exit 100.
Production bytes are unchanged from `da1762d`, and also from `f866af4` apart
from the test-module declarations reviewed earlier. One advisory remains on
residual evasion (SA4).

This recheck grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance.

## Pins and isolation

- Checkout: the prepared T3 worktree. Its top level is
  `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-f60429a6-4694eaa67b495117`,
  not the canonical `D:/Code/Arxcovo`. HEAD was
  `8fb7c84f80220d82f4e2825808b72ee920c2bde4` and the status was clean.
- Target: evidence commit `8fb7c84`. Source/test commit
  `e36f4fdc0fa877200ca7aec5c1c79331debe5dea` has parent `011332d`, which
  keeps prior report `2c47ec9`.
- Scope: `git diff --name-only da1762d 8fb7c84` lists exactly
  `exec/residue_tests.rs`, `docs/PROGRESS.md`,
  `docs/evidence/linux-residue-e5.md` and the prior recheck report.
  `git diff --check` exits 0. `e36f4fd` is `+7/-0` in `residue_tests.rs`
  only.
- Read: AGENTS, the prior recheck, the E5 evidence (including the new B2
  section and manifest), `e36f4fd`, and the `cgroup.rs` source for the
  `Group` struct, `create`, `release_leaf` and `Drop`.
- I ran builds and mutants in a private `git archive` export of `8fb7c84` in
  my scratch area, with a separate `CARGO_TARGET_DIR`. I created and switched
  no checkout. At the end, every `exec/*.rs` file in the export matched its
  `8fb7c84` blob.

### Production identity

| Path | `f866af4` | `da1762d` | `8fb7c84` |
| --- | --- | --- | --- |
| `exec/unix.rs` | `0d087e3` | `0d087e3` | `0d087e3` |
| `exec/cgroup.rs` | `d85544b` | `d85544b` | `d85544b` |
| `exec/tests.rs` | `c2b4130` | `c2b4130` | `c2b4130` |
| `exec/mod.rs` | `fad2ef7` | `7d63eee` | `7d63eee` |
| `exec/residue_tests.rs` | — | `b8bfa8b` | `91c5251` |

- Lines 1–365 of `mod.rs` (through `mod disposable;`) hash to the same
  SHA-256, `fa87babd…b198`, at `f866af4` and `8fb7c84`. The `mod.rs` delta
  from `f866af4` is the `#[cfg(test)]` module move that the prior recheck
  already credited.
- I checked the evidence manifest rows (`linux-residue-e5.md`, "Current local
  manifest") against `git rev-parse` and `sha256sum` of the committed bytes at
  `8fb7c84`. All four match: `mod.rs` `b6cc7f6b…a4a2`, `residue_tests.rs`
  `8f3af428…8af8`, `unix.rs` `500a914a…f0b7`, `cgroup.rs` `039a2f9c…f5c6c`.

## Standards

- **Format and lint.** `cargo fmt --all --check` exits 0.
  `cargo clippy --locked -p arxd --all-targets -- -D warnings` exits 0. Both
  ran on Windows. I did not repeat the workspace-wide run.
- **Style.** The assertion uses the count-based, whitespace-stripped style
  that SA1 recommended, so a rustfmt rewrap of the struct literal cannot
  break it.
- **SA3 closed.** The evidence now has `mod.rs` and `residue_tests.rs`
  manifest rows with blobs and committed-byte SHA-256 values. I verified them
  above. A re-pin onto main will need new rows.
- **SA1 (carried, non-blocking).** The older newline-and-indent literals in
  the same test remain brittle under rustfmt changes. They fail closed.
- **Advisory, cosmetic.** The new literal also contains
  `removed:std::cell::Cell::new(false),`. If someone imports `Cell`, reorders
  the fields or drops the trailing comma, the pin raises a false alarm. It
  fails closed, so it is acceptable.

## Spec

### B2 closure: verified

`Group::create` (`cgroup.rs:45`) ends with
`Ok(Self { …, removed: std::cell::Cell::new(false), residue, })` at
`cgroup.rs:110-117`. With whitespace stripped, the new assertion
(`residue_tests.rs:66-72`) requires exactly one
`removed:std::cell::Cell::new(false),residue,}` in the `create` slice. The
slice runs from `pub fn create(` to `pub fn kill_and_confirm(`, on text
before the first `#[cfg(test)]`. The shorthand `residue,` binds the field to
the `residue` parameter. The existing `release_leaf(fd, &name, &residue)`
counts pin the borrows. With this assertion, the hop I flagged in B2 is
pinned along the whole chain: `Runner::new` → `run_core` → `unix::run` →
`Group::create` → returned `Group` → `Drop`.

| Mutant (`cgroup.rs`, export) | Change | Result |
| --- | --- | --- |
| baseline | none | **pass**, exit 0, 2/2 `exec::residue_tests` (run `c5e4710b-8cfb-4401-8571-71e29b89b73d`) |
| C1 | line 116 → `residue: Arc::new(AtomicBool::new(false)),` | **killed**, exit 100, panic at `residue_tests.rs:67:5` (run `bba922d7-f624-4af4-949e-d62c3aed1957`) |
| C2 | line 116 → `residue: Arc::default(),` | **killed**, exit 100, panic at `residue_tests.rs:67:5` (run `6fa2e42f-f4d1-4d6a-a970-df6ff9b655c1`) |
| C3 (characterisation) | after line 45, insert `let residue: Arc<AtomicBool> = Arc::default();` | **survives the pin**, exit 0, 2/2 (run `6a38fdf0-50dd-41b4-9a75-903532bf2005`). See SA4 |

After each mutant I restored `cgroup.rs` from a byte copy and re-checked it
against blob `d85544b`. All three restores matched.

`cgroup` is `#[cfg(target_os = "linux")]` (`mod.rs:17-18`). On Windows these
mutants therefore reach the test only as `include_str!` text, and nothing
compiles them. My prior recheck called C1/C2 "compiled mutants". That
overstated it: they were compiled into the pin test as text, not type-checked
as Linux code. By source reading, both type-check on Linux, because `Arc` and
`AtomicBool` are already in scope in `cgroup.rs`.

The root-reported runs are consistent with what I reproduced: fmt pass,
baseline 2/2 `428e581e`, C1/C2 exit 100, and restored 2/2 `0b0804b8`. I did
not rerun them by ID.

### Advisory

**SA4 Shadowing evasion (non-blocking).** C3 shadows the parameter at the
top of `create` with a fresh flag. The returned field and both
`release_leaf` borrows then use the fresh flag, and every pin still passes.
The B1 `Arc::new(AtomicBool::new(false))` ban covers only `mod.rs`.

By source inference, the plain form of C3 leaves the `residue` parameter
unread. On Linux, `unused_variables` would then fail hosted
`cargo clippy --workspace --all-targets --locked -- -D warnings` on
`ubuntu-latest` (`ci.yml:49`). I did not run this, so it is not credited
here. A form that consumes the parameter first is a deliberate evasion. The
prior recheck already accepted that a source pin does not catch adversarial
edits.

Production `cgroup.rs`, before its `#[cfg(test)]`, contains no `Arc::` or
`AtomicBool::` constructor. A cheap second guard would therefore be a
whitespace-insensitive ban on `Arc::new(`, `Arc::default` and
`AtomicBool::new(` in production `cgroup.rs` and `unix.rs`. That optional
hardening could land with the re-pin. It is not required to close B2.

## Commands and results (this recheck)

All commands ran on Windows `x86_64-pc-windows-msvc`, rustc 1.95.0
`59807616e`, cargo-nextest 0.9.151, in the private export of `8fb7c84`. Logs
stay local.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `cargo nextest run --locked -p arxd -E 'test(exec::residue_tests)' --no-fail-fast` | exit 0; 2 passed, 325 skipped; run `c5e4710b` |
| the same filter with C1 / C2 / C3 | exit 100 / exit 100 / exit 0 (runs above) |
| `cargo clippy --locked -p arxd --all-targets -- -D warnings` | exit 0 (Windows targets only) |
| blob and SHA-256 identity of `exec/*.rs` (`git rev-parse`, `sha256sum`) | as tabled above; export matched `8fb7c84` at the end |

## Credit and limits

- **Credited to `e36f4fd`/`8fb7c84`:**
  - Spec B2 closure.
  - The Standards SA3 manifest refresh.
  - The Windows fmt result, the Windows `arxd` Clippy result, and the
    focused 2/2 run.
- These are **Windows source-pin and formatting proofs, not a Linux
  typecheck or Linux runtime proof.** Linux and macOS compilation, Linux
  Clippy, and Linux R1/R2 execution all remain outstanding until hosted CI
  runs.
- No real cgroup, root, native, owner, service, account, VM, Lab Manager,
  experiment, private-state or ignored-fixture action. No full workspace
  test run. No PROGRESS or product edit. No GitHub write, push, PR, watch,
  merge, settings change or retry.
- E5 publication still needs:
  - a re-pin onto main `2fbc647`, which carries E1's `mod.rs` addition, with
    refreshed manifest rows;
  - independent integration review on the merged base;
  - all hosted gates.
- M2 is incomplete. Native, installed and dedicated paths remain
  **unverified on real hardware**. `E_PROTECTION` and `ExecutionFailed`
  remain unknown and unresolved, with no fix or retry.

## Summary

Standards: **APPROVED**. SA3 is closed, SA1 is carried, and there is one
cosmetic fail-closed advisory. Spec: **APPROVED** for the narrow B2 scope.
The returned `Group` field is pinned to the incoming residue flag, C1 and C2
are killed at `residue_tests.rs:67`, and production is unchanged. SA4
(parameter shadowing) is advisory: its plain form relies on hosted Linux
Clippy, and the optional constructor ban can harden it at the re-pin.
