# S2a inactive panic containment latch

2026-11-14; OpenAI / gpt-6.1-sol / medium; implementation sub-agent.

Prepared base: `fca6eaba67a371d0e95623a223462d872c688431`.
Source commit: `76aaae5a50ee2ab5fba778e73b36dbdb2d6f21b3`.
Initial cwd equalled Git top level in the isolated T3 worktree
`t3-c2665bee-9e9248c8dfb9e7a2`; HEAD matched the prepared base and
`git status --short` was empty. Read AGENTS, KICKOFF, accepted
`docs/reviews/m2-remaining-gates-proposal.md`, ADR 0020 D6, bridge,
readiness and existing T10/E6 tests. User scope overrides coordinator
startup/progress duties: no babysitter action or PROGRESS edit here.

## Change and regression evidence

Only the inactive UnixBridge catch Err arm changes non-test behavior:
a runner with cleanup residue latches Containment after unwind, before
returning the unchanged Failed { creation: Unknown }. No payload is read
or logged. This is a real inactive code correction, not a claim of
cfg-stripped equivalence. There is no production constructor or activation.

The existing normal T10 parent owns the subprocess and runs its fixture
with and without RUST_BACKTRACE. No direct ignored fixture launch was used.
The new portable fake-port four-case matrix calls Executor::run directly:
residue false/true crossed with an earlier Rights cause established inside
the running port. Immediately after return, before any next gate, it checks
readiness, ordinary preflight, monotonicity, residue and one Runner call.
The report is Failed Unknown in every case. Residue false leaves Ready
unchanged; residue true refuses subsequent preflight; an earlier Rights
cause survives both cases. Rights is established during the call so the
initial admission gate does not prevent exercising the catch arm.

Existing fake PA6 additionally asserts preflight refusal before its next
gate and retains row-G cleanup_unconfirmed, Execution quarantine, activity
payload suppression and one-call assertions. Removed its test-only residue
reset; flags now remain monotone. T10 still verifies identifiers and private
markers withheld with both backtrace settings. Existing report/caller and
source ownership/production selection guards pass. No new admission seam.

## Runs on Windows x86_64

Toolchain: rustc 1.95.0 (59807616e 2026-04-14), pinned rust-toolchain.toml;
cargo-nextest 0.9.151 (6b3f994577a98874a2e0ea5bef77fbbab3434044).
All commands below actually ran locally; no hosted CI credit is claimed.

- `cargo fmt --all -- --check`: passed after applying cargo fmt.
- `cargo clippy --locked --workspace --all-targets -- -D warnings`: passed.
- `cargo test --locked -p arxd service::unix_bridge::tests::t10_reexec_diagnostics -- --exact`:
  passed, one parent test; other binaries had zero matching tests.
- `cargo test --locked -p arxd --lib service::unix_bridge`:
  36 passed, 1 ignored (parent-owned fixture), 316 filtered; 3.78s.
- `cargo test --locked -p arxd --test inventory`: 33 passed; 9.07s.
- One final `cargo nextest run --locked --workspace` on source commit:
  run `b4decebe-119c-4bd1-ae8e-d94be280a197`, 389 passed,
  3 skipped, 8 binaries, summary 13.026s. T10 parent passed in 4.534s.
- `git diff --check`: passed before source commit.

## Mutation controls and byte restoration

Each mutant modified only the catch-arm block and ran
`cargo test --locked -p arxd --lib service::unix_bridge::tests::t10_reexec_diagnostics -- --exact`.
Each completed compilation (Finished test profile), exited 101 and failed
the compiled normal parent's `assert!(output.status.success())`.
No compile rejection counts as a kill. The parent deliberately does not
print child diagnostics; the observed assertion failure is the parent
assertion, not a claimed captured inner assertion trace.

| Mutant | Observed result | Distinguishing assertion exercised in child |
| --- | --- | --- |
| remove conditional latch | compiled assertion failure | PA6 preflight before next gate |
| negate conditional latch | compiled assertion failure | PA6 preflight before next gate |
| unconditional latch | compiled assertion failure | residue-false immediate readiness remains Ready |
| wrong cause Rights | compiled assertion failure | residue-true immediate readiness expects MechanismUnavailable |

The first two fail PA6 before reaching the added matrix. The latter two
reach the matrix; Rights has a distinct observable token. After every
mutant the original bytes were restored and compared exactly; a finally
restore also ran. Original/restored unix_bridge.rs SHA-256:
`323fa965a041c4ad53c99d149f810e031d886cc09e83089d26c82d27b49026d1`.
The source file contains LF only, with no CR bytes.

Additional source/pin SHA-256 values:

- tests.rs: `60dd457a6158a400999111f8b424e8c17221a84acc4dac1f3e31a8f32ea819ae`
- tests/e6.rs: `d84639728a167e57fa8698525e39482c906ab047ecbd59e25c73a3d4f7eb6208`
- Cargo.lock: `1ebaf3af25f462da12eb8e8f7cfff3803deb180dd4c1f294be67bef3f3a7e4cc`
- rust-toolchain.toml: `97aab3493c1fb14fe9a21c760433577f434714f5725bbb176e67048dde581afe`

## Failed attempts and limits

Initial read commands used an unsupported rg Windows wildcard and a wrong
readiness path; corrected to the directory and unix_bridge/readiness.rs.
Initial fmt check failed on new-code formatting; cargo fmt corrected it.
A Python default-encoding read transiently changed an existing Unicode
fixture; diff inspection caught it and exact Unicode was restored before
the final focused/full validation. No failed functional fix attempts;
intentional mutant failures above are controls, not fixes. No historical
retry, installation, privileged/native experiment, external publication,
private-state read or owner action occurred.

Ready for independent Opus 5.5 high source review, which has not occurred.
S1 PR67 still awaits hosted CI/merge per the task; publication waits for
S1 merge and re-pin, independent review and normal required gates.
S2b real-Group/native seams, gate proofs and F0 are absent. Portable fake
ports do not prove real Runner Drop/slot ordering or Linux cgroup behavior.
Production configured selection remains MechanismUnavailable; M2 remains
incomplete, real-Runner PA6 OPEN, and native/installed/dedicated paths are
**unverified on real hardware**.
