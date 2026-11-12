# ADR 0019 Stage B independent review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-12. Review sub-agent,
no subagents. This review covers only the Sol Stage B implementation, so it
can count as independent review of that work. Opus wrote the ADR 0019
architecture (D1-D7, C1-C3). This review claims no independence for that
contract. Contract credit stays with the retained Sol reviews.

**Standards: CHANGES REQUESTED.** One minor test correction (F1) is needed.
No source change is needed. **Spec: PASS** for the Stage B scope, within the
coverage limits below. This review grants no activation, no native execution,
no Stage C-G work and no M2 acceptance. Native, installed and dedicated paths
remain **unverified on real hardware**. M2 remains incomplete.

## Pins

- Worktree: the runtime-prepared T3 worktree, branch `t3/review-stage-b`. Its
  top level matched the working directory and differs from the canonical
  checkout. Status was clean at exact HEAD
  `454beb6a35418512f9f6985fc1b8d15621989ae4`. No checkout or worktree was
  created, and nothing was written to the canonical checkout.
- Candidate `454beb6` (docs only: PROGRESS and evidence). Source
  `4fe2bde4fddef381fc54dab5688f7300ad9e354e`. Base
  `6d474b56ad88e90aff8441bf3bf64d27e65c8078`, whose parent is actual main
  `064736022e559d0fb46a1de3943e57212eca87b0` (#51). Base and main differ
  only in `docs/PROGRESS.md`.
- The diff from base to candidate touches only `exec/{mod,unix,cgroup}.rs`,
  the new `exec/stage_b_tests.rs`, `docs/PROGRESS.md` and
  `docs/evidence/unix-runner-stage-b.md`. `git diff 1d828ca 6d474b5 --
  exec development` is empty, so the runner was unchanged before Stage B.
  `exec/{tests,output,disposable}.rs`, development, service, config, lib,
  main, owner CLI, Cargo and CI files are byte-inherited.
- Source git blobs equal the evidence table: mod `e94549e8`, unix `d3ec3e7e`,
  cgroup `f6c65008`, stage_b_tests `d4d02944`. The evidence checkout SHA256
  values match here for unix.rs and cgroup.rs. They differ for mod.rs and
  stage_b_tests.rs only because this checkout uses LF line endings.
- Origins read: AGENTS, CONTRIBUTING, KICKOFF (rules and routing),
  `dytschgo-review`, accepted ADR 0019 (status, D1-D7, C1-C3, correction
  checkpoint), the complete brief (§1-§10, tables, tests, mutations, staging),
  the retained Sol architecture and clarification reviews, the Opus B1 recheck
  and the ADR 0017 creation/privacy references cited by the brief.

## Standards

Standards used: AGENTS, CONTRIBUTING, KICKOFF ("a test for every behavior";
one concern; honest evidence) and the Fowler smell baseline. The repo standard
wins over a smell: C1 requires every site to name its class explicitly, so the
repeated `map_err(|e| RunFailure::os(…))` is not flagged.

### F1 (Minor, blocking under the KICKOFF test rule): `tool` acceptance is untested

- Location: `crates/arxd/src/exec/mod.rs:318`
  (`matches!(a[1].as_str(), "path" | "tool")`). Test:
  `crates/arxd/src/exec/stage_b_tests.rs:111-155`.
- Proof: `shim()` always builds `path` mode. L2 has no case where a `tool`
  prefix is accepted.
- Reproduction: I replaced the pattern with `"path"` only and ran
  `cargo nextest run --locked -p arxd -E 'test(exec::stage_b_tests::)'`.
  Result: **8 passed. The mutant survived.** Run
  `0d11a6bd-5d85-4c63-999a-a2febd58d045`. Restored byte-for-byte (SHA256
  `d089fb44…`, `git status` clean).
- Impact: the mutant compiles and passes every test. It would refuse every
  Stage D `run_command {tool}` as row F.
- Required correction (test only): in L2, add a `tool` spec with 128 tail
  items, and a wide spec with a 4,096-byte target and cwd, both with
  `arguments[1] = "tool"`. Each must pass `validate_phased_spec`. Assert that
  legacy `validate_spec` refuses the 128-item case. Then rerun this mutant,
  record the kill and restore.

### F2 (Low, advisory, not blocking Stage B): P-SCAN matches only literal syntactic forms

- Location: `stage_b_tests.rs:350-356` (requires `.` before `expect` or
  `unwrap`), `:357-366` (the `From` header must name `RunFailure`) and
  `:367-384` (forbidden macros match only when followed by `!`).
- Reproduction: I appended a temporary probe test and ran it with nextest,
  run `58e80889-2575-42a5-88df-a392732777be`. It is restored byte-for-byte
  (SHA256 `bc8095f4…`).
- **Accepted** (bypasses):
  - `Result::unwrap(r)`
  - `.map(Option::unwrap)`
  - the raw identifier `r.r#unwrap()`
  - `use std::eprintln as say; say!(…)`
  - `type F = RunFailure; impl From<std::io::Error> for F {}`
  - `writeln!(std::io::stderr(), "{x:?}")`
- **Rejected**, as required:
  - an `unwrap` split across lines and comments
  - `panic![…]` and `panic!{…}`
  - `r"{x}"` and `r#"{x}"#` messages
  - nested `concat!`
  - a const message
  - `std::panic!("{}", x)`
  - a formatted `assert!`
  - `panic!` inside `macro_rules!`
  - a test module that is not last, and two test modules
  - the boundary hidden in a string, and a raw string hiding `.unwrap()`
- The brief's letter is met: §8 names "the method calls". The current
  production inventory has none of the bypass forms, and the compiler still
  rejects any `?` that has no real `From` impl. `writeln!` is a P2
  review-only rule.
- Recommended before Stage C/D adds files to P-SCAN:
  - reject the identifier tokens `expect`, `unwrap`, `unwrap_err` and
    `expect_err` in any position;
  - reject forbidden macro names in any position;
  - reject `type … = RunFailure`;
  - add a negative control for each probe form above.
- Otherwise, state these limits in the evidence.

### Smell and precision nits (judgement only, no correction required)

- Possible Duplicated Code: `unix.rs:173-181`, `344-352` and `407-410` repeat
  the same token-gathering expression. A local closure would keep the two
  rechecks and the loop identical by construction.
- `mod.rs:326-329`: `..s.clone()` clones the whole argument vector, then
  throws it away. The cost is bounded and harmless.
- `mod.rs:119-124`: `RunError` drops the ADR's variant docs.
  `SpawnFailed` means creation is unknown; `AfterSpawn` means a child exists.
  Stage D will map these variants.
- `docs/PROGRESS.md:12-13`: a doubled blank line splits the status list.

No documented-standard breach other than F1. The scope is one concern. Hardware
limits are labelled, and no secret, identity or path is printed.

## Spec

**PASS.** Every Stage B requirement in brief §10 is present. No unrequested
behavior was found.

### Critical contract evidence

| Contract | Evidence |
| --- | --- |
| C1 private `RunFailure` | `mod.rs:126-138`: not `pub`, derives nothing, constructors `new` and `os` only. `os` uses `ExecError::from`, so errno and the `-1` fallback are exact (R2 asserts both). Grep finds no `From` impl for it in exec. |
| Every site classified explicitly | All 29 `?`/`return Err` sites in `unix::run` and `account` (`unix.rs:70-436`) build `RunFailure` explicitly, matching brief §3a row by row. ServiceIdentity → AccountPolicy/AccountChanged → PrivateRoots (trusted_root, fstat, mode) → early recheck → macOS Containment → `Group::create` Containment → socketpair Resource → final recheck → spawn SpawnFailed → read_exact, mismatch, pipe take, nonblocking, drains, waitid, try_wait and final drains AfterSpawn. `account(c)?` passes its class through unchanged. `pre_exec` returns stay inside `spawn`. No child errno narrows a phase, and no cleanup is claimed. |
| Legacy value is exact, not rebuilt | `run` applies the unchanged `validate_spec`, then `run_core(…, None)` and `map_err(\|f\| f.legacy)` (`mod.rs:175-179`). `run_phased` applies `validate_phased_spec`, maps any error to `Invalid`, then `run_core(…, Some(shutdown))` and `map_err(\|f\| f.phase)`. No string, errno or ID crosses it. L1b's three distinct literals kill a lossy rebuild. |
| Phased validator | `mod.rs:312-331`: non-shim specs get the legacy rule. A shim spec needs ≥4 items, `path` or `tool`, a target of 1-4,096 bytes, a cwd ≤4,096 bytes, and no NUL in the prefix. The tail then gets the full legacy rule: 128 items, 65,536 bytes, 8,192 bytes per item, NUL-free. Common cwd, timeout and output checks still run. There is no blanket allowance (L2 includes the 65,537-byte tail case). |
| D3 shared cancellation | `pub use crate::development::Cancellation`. It is byte-identical to the removed type (Clone, Default, Release/Acquire). No development API changes. `Runner::drop` cancels only its own token (test). |
| Caller shutdown | `shutdown_requested(runner, caller)` gates the early check, the final check and the poll loop (`unix.rs:175`, `346`, `407`). Legacy `run` passes `None`. |
| C3 placement and priority | The early check is kept after `account`. The final check sits immediately before `command.spawn()`, after the cgroup, socketpair, `Command` and `pre_exec` setup. Nothing is polled between spawn and the identity read. Shutdown wins over cancel, and both use the existing literal. |
| Legacy deviations | Only the three C1 labels: the race-window refusal, the shared type identity and the panic payload text. A final-check refusal after `Group::create` drops the empty leaf through the existing `Drop`, as part of deviation 1. |
| C2 payload rewrites | The six `expect` sites changed. Four are now fixed-literal `unreachable!`. The two four-byte conversions are now panic-free array construction. The inventory has exactly four entries and is non-empty. No hook is installed. `pre_exec` is unchanged. |
| P-SCAN | Comment stripping (nested), string, raw-string and char tokens, structural exclusion of only a trailing inline `#[cfg(test)] mod tests {`, refusal of production code after it, multiset inventory and self-tests are all present. Limits: F2. |
| Tests | R2: all eight cases plus failure tags. Runner drop independence. L1b. L2 (complete except F1). L2b calls the actual legacy entry point. R1/L1 marker test (Unix). L3 (non-Unix, no OS work). `safe_entry()` skips before `Runner::new` and before any runner call on euid 0. Existing exec tests are unchanged. |

### Coverage boundaries (not findings; already declared)

- **No Unix compile exists anywhere yet.** `unix.rs` and `cgroup.rs` source
  has had no compile, Clippy or test run on any Unix host. The builder's Linux
  cross-check stopped at libsqlite3-sys (no GCC). Hosted Linux and macOS
  compile, Clippy and tests (including R1/L1) are required. With no `From`
  impl, a missed site is a compile error, but that also waits for hosted CI.
- Source review only (no runtime check) covers the per-site classes, both
  recheck placements, caller shutdown in the real loop, and the
  `run_phased → run_core` caller-shutdown pass-through. By source, a
  `Some(shutdown) → None` mutant survives every unprivileged test: Windows
  returns Unsupported, and non-root Unix stops at ServiceIdentity. These need
  the separately authorized root experiment (Stage F).
- The evidence states this correctly: there is no claim about std, implicit,
  aborting or dependency panics, backtraces, or real journald and log
  privacy. `NaturalExecutionFailed` and `E_PROTECTION` run 43091339273 remain
  unknown-phase failures with unknown cleanup. No retry, fix or waiver.

## Builder evidence attribution (not re-run)

These results are the builder's. This reviewer did not repeat them:

- fmt and `git diff --check`;
- locked workspace all-target Clippy;
- focused 14/14, run `ec3f34da`;
- one full workspace run: 306 passed, 2 skipped, 11.318 s, run
  `fc80b434-d0e7-45ef-a419-a2b5d65a229c`, at `4fe2bde`;
- nine compiled assertion kills (run IDs in the evidence). The first `From`
  mutant did not compile and correctly gets no credit.

My `cargo nextest list --locked --workspace` lists 306 tests. With 2 ignored
that makes 308, which matches the builder's totals.

## Reviewer commands (Windows, own bounded scope)

- Git pins, diffs, blob IDs and status: read-only.
- `cargo nextest run --locked -p arxd -E 'test(exec::stage_b_tests::)'` on
  the unchanged candidate: **8 passed**, 299 skipped. Run
  `64f905ef-13f1-401b-82c9-df6195664520`. R1/L1 is `cfg`-excluded on Windows.
- F2 probe: run `58e80889-…` (an earlier identical run's ID was not
  retained). The F1 mutant: run `0d11a6bd-…`. Both files were restored, SHA256
  values equal the backups and the tree is clean.
- `cargo nextest list --locked --workspace` (listing only).
- Not run: fmt, Clippy, the full workspace, the service fixture, any Unix,
  root, cgroup, spawn, account, native, ignored or disposable path, or any
  toolchain install. No GitHub action, push, PR or babysitter change.

## Disposition

Correct F1 with a test-only change. Rerun and record its mutant kill, then a
fresh recheck. F2 and the nits are advisory. Publication then needs
exact-head review credit and all five hosted gates, including Linux and macOS
compile and tests. The coordinator alone publishes and merges. No ADR
deviation needs recording; the candidate follows C1-C3 as accepted.
