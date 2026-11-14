# M2 S2b local unwind and admission gate proofs: independent source/security review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
This is a single-agent review with no subagents. It reviews the
OpenAI / gpt-6.1-sol / medium S2b implementation, so it is a
different-provider review. Standards and Spec are reported separately.
It is a source review only. It grants no publication, merge, activation,
experiment, A13 acceptance or M2 acceptance. Hosted checks, hosted Linux U-a
evidence and the exact-head review gate are still required. **This review
does not allow a merge.**

## Isolation and pins

- Cwd and Git top level were the same isolated T3 worktree
  (`<isolated-worktree>`), not the canonical checkout. Branch
  `t3/review-m2-s2b`, status clean, HEAD
  `1f6933121603ad5846c317169464adc926177df3` before any work.
- Base: `7899caa32b0fb3da8849c20d43016629a28336a8` (docs checkpoint) on merged
  S2a PR68 `47ff894f27f9a9275b320ad2da519e70e6117349`.
- Source: builder `efe8e55a360575e0da22202c24bbc7049cce5c60`, retained
  `88195d5595105657c6dbc79ca7468ecd582da8a9`. Same parent `7899caa`, same tree
  `b78eade94333b475cb1b9dd8e5e1f39e36862fa5`, same stable patch-id
  `d6a5c9fc279ab6c78ce2e277feed75d155669455`. Only the committer date differs.
- Evidence: builder `a7d445c49bb34f33e64b564bb258530623d3ddcf`, retained
  `29de5643fae05e85734805e639e31cc1850f29c9`. Same tree
  `d1cb01ad810c01d6c2d58c4914067d5f0ebd2c81`, same stable patch-id
  `3941b5735ce58e8dd3a24f9593f8923ed0e95a08`. It adds only
  `docs/evidence/linux-unwind-local-s2b.md`.
- `1f69331` changes only `docs/PROGRESS.md` (+3/-1: S2a main-CI record and the
  S2b review queue).
- Spec: accepted proposal `docs/reviews/m2-remaining-gates-proposal.md`
  U-a/U-b/U-c and mutants (lines 144–185), ADR 0020 D6, and the final
  correction recheck `29e2d03943c921b8312b175bf9ce96e5443c9914`
  (SP-N2: no token-only gate evidence).
- Standards: AGENTS, KICKOFF (review/routing rules), CONTRIBUTING, and the
  review skill's smell baseline.
- Source diff `7899caa..efe8e55`: six crate files, +324/-7. No workflow, Cargo
  manifest, lockfile, policy, scanner, ADR, proposal or PROGRESS change.
  `git diff --check 7899caa 1f69331` is clean.
- SHA-256 at HEAD equal the builder's evidence manifest and the `efe8e55`
  blobs exactly:
  - `exec/cgroup.rs` `64d4cb8c48e516d5fdcd92f85fe2c9c9abb028ee55251929e95824c16211d121`
  - `exec/mod.rs` `f8d425f7a57a309b7e10c29b4609f2c0c75002a33bc1eb0c00e04cd47dd3f9b4`
  - `service/executor.rs` `45da7396dc926ad40b18a5fab1ff2fd898c3d0577b8cd13d91eba1a7205cf5ed`
  - `service/unix_bridge/tests.rs` `b2e0c0efc24feb46555fee12413edf79c8afe22d0221908fde59238a597a780d`
  - `service/unix_bridge/tests/e6.rs` `f26d1ed0778c5b24495f46bde26b8248f046aedfbcae07440857453bf17dabb1`
  - `service/unix_bridge/tests/s2b.rs` `6eb93098dfda74ae52e751a07f69fd9252417184d65d6064cb6a26270643e089`
  - `service/unix_bridge.rs` (unchanged, S2a) `323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1`
- Builder logs under ignored `target/s2b-evidence` are not in this worktree.
  I could not re-hash them, so their hashes are recorded as builder claims
  only.

## Non-test behavior and fence inventory

All six changed paths are test-only after stripping cfg(test):

