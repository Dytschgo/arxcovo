# ADR 0020 Slice E5 independent security and source review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. Review sub-agent,
no subagents. This is the first independent source and security review of
the Sol E5 cleanup-residue signal. The ported source is byte-identical to the
old E5 `9f53569`, which was never independently reviewed. So this is a full
source review, not an equivalence check. Opus wrote the ADR 0020 architecture
and correction 1, and those keep their own Sol reviews. This review claims
independence only for the Sol implementation.

**Standards: CHANGES REQUESTED.** S1: moving the existing `exec/tests.rs`
behind `include!` takes it out of the required `cargo fmt --all --check`
gate. A misformatted `tests.rs` passes at the target but fails at the base.
Two advisories follow.

**Spec: CHANGES REQUESTED.** B1: no test can fail if the runner's flag stops
reaching `Group::create`. A compiled mutant in `run_core` passes a fresh
flag, and every focused test stays green. Linux R1/R2 cannot catch it either,
because they never reach `run_core`. That would make every production
`cleanup_residue()` false, so the E6 gate and latch would fail open. The fix
is a portable, tests-only change. Four advisories follow. Apart from B1, the
production behaviour I read matches ADR 0020 A5/D6, P6 and brief E5.

Both blockers have one bounded repair, which touches tests only (see
"Bounded repair"). No production byte needs to change.

This review grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Contract PR56 still waits for its gates
(macOS infrastructure retry). Publication still requires that merge, an
exact re-pin and independent integration review on the merged base. Native,
installed and dedicated paths remain **unverified on real hardware**. M2
remains incomplete. `E_PROTECTION` and `ExecutionFailed` remain unknown and
unresolved. This review claims no fix or retry.

## Pins

- Review target `70aa4b08046582ca790190c5059dca917bea9b2d` (evidence commit).
  Source `f866af45a31779abc30d7674a07aaa943dee940a`. Base
  `fc1a6648515ef2227b59623df75e599da3bc9abe`: the contract stack on main
  Stage D `f91ae5a` (`git merge-base --is-ancestor` exit 0). `git diff --stat
  f91ae5a fc1a664` over `crates/`, the Cargo files, `.github/` and
  `rust-toolchain.toml` is empty.
- Isolation: the runtime-prepared T3 worktree. Its top level is not the
  canonical checkout. Status was clean. Its HEAD is the later E1 progress
  commit `8154574`, not the target. Every read used the target's Git objects
  or a private `git archive` export of the target, built in its own target
  directory. I created and switched no checkout.
- Scope: `git diff --name-only fc1a664 70aa4b0` lists exactly
  `exec/cgroup.rs`, `exec/mod.rs`, `exec/unix.rs` and
  `docs/evidence/linux-residue-e5.md`. No `exec/hierarchy*` file exists at
  the target, so no E1 port is included. `git diff --check` exits 0.
- Byte identity: the target blobs `fad2ef7` (mod.rs), `0d087e3` (unix.rs) and
  `d85544b` (cgroup.rs) equal `9f53569`'s blobs. Exported SHA-256 values match
  the evidence table. `tests.rs`, `stage_b_tests.rs`, `disposable.rs` and
  `service/` are unchanged from the base.
- Read: AGENTS, KICKOFF CI rules, README checks, ADR 0020 D1/D6/A5, brief
  gates and E5 R1-R4, the correction-2 disposition (P2, P6) and the Opus
  recheck (A1 errno clause), and the E5 evidence.

## Standards

### Blocking

**S1. `include!("tests.rs")` removes `exec/tests.rs` from the fmt gate.**
`exec/mod.rs` replaces `#[cfg(test)] mod tests;` with an inline
`mod tests { include!("tests.rs"); ... }`. rustfmt follows `mod` declarations,
not `include!`, so `cargo fmt --all --check` no longer formats `tests.rs`.
README and KICKOFF list `cargo fmt --all --check` as a required CI check on
all three OSes.

Evidence (private export): I appended `fn   fmt_probe( ) {   let x=1 ;let
_=x; }` to `tests.rs`. `cargo fmt --all --check` exited **0** with the target
`mod.rs`. With the base `mod.rs` and the same `tests.rs`, it exited **1**.
Both files were then restored to `c2b4130`/`fad2ef7`. The E5 evidence says
the inline module "preserve[s]" the scanner boundary. It does not disclose
that the required fmt gate loses coverage of `tests.rs`. The repair is
below. It also removes the need for the trailing inline module that the
Stage B scanner constrains.

### Advisories

- **SA1 Brittle literal pins.** `cleanup_source_pins_...` matches exact
  newline-and-indent strings, for example `"if raw < 0 {\n
  release_leaf(...)"`. `.gitattributes` forces `eol=lf`, and a mismatch
  fails closed, so this is safe but fragile under rustfmt changes. Possible
  Duplicated Code: the Stage B scanner already tokenizes source. Matching on
  token windows would survive formatting.
