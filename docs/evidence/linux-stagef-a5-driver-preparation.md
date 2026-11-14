# Stage F A5 closed driver preparation

2026-11-14; OpenAI / gpt-6.1-sol / high; implementation sub-agent.
Base `294542a21320a34786dc08710f8f57c674086a03` (PR #94); clean on entry;
`git diff 503c9cc HEAD --stat` was empty before this commit.
Read AGENTS/KICKOFF, accepted A4/A5 plan, Muse plan review/recheck, A5
source evidence/review, and current A3 driver/evidence/review.

Only driver.py, test_driver.py, mutation_check.py and this evidence change.
The allowlist adds exactly `exec::disposable::disposable_linux_a5`, deriving
Standard through the existing rule. Actual TEST stays A3 None. No selector
input, independent profile mode, schema, harness, workflow or package change.
The existing ignored A5 fixture already seeds, calls setup once, checks
postconditions, sends READY and holds identity; its source is untouched.

Independent test expectations pin all four names, profiles and complete argv;
install bytes/owned sets and exact runtime attribution now cover A5 too.
Invalid A4, cross-account, A5 variants/suffix/newline and empty names refuse
before effects. All four summaries accept only their selected literal with
one pass/zero failures, rejecting each other case. A5 also reuses the existing
mocked hold/ack, MainPID/executable/placement/sole-PID, capture/deadline,
execve, attribution lifecycle, reservation, collision and owned-cleanup tests.
No F0/F0b mock changes were needed; bounds/precedence remain unchanged.

## Executed checks (Windows, offline)

- `python tools/unix-exec-validation/mutation_check.py`: 76 audited tests
  passed; 29 compiled in-memory Python mutants killed by assertions (six
  new); three individually exact pure Rust A1/A2 baselines passed and four
  compiled Rust mutants killed by assertions. Driver SHA-256 unchanged:
  `49bc2a5cffaa2712fe4f172efe6a70c477b4747c2496305be2eefedcc44d3709`;
  Rust source fingerprints unchanged. No disk source mutation.
- `python tools/unix-exec-validation/test_f0_blockers.py --mutants`: 23
  assertion kills; driver source hash unchanged.
- `python tools/unix-exec-validation/test_f0b_transport.py --mutants`: 36
  assertion kills, four equivalent/no-credit cases; hash unchanged.
- `cargo test -p arxd --lib <filter> --locked --offline -- --exact`:
  each of these six filters ran one test and passed:
  `exec::hierarchy::fake::a5::empty_orphan_removed_before_acceptance`,
  `exec::hierarchy::fake::a5::preflight_refuses_before_mutation`,
  `exec::hierarchy::fake::a5::seed_observations_refuse_population_nested_and_foreign`,
  `exec::hierarchy::fake::a5::populated_and_empty_ordering`,
  `exec::hierarchy::fake::a5::refusal_never_accepts_residue_or_wrong_evidence`,
  `exec::hierarchy::parse::tests::a3_packaged_profile_status_fixtures`.
  Callgraphs inspected first: Fake/Manual, pure setup/parsers and compile-time
  package text only; no LinuxFs, real Runner or native adapter selected.
- `git diff --check`: passed.

Initial summary mutant produced errors, earning no credit; the positive
summary expectation now explicitly fails an assertion on refusal. The first
Rust stdin baseline then exposed the newly separate `a5.rs` relative module
path. The existing A1/A2-only slice now omits that unused module declaration
with an exact anchor assertion. No A1/A2 tests/assertions were removed or
weakened. The final complete mutation command above passed.

## Limits

Fresh independent Muse review is required; none was launched in this slice.
No A5 execution authority: D7c is consumed. No ignored fixture, product CLI,
native/system/service/account/owner/probe/network/GitHub/CI/dispatch or
subagent action. A1/A2 and A3 historical evidence is unchanged. M2 A4/A5,
remaining F/G and real-Runner PA6 stay open, **unverified on real hardware**.
Linux-only fixture compilation and execution remain unverified locally.