1. `exec/cgroup.rs`: `Group::owned_test_group` is an inherent impl inside the
   existing `#[cfg(test)] mod tests`, additionally gated
   `cfg(all(test, target_os = "linux"))`. It copies the existing
   `group_drop_signals_nonempty_leaf_but_not_already_removed` literal with
   `removed = false`. It opens only ordinary owned tempdir contents. It never
   calls `trusted_root`, `fstatfs`, `mkdirat` on cgroupfs, `/sys`, `/proc` or
   any root path. `Group` stays `pub(super)`, and its fields stay private.
2. `exec/mod.rs`: `TestGroup` is `cfg(all(test, target_os = "linux"))`, with a
   private field and no accessor, so it is an opaque owner. It has no
   `unwrap`, so the production payload scan of `mod.rs` is unaffected.
3. `executor.rs`: `active_for_test` is `#[cfg(test)]` and only does an Acquire
   load of the real `active` counter. It adds no hook and no write. The only
   admission seam used is the existing `admission_barrier`.
4. `unix_bridge/tests.rs` (`mod s2b;`), `tests/e6.rs` (visibility only) and
   `tests/s2b.rs` (new) are all inside the cfg(test) `tests` module.

The change adds no `Runner`/`run_core`/`unix::run` call. It adds no account
resolution, no trust-root skip and no root branch. It changes no production
constructor. Production remains inactive (`MechanismUnavailable`). The
scanners `stage_b_tests.rs` (payload inventory and W8/E7 crate-wide fence) are
unchanged. `s2b.rs` is not on the W8 reviewed-test-file list, so the crate-wide
fence scans it as production. That is stricter, not weaker, and it passes.

## Checks run by this reviewer (local Windows, labels are local only)

- `rv-focus`: `cargo test --locked -p arxd --lib s2b`, exit 0. Three portable
  tests passed, and the Linux U-a test is cfg-excluded on Windows. Log sha256
  `892b95030102d9aaa2a2378792913436f9a750a9db17cc5ee4104461cd270cd5`.
- `rv-linux-clippy`: `cargo clippy --locked -p arxd --all-targets --target
  x86_64-unknown-linux-gnu -- -D warnings`, exit 0. This is a **type-check
  only, not execution**. It used the already-installed Linux std, a separate
  target dir in the scratchpad, and `LIBSQLITE3_SYS_USE_PKG_CONFIG=1` with an
  empty `SQLITE3_LIB_DIR`. That skipped the bundled C build, which needs a
  cross gcc that is absent, and nothing was linked or installed. Log sha256
  `cd78af694b72db1ae98deaec10e4c5a9acd6e2f718282a6ae38b8a2206915fe3`.
- `rv-L1` negative control: I inserted a type error into the Linux-only U-a
  test. `cargo check --tests` for the Linux target failed with E0308 at
  `s2b.rs:54`, so the Linux U-a code really is compiled by `rv-linux-clippy`.
  The file was restored to sha `6eb93098…e089`.
- `rv-L2` `drop_skips_store`, compile-only: I replaced the `Group::drop` store
  with `let _ = self.kill_and_confirm();`. The Linux-target `cargo check
  --tests` passed (exit 0, with a dead-field warning only). **This earns
  compile credit only, with no assertion credit.** The file was restored to
  sha `64d4cb8c…1211d121`.
- Portable mutants. Each one compiled, was run with `cargo test --locked -p
  arxd --lib s2b` and returned exit 101. Exact bytes were restored, and
  SHA-256 equality was checked in `finally`:

