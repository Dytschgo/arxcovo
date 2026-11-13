# ADR 0020 Slice E7 independent security and source review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. I was the only
review sub-agent and used no subagents. This is the independent security,
standards and spec review of the Sol high E7 implementation: inactive startup
discovery, the daemon row and the Activity tool cache. Opus wrote the ADR 0020
architecture and correction 1, which keep their own Sol reviews. This review
claims independence for the Sol E7 implementation only, not for the ADR 0020
architecture.

**Standards: CHANGES REQUESTED.** One blocker (SB1) and four advisories
(SA1-SA4).

**Spec (including security): CHANGES REQUESTED.** One blocker (PB1) and
seven advisories (PA1-PA7).

Neither blocker is a production defect. Production stays inert, and the
security properties I read hold in source: one domain, hardening checked
before Ready, cache fed only by response metadata, and nothing that could
activate. Both blockers are missing or non-compliant tests that the brief and
ADR name explicitly. After a correction, a recheck of the changed test,
evidence and script bytes is enough.

This review does not grant publication, integration, activation, cgroup setup,
experiments or M2 acceptance. Publication still needs a PR, a different-provider
review of the exact published head and all required checks. Native, installed
and dedicated paths remain **unverified on real hardware**. M2 remains
incomplete. The historical dispatcher privacy flake (`65762554`),
`E_PROTECTION` and `ExecutionFailed` remain unknown. I claim no fix, diagnosis
or retry for any of them. Stage F (diagnosis, then a separately reviewed
disposable experiment) and Stage G (activation) remain separate gates.

## Pins and isolation

- Worktree `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-292312d9-...`,
  branch `t3/e7-security-review`. Its cwd and Git top level are both the
  worktree, never the canonical checkout `D:/Code/Arxcovo`. Status was clean
  at the start and after every mutation run. HEAD
  `1237235ecb8bae0b465ce85b43e8bc6f55d3c6d3`.
- Original target `20d76c73834b4f10c135af90d72d118c47fa2196` (evidence) over
  source `cea3038b4db55ea450d4d64168d32708e40784f1`, parent
  `338d12ce64296b505d560595297f611be82be5c7`.
- Port: source `a19f2a9`, evidence `35d655c` and PROGRESS line `1237235` sit
  on `a4a8b36` and `2e55204` over merged E6 main
  `85209a46972bd0f7c92a4e490ddbf20a99f2bfcb`. The `85209a4` and `338d12c`
  trees are equal (`0c4f306`).
- Byte identity: `git diff 338d12c cea3038` equals `git diff a19f2a9^ a19f2a9`.
  `git diff cea3038 20d76c7` equals `git diff a19f2a9 35d655c`. All 11 HEAD
  blob SHA-1s match the evidence table. `85209a4..HEAD` changes only the 11
  source paths, `docs/evidence/linux-discovery-e7.md` and one PROGRESS line.
  There is no CI, Cargo, lockfile or policy change. The mutation script has
  exactly 31 lines.
- Read: AGENTS, KICKOFF, the review skill, ADR 0020 (D1, D6 including the
  single immutable admission domain, D7, D10, Consequences, the ADR 0017
  relation), brief gates, E6 and E7 (T20b, W1-W9, mutations), ADR 0017 rules
  5/6 as cited, the E6 review advisories and the E7 evidence.

## Standards

Sources: AGENTS (one-concern change, a meaningful test for every behavior,
honest evidence, no secrets), the brief's CI rules ("No test sleeps longer
than 1 s. Deadlines are seams with production-constant tests."), plus the
smell baseline. Formatting and Clippy are clean. The bounded rendezvous gates
(5 s/15 s give-up bounds using `yield_now`) are failure bounds, not expected
waits, and comply.

