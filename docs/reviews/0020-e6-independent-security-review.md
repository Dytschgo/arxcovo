# ADR 0020 Slice E6 independent security and source review

Reviewer: Anthropic / claude-opus-5-5 / high, 2026-11-13. Sole review
sub-agent, no subagents. This is the first independent security, standards
and spec review of the Sol high E6 readiness/admission implementation. Opus
wrote the ADR 0020 architecture and correction 1, and those keep their own Sol
reviews. This review claims independence only for the Sol E6 implementation,
not for the ADR 0020 architecture.

**Standards: APPROVE, no blockers.** Two advisories (SA1, SA2).

**Spec (including security): APPROVE, no blockers.** Five advisories
(PA1-PA5). The production behavior I read matches ADR 0020 D5, D6, D8 (bridge
parsing), D10 and A5, and brief E6 T6/T9/T11-T20. Production stays inert.

This review grants no publication, integration, activation, cgroup setup,
experiment or M2 acceptance. Publication still needs a PR, a different-provider
review of the exact published head and all required checks. Native, installed
and dedicated paths remain **unverified on real hardware**. M2 remains
incomplete. The historical dispatcher privacy flake (`65762554`),
`E_PROTECTION` and `ExecutionFailed` remain unknown. This review claims no fix,
diagnosis or retry.

## Pins and isolation

- Worktree: `C:/Users/DylanFerraro/.t3/worktrees/Arxcovo/t3-00fb6c95-...`,
  branch `t3/review-e6-security`. Its top level is the worktree, not the
  canonical checkout `D:/Code/Arxcovo`. Status was clean at the start and after
  every mutation run. HEAD `25b27c0d015d9f771a73a0eb5a5e2ae388a822ee`.
- Original target `9699c8c` (evidence) over source
  `87957ddd4d1f58a9833d14e4ce204a046324e497`, parent `c5e3e7a`.
- Port: `b9b68d9` (source), `9864792` (evidence) and `25b27c0` (PROGRESS line)
  sit on `46c394a` and `6778838`, over merged E4 main
  `f4fb126826ea0aafe3bc28ee4a7c3c1cc2edb730`
  (`git merge-base --is-ancestor` exit 0). The `f4fb126` tree `f946208`
  equals the `9937c16` tree.
- Byte identity: `git diff 87957dd HEAD` over the 8 source paths is empty.
  `docs/evidence/linux-readiness-e6.md` equals `9699c8c`. The base blobs of
  all 8 paths are identical at `c5e3e7a` and `f4fb126`. The 3 new files are
  absent from both, so the port is exact. `c5e3e7a` differs from `9937c16`
  only in `docs/PROGRESS.md` and `docs/evidence/linux-rights-probe-e4-native.md`.
  Both are preserved on the port branch.
- Scope (8 source paths): `exec/stage_b_tests.rs`, `service/executor.rs`,
  `service/executor/tests.rs`, `service/unix_bridge.rs`,
  `service/unix_bridge/readiness.rs` (new), `service/unix_bridge/tests.rs`,
  `service/unix_bridge/tests/e6.rs` (new) and
  `scripts/test_readiness_e6_mutations.py` (new).
- Read: AGENTS, KICKOFF, the review skill, ADR 0020 (D1, D2, D5, D6, D8, D10,
  A5 and the single-admission-domain rule), brief gates, E5, E6 (T6, T9,
  T11-T20, mutations), the E7 boundary (T20b, W3e, W8), and the E6 evidence.

## Standards

Sources: AGENTS (small one-concern change, a meaningful test for every
behavior, honest evidence, no secrets in diagnostics), brief CI rules
(no sleep over 1 s, fmt, locked Clippy), plus the smell baseline.

No blockers. Formatting, Clippy and test timing comply. The bounded
rendezvous loops use `yield_now` and give up after 5 s, with no `sleep`.
The new code carries no payload. `Cause` and the refusal tokens are fixed
variants. `Debug` is derived only over fixed enums.

