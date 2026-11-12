# ADR0017 B1/B2 corrections: independent Opus delta recheck

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, 2026-11-12. I did
the work myself with no subagents. The review was local only: no push, PR,
GitHub write, watch, merge, dispatch, settings change or CI retry. The four-PR
public pause and the babysitter are unchanged.

**Verdict: PASS.** B1 and B2 from
[the full review](0017-implementation-opus-review.md) are closed.
- **Standards: pass.** 0 findings and 1 low advisory.
- **Spec: pass.** 0 findings.

## Target and independence

- **Head.** Prepared head `a803c361147917241111f1f726d7b7c64de3a6aa` on
  `integrate/dedicated-report-validation`. I verified a clean tree at that exact
  HEAD.
- **Full review base.** `07ae3c8`, report `48dd535`.
- **Delta.** `48dd535..a803c36` = `f8191a7` (CI-stall docs) + `a803c36`
  (correction).
- **Changed files.** `executor.rs` (+12/-2), `executor/tests.rs` (+171), the
  ADR 0017 sentence, brief step 5, PROGRESS, the corrections evidence and
  `hosted-actions-stall.md`.
- **Cross-provider (independent).** The Sol correction code and tests, the
  evidence, and Sol's edits to the Opus-authored ADR and brief.
- **Not independent.** The original brief instruction that B2 corrects ("derive
  `rights` from `pin.as_ref().ok()`") was Opus-authored and wrong. That part of
  B2's origin belongs to my provider.
- **Not repeated.** The prior full validator, admissibility, output, creation,
  privacy and N3 review at `07ae3c8` is retained, not repeated. Assessment, the
  output gate, preflight ordering and drain are byte-unchanged in this delta.
  The diff touches only the struct field, the `native`/`configured`/`unavailable`
  plumbing and `capability()`.

## Read

- AGENTS and KICKOFF (routing and review rules).
- The full prior review.
- The corrections evidence.
- The full delta diff.
- The pre-N2 `capability()`/`configured()` at `ad5acb3`.
- ADR 0017's admissibility and precedence tables (rows H, I, J, lines 206–209,
  361–367, 430–432).
- The matrix oracle in `adr0017_dispatch_durable_matrix`.

## B2: capability output (closed)

Pre-N2 (`ad5acb3`) `configured()` stored `Some(map(config.rights()))` and
`unavailable()` stored `None`. `capability()` printed that value and set
`service_state_protected = rights != Some(Admin)`.

At `a803c36`, `configured()` computes the same exhaustive Standard/Admin mapping
before the config moves into the closure, and stores it regardless of
`Pin::from_config`. `unavailable()` passes `None`, and `capability()` reads only
the cached field. So for both production constructors the JSON is byte-identical
to pre-N2, including Unix configs with a missing GID and macOS admin configs
(pin `Err(AdminUnsupportedOnPlatform)`).

The pin is still the only rights source for assessment. `Pin::from_config`
derives its rights from the same `config.rights()`, so the two cannot disagree
when the pin is `Ok`. Capability stays cache-only, with no native probe.

`adr0017_capability_retains_configured_rights_with_missing_unix_gid` covers four
cases: {standard, admin} × {GID present, missing}.
- It runs `parse_inventory` with the test `FakeResolver` and
  `Layout::for_test(Linux)`.
- It asserts `pin.is_err() == gid.is_null()`, which proves the failing-pin branch
  is actually reached.
- It compares the exact JSON.
- It never calls `preflight()`, so the `NativeResolver` closure is boxed but
  never invoked. I confirmed this by reading the test.

`adr0017_backend_preflight_refusal_precedes_invalid_pin` pairs a counted
refusing backend with `Err(PreflightFailed)`. It asserts:
- `execution_reason = AccountMissing` (the backend's refusal wins over the pin
  error);
- one preflight and zero runs;
- a durable row `refused`/`preflight_failed`, read through a read-only SQL
  connection on the owned temp database;
- no quarantine;
- a clean drain.

This closes the MA9 gap the full review raised.

## B1: normative guard cells (closed)

Eight new tuples are added to the real durable matrix. The total is asserted at
81, which equals the 50 original cases, the 23 aliases and the 8 new ones.

Expected values come only from the tuple's explicit reason and the oracle's
name-based admissibility list, never from `assess`. I checked them against the
ADR:

| Cases | Expected | Basis |
| --- | --- | --- |
| standard at 4096 and 12288; admin at 8192 (elevated); admin at 12288 not elevated | I. Admissible, SID A shown (it matches), integrity and elevation stored, matches 0, output withheld, quarantine | Lines 206–207, 362–363, row I |
| Linux and macOS `post_switch` | J. Inadmissible (added to the list), observed identity NULL, matches NULL, output withheld, quarantine | Line 219, line 365, row J |
| Linux `mac_uid_sweep`; macOS `linux_cgroup` | H. Full match 1, cleanup 0, output **released**, quarantine | Line 367, row H |

Each case uses `good(..)` evidence with a nonempty payload in both streams. It
goes through the same per-case column, history, database and journal privacy
scans as the existing cases. The 81-case run passes.

## Checks run (actual, Windows, ordinary user)

| Check | Result |
| --- | --- |
| `git status` at `a803c36` | clean |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed (36.7 s) |
| `cargo nextest run --workspace --locked --no-fail-fast --status-level fail --final-status-level fail` (single run, no retry) | **230 passed, 2 skipped**, run `d4ad348c-84a2-4932-99e2-a6ce4c08a732`, 10.8 s. Matches the builder's 230/2. |

### Source mutations

I made my own production-source mutations to `executor.rs`.
- **Method.** Each mutation was an exact replacement; where there were two
  replacements, each occurred exactly once. Each was followed by the full
  workspace nextest. The original bytes were restored in `finally`, with the
  SHA-256 compared each time (`b6882f00…`).