- **SB1 (blocker) W9 waits 10 real seconds on every run.**
  `w9_stuck_drain_is_failure_not_interruption` runs
  `tokio::time::timeout(Duration::from_secs(10), app.drain())` on a
  multi-thread runtime. The timeout is meant to expire, so each run costs a
  fixed 10 s. I measured it: the E7 module takes 10.07 s, or 0.34 s with W9
  skipped. That breaks the documented brief rule. The 10 s is also a
  test-local copy, not the production bound in `Host::run`
  (`service/mod.rs:165-171`). `assert_eq!(code, Err(ExitCode::Store))` mostly
  re-checks the test's own `unwrap_or(Err(ExitCode::Store))` fallback. So the
  evidence overstates two claims, "waits the actual 10-second drain bound" and
  "asserts Store/exit-5 semantics". The production 10 s and Store mapping are
  already pinned in paused time by
  `stuck_drain_is_bounded_and_never_claims_stop_audit`
  (`service/tests.rs:206`). Fix: use a timeout of 1 s or less for the
  "drain does not return while the fake is stuck" step, keep the
  started-row, non-interruption and late-ownership assertions, cite the
  existing paused-time test for the production bound, and correct the
  evidence wording.
- **SA1 Misplaced doc comment.** The new `ToolCache` was inserted between
  `/// Private local identity and bounded activity; no credentials or host
  paths.` and `pub struct Activity` (`store.rs:9-17`). The comment now
  documents `ToolCache`, and `Activity` has none. Move the comment back and
  give `ToolCache` its own line, for example "memory-only, response-metadata
  only".
- **SA2 Mutation-script newline confound.** `path.write_text(...)` converts
  LF to CRLF on Windows. W8's lexical pin requires `"\n        // No
  build-account..."`, so any `application.rs` rewrite fails W8 even when the
  rewrite changes nothing. I confirmed this: an identity rewrite in the
  builder's style compiled and was "killed" by W8. The builder's
  `production_start_discovery_spawn` kill therefore could not tell a real
  kill from a newline artifact. I reran all 7 builder mutants with
  `newline=''`. All 7 compiled and were killed by their named tests, so the
  control is real. Fix: write with `newline=''` or `write_bytes`.
- **SA3 Primitive Obsession / Data Clump (judgement).**
  `discovery_observation() -> Result<(u64, u64)>` passes `(ticket,
  timestamp)` as a tuple read through `.0` and `.1` in `store.rs`, `mcp.rs`
  and `application.rs`. A small `Observation { ticket, observed_at_ms }`
  would name it. Not blocking.
- **SA4 Weak or vacuous assertions.** In W6, `!text.contains(MARKER)` can
  never fail: `probe()` replaces the only `MARKER` bytes in `result().stdout`
  with `table()`. The `no_new_privs` and `sudo` checks are meaningful,
  because the rights line is present in the input. W8 counts
  `StartupDomain::new(` in `e7.rs` itself, which pins the test file, not the
  wiring. Neither is the only control for its property. Not blocking.

Smells considered and dropped. `StartupDomain` looks like a Middle Man around
`AdmissionDomain`, but ADR 0020 D6/D7 require exactly this ownership point,
so the repo wins. I found no Repeated Switches. The `SetupOutcome` map is
written once.

## Spec (including security)

Verified by source reading, a test, or a builder or reviewer mutant:

- **Single domain (D6, T20b).** `StartupDomain::new` is private and
  `cfg(test)`. It allocates `Readiness::pending()` itself, so no other domain
  can inject readiness. It calls the E6 `AdmissionDomain::new` exactly once.
  The runner factory is an `FnOnce`, consumed inside it, and the setup closure
  sits in a `Mutex<Option<..>>` taken once. There is no replace, reset,
  rebind, second-runner or handle-construction method. `TestHooks::executor`
  and the fixture both return `domain.handle()`, which is an `Arc` clone of
  one `Execution` holding the slot counter, backend and readiness. W1 asserts
  one factory call, `Arc::ptr_eq` across startup and both handles, and a
  shared terminal latch (exactly 2 port calls). W8 pins one
  `AdmissionDomain::new(` and one `Readiness::pending()` after `struct
  StartupDomain`. This closes E6 PA3 for wiring.