- **SA1 Primitive Obsession (judgement).** `Readiness::check` matches raw
  literals `1, 4, 6, 7` and falls back with `_`. Those literals are coupled
  to the `#[repr(u8)] Cause` discriminants (`Setup = 2` onward). If someone
  reorders `Cause`, the tokens remap silently. Only T15/T16 would catch it:
  my `stopped_wrong_token` and `hardening_as_janitor` mutants were killed.
  The `_` arm is unavoidable over `u8`. The `SetupFailure -> Cause` map itself
  is exhaustive with no wildcard, as required. Suggested fix: name the state
  constants (`const READY: u8 = 1`), or match on `x if x == Cause::Janitor as u8`.
  Not blocking.
- **SA2 Tautological assertion.** `l6_e3_tokenized_source_inventory_and_mutants`
  asserts `*name != "hierarchy/fake.rs"` over a literal array that never
  includes it, so the check cannot fail. The evidence sentence "excludes
  fake.rs by name" overstates it. The current set is complete:
  `exec/hierarchy/` holds exactly `fake.rs`, `linux.rs`, `parse.rs` and
  `setup.rs`, and all production files plus `hierarchy.rs` and `readiness.rs`
  are in both the P-SCAN inventory and the E3 denylist set. But a future
  production file would go unscanned without any failure. Suggested fix:
  derive the list from `hierarchy.rs`'s `mod` declarations, and assert that
  `fake` is the only `cfg(test)` module left out. Not blocking.

Smells considered and dropped. `AdmissionDomain` looks like a Middle Man, but
ADR 0020 D6 requires exactly this sole ownership point, so the repo wins.
There are no Repeated Switches: the D5 table lives once in `check` and once in
`From<SetupFailure>`.

## Spec (including security)

Requirements verified by source reading, with the test or mutant that pins
each one:

- **Readiness (D5, T16).** One `Arc<AtomicU8>`. `set_ready` does a CAS 0→1.
  `fail` does a CAS 0→cause. `latch` does a CAS 1→cause, and only for
  `Containment | Rights`. The first writer wins, nothing returns to `Ready`,
  and there is no reset, replace or rebind API. Concurrency precedence: a
  concurrent Containment and Rights latch keep whichever CAS lands first.
  Setup `fail` and the bridge `latch` act on disjoint source states. My
  `set_ready_store` and `fail_store` mutants were killed.
- **D5 table (T15).** Hardening → `mechanism_unavailable`/`preflight_failed`.
  Setup variants → the same. Janitor (`KillTimeout`, `LeafRemoval`,
  `VerifyFailed`, `Panicked`) → `quarantined`/`quarantined`. `Stopped` →
  `stopping`/`shutdown`. Pending, Containment → `mechanism_unavailable`.
  Rights → `rights_mismatch`/`preflight_failed`. Stored reasons come from the
  existing `stored_reason`.
- **Preflight precedence (D5).** Config check, then stored construction
  result, then `readiness.check()`, unchanged. `t1_preflight_order_*` passes.
- **In-slot gate (D6).** `body` runs inside the slot. It returns a
  construction refusal first, then latches on `cleanup_residue()`, then
  checks readiness, and refuses with zero port calls. T18 (Containment and
  cancelled+residue cases) and T13c cover it. My `barrier_after_fetch_add`
  mutant was killed, which shows the seam position is load-bearing in T18.
- **Post-call latch (D6, U3).** It sits after `runner.run` and before
  `match outcome`, inside `body`, and therefore before `fetch_sub`. It fires
  on residue, `NotStarted::Containment` or `NotStarted::Resource`. The report
  is unchanged: T11 row D, T12 row F, T13b row E. The rights latch is also
  inside `body`. My `u3_conservative_latch_removed` mutant (only the
  error-based latch removed, residue latch kept) was killed by T11.
- **Plain cancellation (T13).** With the residue flag clear, six cancelled
  calls all reach the port, so there is no false cap. The builder's
  `cancelled_latched` mutant was killed.