- **SA2 Tautological and unneeded setup in R3.**
  `assert_ne!(runners[0].config.cgroup_parent, runners[1]...)` holds by
  construction. The tempdirs and `create_dir` calls are unneeded because
  `validate_config` is pure. This is harmless. The meaningful assertions are
  `!Arc::ptr_eq`, fresh `false`, isolation and monotone-through-shutdown,
  and they kill the M3/K3 mutant.

No smell from the baseline applies materially. Raw `i32` fds in
`release_leaf` match the surrounding `cgroup.rs` style.

## Spec

### Blocking

**B1. The production flag path `Runner -> unix::run -> Group::create` has no
test that can fail.** Brief E5: "Pass only the private flag clone through
`Runner::run_core -> unix::run -> Group::create`." AGENTS: "Every behavior
needs a meaningful test." This task also requires source pins that are
coupled to the production call site, not only to fakes.

- R3 stores into a clone the test takes itself (`runners[0].residue.clone()`).
  It never goes through `run_core`.
- The portable pin checks `cgroup.rs` and two substrings of `unix.rs`. It
  never reads `mod.rs`.
- Linux R1/R2 build `Group` directly over temporary directories. Reaching
  `run_core -> Group::create` needs root and a trusted cgroup2 parent, and
  ADR 0020 deliberately has no trust-bypass seam for that.

Compiled mutant **S1-mut** replaces `self.residue.clone()` in `run_core` with
`Arc::new(AtomicBool::new(false))`. The focused suite passed: exit 0, 17/17,
run `dc24ac48-...`. The mutant leaves no unused binding, so Linux
clippy/rustc would not flag it (*source inference*, Linux not compiled). With
that mutant, every residue would be stored into a dropped flag. Then
`cleanup_residue()` is always `false`, the D6 in-slot gate and after-run
latch never fire, and the per-domain bound of `L` residue calls disappears.
That is a silent fail-open on the security property E5 exists to provide.

Mutant **S2-mut** shadows the parameter with a fresh flag in `unix::run`
before `Group::create`. It also passed: exit 0, 17/17, run `735c10f4-...`.
Linux `clippy -D warnings` would probably flag the then-unused parameter
(*source inference*, unrun), but no test pins it.

### Advisories

- **PA1 Row H wording versus Drop retry.** After a run, `ChildGuard::kill_tree`
  caches the first `kill_and_confirm()` result. When that result is `false`,
  `Group::drop` calls `kill_and_confirm()` again (pre-existing behaviour) and
  stores the flag only if the retry also fails. That matches the residue
  model ("released without a confirmed `unlinkat`") and the brief's text.
  Row H keeps its quarantine through `tree_cleanup_confirmed: false`. However,
  the ADR D6 table lists "residue signal" for row H as if unconditional.
  Clarify the wording in the E6 or integration docs. No source change.
- **PA2 E5 alone gives no service bound.** `Runner::new` stays `pub` and mints
  a fresh `false` flag. Nothing in E5 stops a second runner over one subtree.
  This is correctly deferred to E6/E7 (P2, T20 construction and visibility
  pins), and it is a precondition, not an E5 defect. E5 adds no static, no
  config flag, no public reset and no replacement API. `git grep` over
  `exec/` for `store(false`, `swap(`, `compare_exchange`, `static ...
  AtomicBool` and `OnceLock` finds nothing. `RunnerPort` is unchanged.
- **PA3 Portable pins do not forbid a reset.** Mutant **S3-mut** adds
  `else { residue.store(false, ...) }` to `release_leaf`. It passed on
  Windows: 17/17, run `b23286af-...`. Linux R2's "later" assertion should
  kill it, but R2 is **unrun** here. A cheap portable assertion that no
  production slice contains `store(false` would close this on every OS.
- **PA4 Linux evidence is outstanding.** R1/R2, Linux compilation and macOS
  compilation were not run locally. The first execution is Linux/macOS CI.
  This review does not satisfy those requirements.

### Verified (source reading; no runtime claim on Linux)

- **Ownership and ordering.** `Runner` owns one `Arc<AtomicBool>`, created
  in `new`. The only clone is the Linux-gated argument in `run_core`, moved
  into `unix::run` and then into `Group::create`/`Group`. The only stores
  are `true` with `Release`, in `release_leaf` and in `Group::drop`. The only
  load is `Acquire` in `cleanup_residue`. `Runner::drop` cancels shutdown and
  does not touch the flag. The flag is per instance and never cleared.
- **Create paths.** No leaf exists before `mkdirat`, so earlier returns
  correctly leave the flag alone. On failed `openat`, `release_leaf`
  performs `unlinkat` and at most an atomic store, then the caller reads
  `last_os_error()`. That is bit-for-bit the legacy read order, including
  the recorded quirk that an unlink failure is the reported errno (A1). On a
  setup failure, `e` is captured before `release_leaf`. The `dir` fd stays
  open across rmdir, as before.
- **Drop and kill confirmation.** `Group::drop` keeps its single
  `kill_and_confirm()` call and stores only on `false`. `removed == true`
  short-circuits with no kill write and no flag (R1 second case).