- **Hardening and nonstandard configurations are refused before Ready (E6
  PA5).** `derive` refuses non-Linux and admin configurations before the
  factory runs, so `constructed` stays false and setup calls
  `fail(Cause::Setup)` without calling the setup closure. A
  `SetupFailure::Hardening` maps through `f.into()` to `fail()`, never
  `set_ready`. `w2_nonstandard_and_hardening_fail_before_ready` covers
  Linux-admin, macOS and Windows, plus the wiring transition for Hardening and
  a zero-call refusal. The same `Location` value supplies `runs()` to the
  bridge and moves into the setup closure (see PA4 for test coverage).
- **Sequence (D7, W1).** The sequence is: setup on `Blocking`, then Ready
  only if `!stop.is_cancelled()`, then the `ready` barrier, then a stop check,
  then ticket and timestamp, then `start_daemon_discovery` (one blocking
  closure), then the `row` barrier, then `dispatch(None, ..)`, then
  `cache_discovery` only on `Ok`, gated inside on `outcome == "succeeded"`.
  W1 asserts the exact log `setup, s9, ready, row, port, dispatch`. The
  `discovery_started` flag and the single setup take make a second
  `start_discovery` a no-op.
- **Shutdown at every W3 barrier.** The setup failure gives zero rows after a
  bounded completion wait. The janitor `wait`, `s9` and `ready` points give
  zero rows and zero calls. At `row`, the synchronous `stopping` refusal under
  the workers lock gives refused/`shutdown`. W3e (`admission`) reuses E6's
  single `admission_barrier_for_test` seam, before the final
  shutdown/admission check. It gives refused/`shutdown` with zero port calls.
  For the already-admitted case (`port`), the fake observes
  `stop.is_cancelled()`, makes exactly 1 port call and leaves an honest
  `failed` row. The test does not claim zero calls there. All three of the
  builder's shutdown and order controls were killed in my rerun.