- **Results.** All 14 compiled, so none gets build-failure credit. 13 were
  killed; one survives (S-A1).
- **Afterwards.** `git status` showed only this report's untracked file.

| # | Mutation | Run ID | Result |
| --- | --- | --- | --- |
| R1 (MA5) | Standard class drops `integrity_rid == 8192` | `7c41eacf-fee3-407b-880b-da37dc32154d` | killed: durable matrix |
| R2 (MA6) | Admin class drops `integrity_rid == 12288` | `8ed24a73-12cd-4d0e-9360-75dcd0de3c6a` | killed: matrix |
| R3 (MA7) | Admin class drops `elevated` | `6c3b5028-6074-4edc-97ee-fd6ed21b6c6d` | killed: matrix |
| R4 (MA3) | Unix accepts `post_switch` (both platforms) | `ec8a453c-28d6-44ec-9ec8-1a84ff24e203` | killed: matrix |
| R4l | Linux-only `post_switch` accepted | `fe9ddaa8-7090-4095-aed5-dbb25dc2de5d` | killed: matrix |
| R4m | macOS-only `post_switch` accepted | `6586d2bf-dc1b-4e88-81f6-6d30bae758d2` | killed: matrix |
| R5 (MA4) | Linux accepts `mac_uid_sweep` | `cf59732c-4807-49af-a3e0-0f023a5bd11b` | killed: matrix |
| R6 | macOS accepts `linux_cgroup` | `f8d0f21e-32e4-4bba-bdd6-815fde6978d2` | killed: matrix |
| R7 | `capability()` rights from the pin (B2 revert) | `9ba536ae-6740-4511-a9d7-fb065d7c6082` | killed: capability test |
| R8 | Cache configured rights only when the pin is `Ok` | `2835992e-47b2-4a76-ba6b-8a4bba61fa43` | killed: capability test |
| R9 | `configured()` maps Admin to Standard | `033c9826-5d7a-4621-9b14-e654326a590d` | killed: capability test |
| R10 | `unavailable()` caches `Some(Standard)` | `fc9adccc-ca04-493d-aba9-744f714514c3` | **survived**, 230/230 (S-A1) |
| R11 (MA9) | Pin checked before `backend.preflight()` | `a6de9531-8121-45fa-81dd-82917096ca04` | killed: new precedence test and two application tests |
| R12 | `service_state_protected` from the pin | `2a029fed-7571-4faf-a461-fc5f0f15e84f` | killed: capability test |

## Standards

**Met.**
- One-concern conventional commit.
- Every new behaviour has a meaningful test, now including the per-platform
  `post_switch` and reverse-boundary cells.
- The evidence keeps the scaffolding compile failure without kill credit.
- PROGRESS records provider, effort and outcome, and labels the corrector's
  actual medium effort honestly.
- IDs are synthetic, with no secrets or owner data.
- There is no schema, MCP, workflow or approval-flow change.
- Native stays `MechanismUnavailable`.

**S-A1 (low, advisory, test-only).** Nothing asserts
`Execution::unavailable().capability()`, so R10 survives. That constructor is used
for the unconfigured service in `store.rs:136` and `application.rs:25`. Its
`None` argument is correct by inspection and equals pre-N2. I believe the gap
predates N2, but I did not run the equivalent mutation at `ad5acb3`. A one-line
assertion would close it: `rights: null`, `service_state_protected: true`. This
does not block.

The full review's smells and A1–A3 stay open as the evidence states: discovery
`tools` only for row N, cancellation assertion strength, and indirect pin-refusal
counting in the old case. A4 is addressed by the ADR sentence. A5 is integration
hygiene.

## Docs consistency

- **Brief step 5.** It now says to cache configured rights for capability,
  independently of the pin, with no native probe. It names B2 as its origin. This
  matches the brief's own byte-identical requirement and the code.
- **ADR 0017 closing sentence.** It now separates the accepted contract (Sol
  review) from the pending implementation review and hosted gates, and says
  contract acceptance alone activates nothing. Accurate.
- **`hosted-actions-stall.md` and the PROGRESS blockers (`f8191a7`).** They
  record:
  - exactly one cancel, returning 409 ("not been queued yet");
  - exactly one rerun, returning 403 ("already running");
  - a read showing queued, attempt 1, `jobs []`;
  - billing API 404.

  They explicitly say:
  - no cause is established;
  - the 404 does not prove a lock, budget or account flag;
  - main CI after #10 is "not green" and "not claimed green";
  - nothing merges until CI recovers;
  - retries stop.

  I found no false main-green claim and no account-lock claim. The owner and
  Support step is described as an owner action, not something agents do. I did
  not re-verify GitHub state, because no GitHub reads or writes were in scope.
- **PROGRESS 2026-11-12 entry.** It matches what I reproduced: 81 cases, eight
  caught mutations, 230/2. It correctly leaves the independent recheck and hosted
  gates pending.

## Limits

- No acceptance or activation is claimed. Hosted Windows, Linux and macOS checks,
  agent-review and ci-time-budget gates are still required, and hosted CI is
  currently stalled.
- No native preflight closure, `NativeResolver`, process backend, guard, session,
  install, layout or system path ran. Nor did any SCM/LSA, privilege, account,
  service, root, admin, owner, pipe or connect, VM, Lab Manager or S4U operation.
- No ignored fixture was invoked directly. The two skipped tests are
  parent-exercised. I did not read `.git/acceptance/private-state.json`.
- Native, installed and dedicated M2 remain **unverified on real hardware** and
  incomplete.

**Totals:** Standards 0 findings, 1 low advisory (S-A1). Spec 0 findings. B1 and
B2 closed.
