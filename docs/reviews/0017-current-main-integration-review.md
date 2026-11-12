# ADR0017 N2 report-validation current-main integration review

Reviewer: Anthropic / Claude Opus 5.5 / high. Independent review sub-agent, no
subagents, 2026-11-12. This is a single-agent review on two separate axes
(Standards and Spec). It reviews the N2 integration port onto PR41 and E1 main.
It is not a fresh full review of the unchanged reviewed source. The port was
built by Sol (coordinator, gpt-6.1-sol / medium), so this review is independent
for those Sol changes. ADR0017 and its output-release amendment are
Opus-authored, so this report claims no independent architecture credit.
Independent architecture credit stays with the retained Sol contract and
output-release reviews.

Scope: read-only, apart from committing this report. There were no
implementation or PROGRESS edits and no GitHub queries or writes. PR41 was not
polled. No production constructor ran except the authorized cached-capability
seam described below. No native preflight, stored native closure, NativeResolver,
owner/SCM/LSA/privilege/account/system-path/service/fixed-pipe operation,
VM/Lab Manager/S4U probe, direct ignored-fixture invocation or private-state.json
operation ran.

## Pins

| Item | SHA |
| --- | --- |
| Reviewed target (actual `HEAD`, LOCAL, no PR) | `df60b227d3234a024007b771905690a5d3d3b7f5` |
| Base: PR41 N3 | `597d962bdd8924a88909114320e36aab987fcf41` |
| Main under PR41 (E1, #40) | `884a301eaa1e6dda6d218e439315ecdf3902ae82` (`origin/main`; the local `main` ref `176af7a` is a stale ancestor and was not used) |
| N2 port source (`integration/n2-reviewed-port`) | `95f06d3d5fe10c612d6e011caaa8067ef48f47fc` |
| Originally reviewed correction (B1/B2) | `a803c361147917241111f1f726d7b7c64de3a6aa` |
| Retained validator head | `00fdd37dd4081846937f85e46107a2de3bec5111` |
| Retained N3 port (`integration/n3-reviewed-port`) | `1976afb1a2f3e7d2b22cbf819909a0a22575be24` |

Reviewed diff: `git diff 597d962...df60b22` (one commit, 27 files: 5 runtime/test
files, PROGRESS, ADR0009 pointer, ADR0017, brief, 2 evidence docs and 16 retained
review artifacts). `597d962` is byte-identical to N3 `1976afb` for `executor.rs`
and `executor/tests.rs`. N3 `a26570d..1976afb` does not change either file.

## Source identity (checked here from Git objects)

| File | `df60b22` blob | Also equal at |
| --- | --- | --- |
| `crates/arxd/src/service/executor.rs` | `3943b9c` | `95f06d3`, `a803c36`, `00fdd37` |
| `crates/arxd/src/service/executor/tests.rs` | `ce3327a` | `95f06d3`, `a803c36`, `00fdd37` |
| `crates/arxd/src/service/application/tests.rs` | `18e6639` | `95f06d3`, `a803c36`, `00fdd37` |
| `crates/arxd/src/store.rs` | `7f2bdf6` | `95f06d3`, `a803c36`, `00fdd37` |
| `crates/arxd/src/local_execution_tests.rs` | `0d23f27` | `95f06d3` |

- The blob ids match, so the files are byte-identical, line endings included. All
  27 changed files have 0 CR bytes and no BOM, and `.gitattributes` is
  `* text=auto eol=lf`. `git diff --check 597d962 df60b22` is clean.
- `git diff 597d962 df60b22 -- local_execution_tests.rs` is exactly one line:
  `backend_fixture(probe.clone())` becomes `default_fixture(probe.clone())`.
  This is the minimal adapter. N2 renamed the base's one-argument
  `backend_fixture` to `default_fixture` (`executor/tests.rs:305`) and added a
  two-argument `backend_fixture(backend, pin)` (`:852`). The rest of the main SDK
  fixture is unchanged: `CancellationProbe`, the 250 ms hold, the automatic
  `ServiceError::Timeout` and explicit-cancel arms, and the durable
  `("failed","cancelled")` before-shutdown assertion.
- The diff touches only these five source files. No other `crates/`, `tools/`,
  `.github/`, `Cargo.toml` or `Cargo.lock` path changed, and neither did the
  E1/owner, client, native, config, MCP, `service/mod.rs` or `application.rs`
  sources. `git diff 95f06d3 df60b22 -- crates` lists only the five E1 files
  inherited from main.
- Docs equal `95f06d3`, except for E1 docs inherited from main and the one
  attribution line in `n2-integration-port.md` (owner → coordinator).

## Standards

Verdict: **PASS**. 0 findings and 4 low advisories.

Standards applied: AGENTS.md, CONTRIBUTING.md, KICKOFF routing/privacy (lines
89–103) and the smell baseline.

- **S-A1 (low, docs typography).** The new PROGRESS current-state lines drop the
  space before several identifiers: `main884a301`, `run43187047371`, `in428s`,
  `port95f06d3`, `passed241/241 with2`, `runbdbf032b`. The facts are correct
  (`bdbf032b` is the held E2 run). The previous checkpoint has the same pattern.
  This is cosmetic.
- **S-A2 (low, KICKOFF:103).** The PROGRESS line for this integration task records
  the outcome but not the provider/effort. The evidence doc does record "OpenAI /
  gpt-6.1-sol / medium". The earlier N3 line names "Coordinator Sol medium". Add
  it when PROGRESS is next updated.
- **S-A3 (low, discoverability).** No document links
  `docs/evidence/n2-current-main-integration.md`. The "Local integration
  checkpoint" in ADR0017 still names the earlier base (main #36 `0937cac` on N3
  `1976afb`). That checkpoint is labelled historical, so it is not wrong, but the
  current base evidence can only be found by path. A pointer from PROGRESS or the
  ADR0017 checkpoint would help.