| Label | Mutation | Failing assertion |
| --- | --- | --- |
| R1 `in_slot_readiness_refusal_omitted` | remove only the in-body readiness refusal | both gate tests: `S2b gate refuses zero additional Runner calls` (2 vs 1 latched; 1 vs 0 residue) |
| R2 `in_slot_residue_check_omitted` | remove only the pre-run residue latch | `s2b_independent_residue_gate`: same assertion (1 vs 0); latched case still passes, as designed |
| R3 S2a revert | catch arm no longer latches | `ordinary postpanic preflight` (`Ok(())` vs `Err(MechanismUnavailable)`) |
| R4 slot leak | drop post-run `fetch_sub` | `actual slot observed free before release` (1 vs 0) and the lexical guard |
| R5 `catch_outside_slot` (moved) | move decrement before `let r = if …` and remove the original | lexical guard only; both behavior tests pass, consistent with lexical-only credit |
| R6 early residue refusal | bridge preflight consults residue | `independent Ready residue case` (preflight not Ok) |

  R1, R2 and R5 reproduce the builder's three claimed mutants and their
  intended assertions. R3, R4 and R6 are additional reviewer mutants.
  Restored shas: bridge `323fa965…026d1` and executor `45da7396…cf5ed`, as
  pinned above. Reviewer logs are kept in the session scratchpad
  (`<scratchpad>/s2b`), not in the repo. Their sha256:
  - R1 `cef9f46d4c29e8b26ed40cd1502f761d9fecf8328616774bf665c5f821668f11`
  - R2 `ef67d1135bdc55987e0067a65e58265387d0b4660fbc2650495139b0e0850f8c`
  - R3 `67fdb0ca5e370da793b31b115f968b8633a6f60adce5b03e34a598095d941fac`
  - R4 `3f6994302e7d7b606daf3626a6ebd6d6c2654cf20d962343072363d4a3968c77`
  - R5 `5a5a86393b067900c4f06785682bcbf32477d1935f9bfffd6ea2060e4060dc03`
  - R6 `5b6a4a3f800d77f0df31db826bbc469af95b951dc59799d64fffa78b3d5b175e`
  - L1 `62f84b78a2369ec10d9ff4cba8b9dae70fd520460beea9e2506c601ccd0a0cfd`
  - L2 `a2c69aafcd046d88311be3b8643d45cd504e303c5091b365b7e4732be94520c0`
- Not run: full workspace nextest (not redundant work; the builder's
  392/3 run and hosted CI cover it), fmt, native Linux execution, and the
  Linux `drop_skips_store`/`group_forgotten` assertion mutants. There were no
  hosted, CI, GitHub or push actions.

## Standards

**0 blockers; 3 advisories.**

- **ST-1 (advisory, smell: Speculative Generality, mild).**
  `#[cfg(all(test, target_os = "linux"))]` on `owned_test_group` repeats what
  its context already guarantees: `cgroup` is Linux-only and it sits in
  `#[cfg(test)] mod tests`. It is harmless and readable as an explicit fence.
  Keep it or drop it.
- **ST-2 (advisory, inventory drift).** `mod s2b;` and `tests/s2b.rs` are not
  registered in `w8_e7_crate_wide_inactive_source_inventory`'s reviewed
  declarations or test files. Today that makes the fence stricter, because s2b
  is scanned as production and passes. A later legitimate test-only addition
  to s2b may trip the production fence and invite an ad-hoc scanner edit.
  Register it in a follow-up that has its own review; do not do it here.
- **ST-3 (advisory, smell: Mysterious Name / flag argument).**
  `stale_admission(panic_first: bool)` switches between two distinct
  scenarios. The separate named test entry points
  (`s2b_latched_readiness_gate`, `s2b_independent_residue_gate`) mitigate
  this. The `e6` helpers widened to `pub(super)` are a minimal reuse and are
  acceptable.

The commits are conventional and single-concern (test source and evidence
doc). The evidence doc discloses its failed attempts honestly, and no check
or scanner was weakened.

## Spec

**0 blockers; 4 advisories.** U-a, U-b and U-c match the accepted proposal
within their stated proof boundary.

- **U-a, verified by inspection plus Linux type-check, not executed.** A real
  `Group` (ordinary owned leaf with files, so `unlinkat(AT_REMOVEDIR)` fails
  with ENOTEMPTY) moves through `Mutex<Option<TestGroup>>` into the fake
  port's `run` frame. It is bound as `_group`, not `_`, so it lives until the
  unwind. Then a fixed-literal panic fires. The port reports the Group's own
  residue Arc, and the fake never stores it. Real `Group::drop` publishes
  residue during the unwind inside `catch_unwind`. The S2a catch arm then
  latches. Direct `Executor::run` on a real `UnixBridge` asserts
  `Failed{Unknown}`, residue true and readiness `MechanismUnavailable` before
  return. `Holding` is `Send + Sync`, since `Group` is `Send` and the Mutex
  supplies `Sync`. Windows excludes the test, and no native credit is claimed.
