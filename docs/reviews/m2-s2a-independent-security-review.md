# M2 S2a inactive panic latch: independent source/security review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-14.
This is a single-agent review with no subagents. It reviews the
OpenAI / gpt-6.1-sol / medium S2a implementation, so it is a
different-provider review. Standards and Spec are reported separately.
It is a source review only. It grants no publication, merge, activation,
experiment, S2b/F0 work or M2 acceptance. Normal hosted checks and the
exact-head review gate are still required.

## Isolation and pins

- Cwd and Git top level were the same isolated T3 worktree
  (`<isolated-worktree>`), not the canonical checkout. Branch
  `t3/m2-s2a-security-review`, status clean, HEAD
  `0b4df15393358c53745c6af014955041a0da1ddd` before any work.
- Base: merged S1 (PR67) `03c60033b6800f347f25af2aae3c786b848a7376`.
  It has the same tree as builder base
  `fca6eaba67a371d0e95623a223462d872c688431`
  (`ac29b1f64e53f6ffc2e56c281bf33e5f6bf76975`).
- Source: builder `76aaae5a50ee2ab5fba778e73b36dbdb2d6f21b3`, retained
  `a09569a4705e06f33fc6bf14e3919d841532bcf6`. Same tree
  `d751e20806a690f2735668f8684de8a6f5eddc1c`, same stable patch-id
  `39e996f9df24a7402f8323b8e80b3e76246456be`.
- Evidence: builder `4918a2415b9d229ee6ace0b1e3a15ea9add06c9f`, retained
  `3c3852ea4a9d233e029faabff2765905c5498e2f`. Same tree
  `dcdc5e5e0e0d2cea0e154217e9a0744138379ff3`, same stable patch-id
  `c600c6852afeef6def40e5630cbe6a217071b96a`.
- `0b4df15` adds only three PROGRESS lines (S1 merge record, S2a queue).
- Spec: accepted S2 / S2a in `docs/reviews/m2-remaining-gates-proposal.md`
  ("D6 disposition proposed for separate implementation review") and ADR
  0020 D6.
- Standards: AGENTS, KICKOFF, CONTRIBUTING and the review smell baseline.
- Changed files `03c6003..0b4df15`: `unix_bridge.rs` (+12/-3),
  `unix_bridge/tests.rs` (+2), `unix_bridge/tests/e6.rs` (+76/-2),
  `docs/evidence/linux-panic-latch-s2a.md` (new), `docs/PROGRESS.md` (+3).
  No CI, workflow, Cargo manifest, lockfile, policy or other file changed.
- SHA-256 at HEAD match the evidence manifest exactly:
  - `unix_bridge.rs`
    `323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1`
  - `unix_bridge/tests.rs`
    `60dd457a6158a400999111f8b424e8c17221a84acc4dac1f3e31a8f32ea819ae`
  - `unix_bridge/tests/e6.rs`
    `d84639728a167e57fa8698525e39482c906ab047ecbd59e25c73a3d4f7eb6208`
  - `Cargo.lock`
    `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc`
    and `rust-toolchain.toml`
    `97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe`
    also match. All five changed files
  have 0 CR bytes and `eol=lf` attributes. `git diff --check 03c6003
  0b4df15` passed. A scan of the diff found no raw owner path, account
  name, e-mail, secret or private data.

## Source behaviour

The only non-test change is the `Err(_payload)` arm of
`impl Executor for UnixBridge::run`:

```rust
if self.runner.as_ref().is_ok_and(|runner| runner.cleanup_residue()) {
    self.readiness.latch(Cause::Containment);
}
Report::Failed { creation: Creation::Unknown }
```

- The latch runs before the arm returns, so it is before `Execution`'s
  `active.fetch_sub` (slot release). This matches ADR 0020 D6 ("stores the
  flag, then latches, before its slot release") and the proposal's
  recommended one-line alignment.
- The payload binding stays `_payload`; nothing reads, formats, logs or
  returns it. The report is unchanged `Failed { Unknown }`, so row G,
  Execution quarantine, output capture and identifier withholding are
  untouched. No `Report`, `Creation`, `Cause`, `Unavailable` or row
  changes.
- `Readiness::latch` is a `1 → cause` compare-exchange, so an earlier
  Rights (or any non-Ready) state wins and the latch is monotone. A
  constructed-Err runner skips the latch, which is correct: it never ran.
- `UnixBridge::new` and `AdmissionDomain` stay `#[cfg(test)]`; the existing
  T-test that checks `Execution::configured` contains `Self::native(` and
  not `UnixBridge` still passes. Production cannot construct the bridge.
  No `Group` constructor, held-request hook, U-a/U-b/U-c seam, A13 point
  or F0 artifact is present.

## Standards

- No documented-standard breach. One concern, one conventional source
  commit, behaviour covered by a meaningful test, honest evidence with
  limits and **unverified on real hardware** label.
- Advisory (smell, judgement): possible **Duplicated Code**. The shape
  "if `cleanup_residue()` then `latch(Containment)`" now occurs three times
  in `unix_bridge.rs` (gate step 1, post-run, catch arm). A tiny private
  helper would unify them. Non-blocking; the proposal asked for one line.
- Advisory: the matrix's monotone check (`state.set_ready()` then
  `check()`) is near-tautological for the current `0 → 1` compare-exchange.
  It still kills a future unconditional `set_ready` store, so it is kept.