- **S-A4 (smell, judgement call; in already-reviewed bytes).** Possible Mysterious
  Name. `application/tests.rs:462` imports `default_fixture as backend_fixture`.
  The same file at `:702` imports the different two-argument `backend_fixture`.
  Two helpers with different signatures share one local name. This has no
  behavioral effect. It is not introduced by the integration (blob equal to
  `a803c36`), so it does not block.

Other observations (no finding):
- The `expect("preflight pin")` calls in `complete` are unreachable in production.
  The worker runs `this.preflight()` before `backend.run`, and `preflight` returns
  the pin `Err` after backend preflight. A test backend that bypassed this would
  panic inside the JoinSet worker. The next `try_join_next` would then quarantine,
  or `drain` would return `ExecutionFailed`, so the panic stays contained.
- No secrets or private owner data appear in the diff. Test SIDs and markers are
  synthetic constants.

## Spec

Verdict: **PASS**. 0 findings.

Spec: ADR0017 (typed-report contract plus output-release amendment), ADR0009
pointer, `docs/briefs/0017-report-validation-implementation.md` and the integration
manifest/evidence. Each item below was checked independently against `df60b22`:

- **N3 drain/registration/quarantine priority retained.** The `executor.rs` diff
  from N3 changes dispatch in one place only. The accepted worker's pre-spawn
  `Report::Completed{Cancelled,…}` becomes typed `Report::CancelledBeforeSpawn`.
  It still maps to `Failed`, `stop_reason=cancelled`, no reason and no quarantine,
  the same as the old `terminal`/`normalize` result. No `+`/`-` line touches
  `workers`, `drain`, `try_join_next`, `spawn_blocking` or the stopping branch. The
  synchronous stopping refusal still calls `preflight()`, which returns
  `Quarantined` before `Stopping` and never reaches backend preflight. All base
  N3 test functions remain. Only the two tests of the removed
  `normalize`/`terminal`/`Observation::metadata` were replaced by the ADR0017
  matrix, as in the reviewed source.
- **Immutable pin and preflight precedence.** `pin: Result<Pin, Unavailable>` is set
  once in `native()`/`configured()` and has no setter. `preflight` checks, in
  order: quarantine, stopping, `backend.preflight()?`, then pin. Backend refusal
  therefore precedes pin refusal.
- **Pure assessment.** `assess` performs no IO and checks, in order: point per
  platform, observation shape (store-owned `observed_id_valid`), Windows token
  class table (exhaustive over rights × elevation type × group state), Unix
  uid+gid, boundary per platform, then stop/exit coherence. Reason precedence is
  cleanup/boundary → identity mismatch → missing identity, invalid stop or invalid
  discovery → none. Quarantine equals `reason.is_some()`. Success requires no
  reason plus `Exited` with exit code 0.
- **Privacy and sanitized Metadata.** A mismatched SID, UID or GID is withheld
  (`then_some` only on equality). `Observation`'s `Debug` prints `<withheld>`.
  Only the sanitized `Metadata` leaves `assess`.
- **Full-match output gate.** `identity_matches_pin == Some(true)` is the only arm
  that calls `output_value`. `Some(false) | None` returns fixed
  `{"head":"","tail":"","dropped_bytes":null}` for both streams, regardless of
  cleanup or outcome, as the amendment specifies.
- **Capability precedence.** `capability()` reads cached `configured_rights`, which
  is independent of pin validity (B2 fix). The authorized test
  `adr0017_capability_retains_configured_rights_with_missing_unix_gid` builds
  `Execution::configured` from a `parse_inventory` wire with `FakeResolver`. It
  reads only `e.pin` and `e.capability()`. It never calls preflight, the stored
  native closure, NativeResolver, account lookup or a backend. Its assertions are
  unchanged from `a803c36`.