- **U-b, verified by run and mutants.** The pre-admitted request is held at
  the sole existing barrier, after executor preflight/quarantine and before
  `fetch_add`. The panic case awaits the panic dispatch (row-G success).
  Ordinary post-panic preflight refuses, and the early refusal is shown not
  to enter the bridge. The real active count reads 0 before release. After
  release the checks run in this order: actual bridge entry (+1), zero
  additional Runner calls, the latch, and only then the token. Bridge entry
  excludes Busy, Stopping and CancelledBeforeSpawn, because those never call
  `backend.run`. The independent case has no panic, dispatch or quarantine.
  Residue is published monotonically while the request is held, preflight
  stays `Ok`, and only the in-slot residue step can latch. There are no
  resets. R1 and R2 fail the intended Runner-count assertion, which satisfies
  SP-N2.
- **U-c, verified.** It is bounded and partial lexical only, and the source
  comment says so. Its two in-memory controls are meaningful, because each
  `replacen` target exists. R5 gives the moved-decrement mutant lexical credit
  only, not ordering-behavior credit.
- **SP-1 (advisory; condition on native credit). U-a root skip is a silent
  pass.** Under real or effective root the test prints `SKIP` and returns
  `ok`. nextest captures the stdout and stderr of passing tests, so a hosted
  green cannot distinguish executed from skipped. ENOTEMPTY does not depend
  on root, so the skip is unnecessary for correctness. Hosted ubuntu runners
  run nextest as a non-root user, so this does not block the source. U-a
  native credit must still show execution: for example
  `cargo test … s2b_real_group_unwinds_before_bridge_returns -- --exact
  --nocapture` with no `SKIP` line and a non-root uid recorded. Alternatively,
  a reviewed follow-up can make root a hard failure.
- **SP-2 (advisory; native mutant definition). `group_forgotten` as a real
  Linux mutant is underspecified.** The lexical control inserts
  `std::mem::forget(group)` after `unix.rs:223`. As compiled code that is a
  use-after-move, because `group` moves into `ChildGuard` at `unix.rs:363-368`.
  A `unix.rs` forget is also unreachable from U-a, because the fake port
  bypasses `unix::run`. The native mutant needs a compilable site that U-a
  reaches. One option is forgetting the `TestGroup` in the `Holding` frame
  before the panic, which U-a's residue assertion would kill. Otherwise
  `group_forgotten` stays lexical-only. Define this before native evidence is
  credited. `drop_skips_store` (`cgroup.rs` Drop) is reachable by U-a and
  compiles for Linux (`rv-L2`).
- **SP-3 (advisory, disclosed limit).** U-c strips comments with
  `split("//")`. That also truncates code after a `"//"` string literal on the
  same line, and only the first `workers.spawn_blocking(move || {` closure is
  checked. This is consistent with the "bounded partial lexical" disclosure,
  so no additional credit is due.
- **SP-4 (advisory).** U-a's "owned leaf remains nonempty" assertion checks
  that a file exists. It does not distinguish the ENOTEMPTY unlink failure
  from an earlier `put`/`text` failure. Both are real Drop paths that publish
  residue, so the decomposition claim holds. The test does not pin which
  cleanup step failed.

## Verdict

**Source review: no blockers on either axis.** The S2b portable proofs (U-b,
U-c) are credited locally. U-a is credited by inspection and Linux
type-check only. Production is inactive, there is no non-test behavior
change, no trust-root/run_core bypass and no real proc/sys/cgroup use.

Outstanding gates before any merge or native credit:

1. Hosted Linux execution of `s2b_real_group_unwinds_before_bridge_returns`,
   shown non-skipped (SP-1).
2. Native Linux compiled assertion-failure runs of `drop_skips_store` and a
   defined, compilable, U-a-reachable `group_forgotten` (SP-2). Both are
   UNRUN.
3. PR publication, green required checks (CI, `ci-time-budget`,
   `agent-review`) on the exact head, and a coordinator merge decision.
   None is granted here.

Real-Runner PA6 remains **OPEN**, A13 is not accepted, M2 is incomplete, and
native or root paths are **unverified on real hardware**. No activation,
experiment, ignored or native fixture, install, owner/admin, private-state,
subagent, push, PR, GitHub or CI action occurred. Old Lab Manager and
`agent-windows-test` were not touched.

Standards: 0 blockers / 3 advisories (worst: ST-2 inventory drift). Spec:
0 blockers / 4 advisories (worst: SP-1 silent root skip can mask U-a native
evidence).