- Advisory: PA6 drops its test-only `residue.store(false)`. That reset
  never affected `preflight` (which reads only check, runner and
  readiness), and the following four calls refuse earlier at Execution
  quarantine, so no coverage is lost. Removal matches the proposal's
  "No assertion may reset ... the monotone flag".
- Advisory: the catch arm calls `cleanup_residue()` outside the
  `catch_unwind`. For `exec::Runner` it is a single atomic load and cannot
  panic; only a misbehaving test fake could unwind past the slot here.

## Spec

- Implemented as specified: on unwind, latch Containment if
  `cleanup_residue()` is set, before returning the unchanged report. No
  missing or partial requirement for S2a.
- Tests match the requested matrix. `s2a_panic_latch_before_return` crosses
  residue {false, true} with an earlier Rights cause established inside the
  running port, calls `Executor::run` directly and, immediately after
  return and before any next gate, asserts `Failed { Unknown }`,
  `state.check()` (terminal/first cause: Rights wins, else Containment iff
  residue, else Ready), ordinary `preflight`, monotonicity after
  `set_ready`, residue value and exactly one fake Runner call. Residue
  starts false and is set only inside the panicking step, so body gate
  step 1 cannot latch and the post-run latch is skipped by the unwind: only
  the catch arm can satisfy the residue-true rows.
- PA6 adds `preflight == MechanismUnavailable` before its next in-slot
  gate, which the original catch arm could not satisfy.
- The new matrix runs only inside the existing normal T10 parent's child,
  under both backtrace settings; the parent asserts the fixed literal
  `ARX-S2A-FIXED-PANIC` appears exactly 4 times per mode, and keeps the
  MARKER and identity-marker withholding assertions. No direct ignored
  launch was used by builder or reviewer.
- No behaviour beyond the spec was added. Claims are correctly limited:
  portable fakes do not prove real Runner Drop/slot order or cgroupfs;
  real-Runner PA6 stays OPEN.

## Reviewer runs (Windows x86_64, rustc 1.95.0, cargo 1.95.0)

All of these actually ran on the HEAD bytes above.

- `cargo fmt --all -- --check`: passed.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo test --locked -p arxd --lib service::unix_bridge`: 36 passed,
  1 ignored (parent-owned fixture), 316 filtered.
- `cargo test --locked -p arxd --test inventory`: 33 passed.
- No full nextest rerun. The builder's 389 passed / 3 skipped (run
  `b4decebe-119c-4bd1-ae8e-d94be280a197`) is credited at the source pin
  only; the HEAD source bytes are identical.

**Independent mutation controls.** To attribute kills to the new matrix
rather than PA6 (which runs first in the shipped fixture and kills the
remove/negate mutants on its own), I temporarily moved
`e6::s2a_panic_latch_before_return()` to the first statement of
`t10_fixture`. Each run used
`cargo test --locked -p arxd --lib service::unix_bridge::tests::t10_reexec_diagnostics -- --exact`.

| Variant (fixture reordered, S2a first) | Compiled | Result |
| --- | --- | --- |
| control, no source mutation | yes | passed (exit 0) |
| remove catch-arm latch | yes | parent `output.status.success()` failed, exit 101 |
| negate condition (`!cleanup_residue()`) | yes | failed, exit 101 |
| unconditional (`is_ok_and(\|_\| true)`) | yes | failed, exit 101 |
| wrong cause (`Cause::Rights`) | yes | failed, exit 101 |

Kills are observed only through the compiled parent assertion; the parent
does not print child diagnostics and I make no claim of a captured inner
assertion trace. Because the matrix ran first and the control passed, each
child failure is attributable to the matrix. After every variant both files
were restored with `git checkout` and their SHA-256 compared exactly to the
values above; the worktree was clean afterwards.

## Verdicts

- **Standards: PASS**, 0 blockers, 4 advisories (worst: possible
  Duplicated Code of the residue-latch shape).
- **Spec: PASS**, 0 blockers, 0 findings.

Remaining gates: normal hosted required checks and exact-head review. M2
remains incomplete; historical unknowns are untouched; production
selection stays inactive; real-Runner PA6 remains OPEN; native, installed
and dedicated paths are **unverified on real hardware**.