- **Bound (T19).** Four held residue calls, then six refusals with zero
  calls, four row E and six `preflight_failed` rows.
- **Rights line (D8 bridge parsing, T6/T14/T17).** Exactly `TOOLS.len() + 1`
  lines. The last line must be a strict `rights_line` (E1, 96-byte bound).
  The zip pairs the first `TOOLS.len()` lines with ordered `TOOLS`. A missing
  line, two rights lines or a malformed table gives K with no tools. A valid
  table with failing `classify_rights` (`exit_zero`, `untrusted`, `unknown`)
  latches `Rights` and keeps N with tools. `absent` and `exit_nonzero` stay
  available. The rights text, `no_new_privs` and `sudo` are absent from
  responses, rows and database bytes. My `unknown_not_latched`,
  `rights_latch_unconditional`, `rights_wrong_cause` and
  `extra_rights_line_accepted` mutants were killed.
- **Single domain (D6, T20).** `AdmissionDomain` (`cfg(test)`, private, no
  `pub`) consumes an `FnOnce` factory once and keeps one `Arc<Execution>`.
  That shares one backend, one readiness and one slot counter. `handle()`
  only clones the `Arc`. Dropping and recreating handles keeps the latch, with
  one allocation. There is no replace, reset, rebind or second-runner method.
  The ticket allocator, startup task, daemon row, cache and wiring are absent,
  as the E7 boundary requires.
- **Executor seam (brief E6).** Exactly one `cfg(test)` barrier, after
  `preflight` and before `active.fetch_add`. The lock is held only to clone
  the hook. Production statements and `AcqRel` operations are unchanged, only
  nested in a new `else`. There is no other `admission_barrier` use.
- **Production inertness (D1, D10, T9).** `mod unix_bridge` is compiled in
  production under `#![allow(dead_code)]`, but `UnixBridge::new`,
  `AdmissionDomain`, `unix_bridge_for_test` and `admission_barrier_for_test`
  are all `cfg(test)`. No production path can construct a bridge, so the new
  `Readiness`, `RunnerPort::cleanup_residue` and rights parsing are
  unreachable. `Execution::configured` is unchanged (`Self::native(`), and
  the existing pin tests pass. There is no Stage G activation.
- **P-SCAN (D10).** The inventory adds `readiness.rs`, `hierarchy.rs` and
  `hierarchy/{parse,setup,linux}.rs`, with the existing shim, rights, runner
  and bridge files. The expected panic inventory is unchanged, so the new
  files have no panic payloads. The E3 denylist (`kill(`, `killpg(`,
  `setpriority(`, each with a mutant control) now also covers `parse.rs` and
  `setup.rs`. A full-text grep of the hierarchy files finds only the
  `"cgroup.kill"` control-file string.

Advisories:

- **PA1 Lexical bypass not disclosed.** I added a temporary reviewer test,
  restored byte for byte, and confirmed that `e3_source_allowed` accepts
  `use libc::kill as stop; stop(p, 9)`, `(libc::kill)(p, 9)` and
  `libc::syscall(libc::SYS_kill, p, 9)`. A direct `libc::kill(p, 9)` is
  rejected. The denylist is therefore a lexical regression tripwire, not a
  semantic proof. D10 asks only for the tokens and their mutant
  self-tests, which exist, and today's sources contain none of these forms.
  The E6 evidence should still say "lexical". Optionally deny `syscall` and
  `as` re-imports of denied names in hierarchy files. I do not claim the
  test-module truncation is a bypass: my probe of that case was rejected.
- **PA2 T16 race shape.** The main thread latches `Rights` before releasing
  the 8 workers. So the test shows that a first cause survives 8 concurrent
  late writers. It does not show 8 racing first writers converging. That is
  still meaningful: a plain-store `latch` mutant was killed 8 of 8 runs.
  Optional: add an unprimed round asserting that all 8 observations agree.