- **Creation boundary.** `Failed{NotInvoked|ProvenNone}` gives `ExecutionFailed`
  with no quarantine. `Failed{Created|Unknown}` gives `CleanupUnconfirmed`,
  `cleanup_confirmed=false`, pin rights and quarantine.
- **No activation.** Production `Native` is unchanged: preflight returns
  `MechanismUnavailable` after its check, and `run` refuses. The `store.rs` change
  only extracts the identifier-syntax helper and keeps the 184-byte cap. There is
  no migration, and `user_version` stays 3.
- **PROGRESS.** The change only adds lines (+9/−0). Current state is first,
  followed by "Historical checkpoint before report-validation integration", so
  the history from main is preserved. There is one current queue-priority line:
  PR41 awaits Windows CI, N2 is a separate local child awaiting this review, and
  E2 stays held by `bdbf032b` with instrumented passes not waiving it. ADR0018
  `29068df` is explicitly labelled local
  (`docs/owner-cli-final-contract-corrections`). M2 is still incomplete, and
  native/installed/dedicated paths remain **unverified on real hardware**.
- **Attribution change.** In `n2-integration-port.md`, owner → coordinator matches
  the actual authorizer of the pure cached-capability test. It grants no
  additional authority.

## Evidence credits

Credited to their pinned runs, not re-run or claimed here:
- Original full Opus implementation review `48dd535` at prepared head `07ae3c8`.
  It found B1 (missing token/observation/boundary cases) and B2 (capability rights
  regression), with 228/2 at that head.
- B1/B2 correction `a803c36` and Opus recheck `7758f64` **PASS** (Standards 1 low
  advisory, Spec 0). The source reviewed there is byte-identical to the four
  original files here.
- Sol contract review `86b0a40b` (changes required), Sol recheck `1d78d74d`
  **PASS**, and Sol output-release review `0132c3e` **PASS**.
- Historical mutation rows (`0017-local-mutations.jsonl`,
  `0017-output-release-mutations.jsonl`) and the evidence docs apply to identical
  implementation/test bytes. No new kills are claimed, and historical
  survivors/advisories remain disclosed.
- Coordinator checks on this integrated source: fmt, locked workspace
  all-targets Clippy, and one full normal nextest run with **241 passed, 2
  skipped** (run `ce6a9bc3-1f42-468c-97da-646020c4d18d`, 11.263 s), plus
  UTF-8/LF, relative-link and diff checks.

Checks run by this reviewer at `df60b22`:
- Git blob identity across `df60b22`/`95f06d3`/`a803c36`/`00fdd37`, CR/BOM scan
  of all 27 files, `git diff --check`, scope and ancestry checks (`597d962` ≡
  `1976afb` for the executor files; `a26570d` is an ancestor of `a803c36`).
- `cargo fmt --all -- --check`: pass.
- One focused run that uses only fakes and owned temp dirs: `cargo nextest run
  --locked -p arxd --no-fail-fast -E 'test(service::executor::tests::) |
  test(=…dedicated_report_mcp_privacy_in_memory) |
  test(=…endpoint_failure_still_drains_dedicated_worker_before_stop_audit) |
  test(dedicated_fake_sdk_cancellation)'`. Result: **36 passed**, 206 skipped,
  2.882 s. The run ID was not captured. This includes all 32 executor tests
  (durable matrix, boundary cases, creation phases, the cached-capability test,
  N3 late-dispatch and quarantine tests), both SDK cancellation tests and both
  changed Application tests. The pre-existing
  `production_refuses_normal_user_before_fixed_config_access` was deliberately
  excluded because it calls `native::require_service_identity`.
- `cargo nextest list --workspace` reconciliation: 241 non-ignored tests plus 2
  ignored fixtures (`owner_fixture`, `hot_journal_child`, listed but not run),
  which matches the coordinator's 241/2.
- Not run here: Clippy, the full suite and mutations.

ExecutionFailed did not recur in the focused run. Its cause is not established
and no fix is claimed. E2's red run `bdbf032b` stays held, and nothing here
waives it.

## Disposition

**Review PASS for publication** of this N2 integration as its own one-concern PR,
stacked on PR41 `597d962`. Merge additionally requires PR41 to land first, a
re-pin if the base changes, all hosted required checks (CI, `ci-time-budget`,
`agent-review`) and coordinator-only merge. The advisories do not block. Native,
installed and dedicated paths remain **unverified on real hardware**. Milestone 2
is incomplete.

Standards: 0 findings (worst: S-A2 missing provider/effort in PROGRESS, low).
Spec: 0 findings.