- **Drain (D7).** Source order is `Execution::drain`, then the sequence
  handle, then `Blocking::drain`, then `shutdown_pending`. A dispatch after
  `Execution::drain` took the JoinSet hits the synchronous `stopping` refusal,
  so it creates no stray worker. W9 makes no interruption claim: the stuck
  call stays owned, and the late completion is joined after release (SB1
  covers W9's timing).
- **Daemon row (W4).** `start_typed("arxd", DiscoverTools, true,
  DedicatedAccount)` reads back as `("daemon", "arxd")` and is absent from the
  agent's `list_activity`. It is `cfg(test)`, so no non-test caller compiles.
  This is stronger than a lexical pin.
- **Ticket and cache (D7, W5).** The `AtomicU64` sits next to the
  `Mutex<Option<ToolCache>>` on `Activity` and is shared by the MCP and
  startup paths. `fetch_update(.., checked_add(1))` fails closed on overflow,
  never wraps, and never issues ticket 0. The update replaces the whole value
  only if `ticket > cached`, which is strict. `now_ms` is bounded and
  non-negative, so the `as u64` cast is safe. K, M, G and refusals keep the
  seed. The equal ticket and the older ticket are both rejected. The
  reverse-finish overlap keeps the later ticket. The cache is never persisted
  or logged.
- **Privacy (ADR 0017 rules 5/6).** The cache copies only `response["tools"]`
  and `response["observed_identity"]`. `complete` builds the first from the
  validated table, and only on `succeeded`. It builds the second from the
  redacted `Metadata`. No code path at the update boundary can reach
  `Evidence`. The E6 bridge keeps the rights line out of `tools`. W6 compares
  the cached identity to the response, and the
  `identity_metadata_bypassed` structural substitute was killed. As the
  builder states, it is not a real-Evidence extraction proof.
- **MCP (W6, W7).** The agent `discover_tools` takes the observation before
  the row and updates only from the returned value. `list_hosts` overrides
  only `tools`, `tools_observed_at` and `last_observed_identity` when a cache
  exists. I extracted the no-cache block independently from `git show
  d44035d:crates/arxd/src/mcp.rs`. It equals the golden and the E6-main block
  byte for byte (1,548 bytes). `.gitattributes` sets `eol=lf`, so the source
  pin is CRLF-safe on hosted Windows.
- **Production inertness (W8, D10).** `start_discovery` outside its
  `cfg(test)` block is a comment only, which W8 checks with brace matching.
  `impl Hooks for Native` has no override. `configured` contains
  `Self::native(` and no `UnixBridge`. The capability strings are unchanged.
  The new non-test code (`discovery_observation`, `cache_discovery`,
  `tool_cache` and the `list_hosts` read) cannot produce a cache in
  production, because the native backend never returns `succeeded`. My grep
  found non-test `hierarchy::` references only in E1/E6's existing pure
  imports (`unix_bridge.rs:21,24`, `readiness.rs:3`). All other hits are
  `cfg(test)`. Non-test `Runner::new` has no sites: the only hits are
  `exec/disposable.rs`, which is `cfg(all(test, target_os = "linux"))`, and
  test files.
- **E6 advisories.** PA1 (lexical controls) is disclosed in the E7 evidence.
  PA3 is closed for wiring, as above. PA5 is honored. PA4 is carried forward
  (PA6 below).

Blocker:

- **PB1 Crate-wide non-test scans are not tests.** Brief T20b, under
  "Tests:", requires: "Non-test `exec::Runner::new` call sites must remain
  absent while production is inactive. Scan those calls explicitly, plus
  every wiring domain constructor site." W8 requires: "no non-test module
  references `hierarchy::` or `start_daemon_discovery`". ADR 0020 says the
  follow-up "names non-test Runner construction pins". D10 says "Production
  pins (tests in the brief)". The implementation checks `Runner::new(` only in
  the text after `struct StartupDomain`. The crate-wide scan was a one-time
  `rg` recorded in the evidence. So a future non-test `Runner::new` or
  `hierarchy::setup` call elsewhere would fail no test. That is the
  regression these pins exist to catch before activation.
  `start_daemon_discovery` is protected by `cfg(test)` (credited). Today's
  tree is clean, as I confirmed above. Fix: add a source-inventory test.
  It should list every `.rs` file containing `Runner::new(` or `hierarchy::`,
  and assert that each hit lies in a reviewed `cfg(test)` module or test file
  or matches the four allowed E1/E6 pure imports. The literal "no
  `hierarchy::`" wording cannot hold with those imports, so the inventory
  needs an allowlist. Give it a mutant control.

Advisories (my mutants were compiled and each was run against the whole E7
module; the survivors pass all 8 tests; no other module constructs
`StartupDomain` or touches the cache):

- **PA1 Callers' ticket position is unpinned.** I moved ticket allocation to
  completion time in the MCP path (`mcp_ticket_at_completion`) and in the
  startup path (`app_ticket_at_completion`). Both survived. W5's overlap test
  calls `Activity` directly with tickets placed by hand. So the
  completion-order bug the ticket prevents can come back at either call site
  without failing a test. The source placement is correct today. Suggest
  driving the reverse-finish overlap through the MCP `discover_tools` path.
  This would also show W7 replacing an existing cache, which today's W7 does
  not: it starts from an empty cache.
- **PA2 W3/W3b readiness and `Stopped` are not observed.** Brief W3b:
  "readiness never `Ready`". `ready_despite_stop` (calling `set_ready` even
  when stop is requested) survived. Safety holds anyway, because the stop
  token independently refuses every later step. Assert the readiness state,
  for example through `Readiness::check()`, after the `wait` and `s9`
  points. W3e's "leaves the executor set empty" is implied by a clean drain,
  not asserted.
- **PA3 Drain order is unpinned.** `drain_sequence_before_execution`
  (joining the sequence before `Execution::drain`) survived. The source order
  matches D7.
- **PA4 Same `Location` is unpinned.** `bridge_parent_not_location` (bridge
  parent `/unrelated/runs`) survived, because the E7 factory ignores
  `OwnerConfig`. Assert `owner.cgroup_parent == Some(location.runs())` in the
  W1 factory.
- **PA5 D7 paths that are not wired yet.** `StartupDomain` requires a
  located `Location`, so D7's "a `Locate` failure constructs the bridge with a
  refused runner and readiness `Unavailable(Setup)`" has no representation.
  The bridge `check` closure is constant `Ok(())`. Both are acceptable while
  this is test-only. Future activation must add both, with tests.
- **PA6 E6 PA4 unwind path (carried forward).** There is still no
  fake-port panic case showing that the flag store precedes slot release on
  unwind. E7 did not require one.
- **PA7 Cache visibility.** Every agent's `list_hosts` now shows the latest
  daemon or agent tools and the build account's `observed_identity`. This is
  the specified behavior (D7, W6), and it is limited to redacted metadata. I
  note it so publication review sees it deliberately.

## Credits and limits

Builder runs at source `cea3038` only. I credit them by the byte identity
above and repeated only what is listed under my own runs:

- `cargo fmt --all -- --check`
- workspace `cargo clippy --workspace --all-targets --locked -- -D warnings`
- focused E7: 8 passed; `service::unix_bridge::tests` plus `p_scan`: 35
  passed, 1 ignored
- `cargo nextest run --workspace --all-targets --locked`: **383 passed, 3
  skipped**
- 7 compiled mutation kills. SA2 found that the method was confounded for
  W8; my LF-preserving rerun independently confirms 7 of 7.

The failure history is honest. The earlier red workspace run (313 passed, 1
failed, 3 skipped, 69 unrun) came from the unchanged E6 ownership pin
scanning the newly appended factory. The correction moved `StartupDomain`
before `AdmissionDomain`. The E6 test and `readiness.rs` have no diff, and the
E6 pin still checks the whole `AdmissionDomain` region, so the placement does
not weaken E6. `StartupDomain` is outside E6's `!contains("pub")` tail, but
Rust privacy makes it module-private. The masked `row_on_setup_failure`
control was fixed with a bounded completion wait. Compile failures were never
counted as kills. Calling the identity mutant a structural substitute is
accurate.

Reviewer runs at HEAD `1237235` on Windows x86_64-pc-windows-msvc:

- `cargo test -p arxd --lib --locked service::unix_bridge::tests::e7`: 8
  passed in 10.07 s. With `--skip w9`: 7 passed in 0.34 s.
- `cargo test -p arxd --lib --locked -- service::unix_bridge::tests exec::stage_b_tests::p_scan service::application`:
  47 passed, 1 ignored.
- `cargo fmt --all -- --check`: exit 0.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: clean. This
  is package-scoped, not the workspace.
- Builder mutants rerun with `newline=''`: 7 of 7 compiled and killed by
  their named tests.
- W8 confound probe: a builder-style identity rewrite was "killed". The real
  spawn mutant, inserted after the comment and also after the block, was
  killed with LF preserved.
- Reviewer mutants: `mcp_cache_removed` and `list_hosts_ignores_cache` were
  killed (W6/W7). `ready_despite_stop`, `mcp_ticket_at_completion`,
  `app_ticket_at_completion`, `drain_sequence_before_execution` and
  `bridge_parent_not_location` survived (PA1-PA4).
- Every run ended with SHA-256 exact restore and a clean `git status`. The
  scripts live in the session scratchpad, not the repository.
- I ran no full workspace suite: no material concern called for one.

Limits:

- A Windows run does not compile or run `cfg(target_os = "linux")` code. It
  proves nothing about Linux or macOS paths.
- No hosted CI, and no GitHub push, PR, watch, merge, retry or settings
  change.
- No NativeRunner, owner, session, account, service, `/proc`, `/sys`,
  cgroup, root, VM, private state, experiment, ignored native fixture or
  installation.
- No product or PROGRESS edit. This file is the only change.

## Summary

Standards: 1 blocker, 4 advisories. The worst is SB1, W9's 10 s real-time
wait against the 1 s rule, with overstated evidence. Spec: 1 blocker, 7
advisories. The worst is PB1, the missing crate-wide non-test `Runner::new`
and `hierarchy::` source-inventory pins required by T20b/W8.