- **PA3 Rebuild mutant is structural.** The builder's `domain_rebuilt`
  replaces `handle()` with `Execution::unavailable()`. `Arc::ptr_eq` kills it
  before the shared-latch assertion runs. A faithful "rebuild runner or
  readiness on handle" mutant cannot be written: the factory is consumed and
  `Readiness` moves into the bridge, which the source pin backs up. Test code
  can still call `AdmissionDomain::new` twice, or share one `Readiness` clone
  across domains. E7's T20b/W8 must forbid both in wiring.
- **PA4 Unwind path of the bound.** If the runner unwinds, `body` skips the
  post-call latch, and `run` returns `Failed { Unknown }` (existing
  quarantine). The D6 bound still holds. `Group::drop` stores the flag
  (`Release`) before `catch_unwind` returns and before `fetch_sub`. Every
  later `fetch_add` (`AcqRel`) is followed by the gate's `Acquire` read. On
  this path, "flag before release" stands in for "latch before release". It
  is untested. Suggest one ADR sentence, or a fake-port panic case in E7.
- **PA5 Fixed classification inputs.** The bridge classifies with
  `Rights::Standard, Hardening::Standard`. The evidence discloses this. It
  is sound only because `derive` refuses non-Linux and admin configurations,
  and `Ready` implies S0 passed. E7 must keep a hardening failure as `fail()`
  before `Ready`.

## Credits and limits

Builder runs at source `87957dd` only. I credit them by the byte identity
above and did not repeat them:

- `cargo fmt --all -- --check`
- workspace `cargo clippy --workspace --all-targets --locked -- -D warnings`
- focused E6 suite: 9 passed
- `cargo nextest run --workspace --all-targets --locked`: 375 passed, 3
  skipped
- 11 compiled mutation kills

I reran none of these as a full suite: no material concern called for it. The
failure history is honest. Three tuple substitutions failed to compile, a
T17 assertion was over-broad, and the initial post-residue mutant survived
because the next gate masked it. The test now observes readiness before the
second dispatch. A compile slip was fixed before validation. No compile error
was counted as a kill. I inspected the mutation script. It requires the
absence of `error[E` and `could not compile`, plus a `<test> ... FAILED`
line, and it restores both files in `finally`.

Reviewer runs at HEAD `25b27c0` on Windows x86_64-pc-windows-msvc:

- `cargo fmt --all -- --check`: exit 0.
- `cargo test -p arxd --lib --locked service::unix_bridge::tests::e6`: 9
  passed.
- `cargo test -p arxd --lib --locked -- p_scan l6_e3 t6_discovery_exact_table t1_ service::executor`:
  40 passed.
- `cargo clippy -p arxd --all-targets --locked -- -D warnings`: clean. This
  is package-scoped, not the workspace.
- Reviewer mutants from a scratch script, each compiled and killed by its
  named test: `u3_conservative_latch_removed`, `unknown_not_latched`,
  `rights_latch_unconditional`, `rights_wrong_cause`,
  `extra_rights_line_accepted`, `set_ready_store`, `fail_store`,
  `stopped_wrong_token`, `hardening_as_janitor` and
  `barrier_after_fetch_add` (10 of 10). A T16 plain-store stability check
  was killed 8 of 8, and the PA1 lexical probe ran. Every run ended with
  SHA-256 exact restore and a clean `git status`.

Limits:

- A Windows run does not compile or run `cfg(target_os = "linux")` code
  (`hierarchy/linux.rs`, the E4 Linux rights probe). It proves nothing about
  Linux or macOS paths.
- No hosted CI, and no GitHub push, PR, watch or merge.
- No native `Runner`, owner, session, account, service, `/proc`, `/sys`,
  cgroup, root, VM, private state, experiment or ignored native fixture.

## Summary

Standards: 0 blockers, 2 advisories; worst is SA2, the tautological
fake-exclusion assertion. Spec: 0 blockers, 5 advisories; worst is PA1, the
lexical kill-denylist bypass that the evidence does not disclose.