- **Same-thread release before return.** On every early return between
  `Group::create` and `ChildGuard` (socketpair, final recheck, spawn
  failure), the drop happens inside `unix::run`. After spawn, it happens
  in `ChildGuard`'s field drop, also before `run` returns. That is the same
  thread and is sequenced before any caller's slot release, which the D6
  bound assumes. No `panic = abort` profile is set.
- **Unchanged surfaces.** No change to `RunError`, `NotStarted`,
  `RunResult`, `Outcome`, `ExecError` text, legacy or phased mapping,
  cancellation or fence checks, outputs, reports or rows. No service
  caller, readiness, bridge, domain or activation change. The new source
  only threads the flag along existing runner/cgroup paths.
- **Stage B scanner.** `mod tests { ... }` is the single trailing module,
  so the scanner truncates it and the production inventory is unchanged
  (`p_scan_*` pass).

## Bounded repair (tests only; both blockers)

1. Restore `exec/mod.rs`'s test declarations to the base form
   (`#[cfg(test)] mod tests;` and `mod stage_b_tests;`). Add one out-of-line
   module, for example `#[cfg(test)] mod residue_tests;` in a new
   `exec/residue_tests.rs`, and move the two new tests there. It is a child
   of `exec`, so it can still read `Runner`'s private fields. It adds no
   `mod tests {` token for the scanner, and rustfmt formats it. Record this
   test-only file in the E5 manifest. This fixes S1.
2. Extend the source pin over the production slices (text before the first
   `#[cfg(test)]`) of `mod.rs`, `unix.rs` and `cgroup.rs`:
   - In `mod.rs`, `run_core`'s `unix::run(` call ends with the Linux-gated
     `self.residue.clone(),` exactly once.
   - `Arc::new(AtomicBool::new(false))` appears exactly once in production
     `mod.rs`, inside `Runner::new`, and never in production `unix.rs` or
     `cgroup.rs`.
   - In production `unix.rs`, the `residue` identifier appears exactly twice
     (the parameter and `Group::create(c, residue)`), and `Group::create(`
     appears once.
   - Optionally (PA3), no production slice contains `store(false`.
3. Re-run fmt, clippy and the focused filter. Show that S1-mut and S2-mut
   (and S3-mut if step 2's option is taken) now fail, then restore them by
   blob hash. K1-K3 must still fail.

## Commands and results (this review)

All commands ran on Windows `x86_64-pc-windows-msvc` with rustc 1.95.0
`59807616e` and cargo-nextest 0.9.151, in the private export with
`CARGO_TARGET_DIR` in the review scratch area. Logs stay local and are not
published.

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | exit 0 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | exit 0 (Windows targets) |
| `cargo nextest run --locked -p arxd -E 'test(exec::)'` | exit 0; 17 passed, 310 skipped; run `27318fc1-ed70-473a-88d0-93957b95085f` |
| `cargo clippy --locked -p arxd --all-targets --target x86_64-unknown-linux-gnu -- -D warnings` | exit 101: `libsqlite3-sys` build script needs `x86_64-linux-gnu-gcc`, which is absent. **Linux typecheck not achieved**. Nothing installed. |
| fmt probe (S1) | target `mod.rs`: exit 0; base `mod.rs`: exit 1 |
| `wsl --list --quiet` | WSL not installed; no Unix execution |

Each mutant below ran the same focused filter with `--no-fail-fast`. After
each run, the file was restored from the target blob, and the hash was
re-verified as equal.

| Mutant | Change | Result |
| --- | --- | --- |
| K1 | `Drop` ignores the `kill_and_confirm` result | **killed**, exit 100: `cleanup_source_pins_...` (run `25d0fd32`) |
| K2 | setup-failure site keeps bare `unlinkat` | **killed**, exit 100: `cleanup_source_pins_...` (run `291d2a1e`) |
| K3 | process-global `OnceLock` flag in `Runner::new` | **killed**, exit 100: `independent_owned_domains_...` (run `f46c1951`) |
| S1-mut | `run_core` passes a fresh flag | **survived**, exit 0, 17/17 (run `dc24ac48`) — B1 |
| S2-mut | `unix::run` shadows `residue` before `Group::create` | **survived**, exit 0, 17/17 (run `735c10f4`) — B1 |
| S3-mut | `release_leaf` resets on success | **survived on Windows**, exit 0 (run `b23286af`). Linux R2 should kill it; **unrun** — PA3 |

K1 and K2 exercise only the portable source guard, not Linux syscalls.

Not repeated, and credited only to old source `9f53569` and its evidence
`b181e72`: the 325/3 full workspace run and the old M1-M3 mutants. Byte
identity does not make those runs new-stack evidence.

No real cgroup, root, owner, service, account, VM, Lab Manager, experiment,
private state or native ignored-fixture action. No full workspace run. No
GitHub, push, CI, PR, label, watch or retry action.

## Summary

Standards: 1 blocking (S1: `tests.rs` falls out of the fmt gate), 2
advisories. Spec: 1 blocking (B1: no test catches decoupling the
runner-to-Group flag path), 4 advisories. Both blockers have one bounded,
tests-only repair, and no production byte needs to change.
