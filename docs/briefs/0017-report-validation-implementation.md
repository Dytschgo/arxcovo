# ADR 0017 implementation brief (Sol)

For: OpenAI gpt-6.1-sol / high, builder. The corrected
[ADR 0017](../adr/0017-dedicated-report-validation.md) is accepted for local
implementation by the [Sol recheck](../reviews/0017-report-contract-sol-recheck.md).
Obtain an independent Opus implementation review and all normal gates before merge. The
[correction disposition](../reviews/0017-report-contract-correction-disposition.md)
lists what changed since the first review.

**Output-release amendment: accepted for local implementation.** The section
[Output-release amendment](#output-release-amendment-accepted) below is a
follow-up slice on top of the completed validator (`c63f6cb`, evidence
`c394ec5`, base `2f3b4863`). The [independent Sol review](../reviews/0017-output-release-sol-review.md)
accepts proposal `0132c3e` and this section for local implementation. The rest of this brief
is unchanged and already implemented.

Base: the accepted ADR head. Its runtime equals `4abf276a` and PR36 head
`5c3c01e3`; every later commit is docs only. Rebase onto the N3 change if it
merges first. Keep validation in one pure function so the conflict surface
inside `dispatch` is a single call site. Do not change drain, the `JoinSet`
handling or the concurrent-drain behaviour (N3).

Local integration checkpoint, 2026-11-12: the original design/runtime bases above
are historical. This port follows retained N3 `1976afb1a2f3e7d2b22cbf819909a0a22575be24`
on actual main #36 `0937cac`. The accepted typed-report, identity-privacy and
output-release contract is unchanged. The [full Opus implementation review](../reviews/0017-implementation-opus-review.md)
and [B1/B2 PASS recheck](../reviews/0017-blocker-corrections-opus-recheck.md)
are retained for their original heads. Fresh coordinator Opus high integration
review and hosted gates remain required before publication/merge. See the
[port evidence](../evidence/n2-integration-port.md). This establishes no native
activation or M2 acceptance.

## Scope

Change only these files:

- `crates/arxd/src/service/executor.rs`
- `crates/arxd/src/service/executor/tests.rs`
- One `pub(crate)` syntax helper in `crates/arxd/src/store.rs`
- `crates/arxd/src/service/application/tests.rs`, test only: one MCP
  end-to-end privacy test (see [Privacy](#privacy)).
- Docs updates: PROGRESS, and adding the ADR status line after review.

Why the scope grew by one test file, and no further: the identifier rule lives
in `assess`. The response's `observed_identity`, its nested `metadata` and the
row all come from the `Metadata` that `assess` returns. `list_activity` reads
that row, and `mcp.rs` sends the dispatch value unchanged. So no production
change outside `executor.rs` is needed. The application test is the cheapest
proof that the real MCP text path, `list_activity` and `list_hosts` carry no
protected identifier. To use it, make `OnceReport` and the pin-taking
`backend_fixture` `pub(crate)` in `executor/tests.rs`.

Do not touch:

- schema, migrations or vocabularies
- `mcp.rs`, or the response shape and keys
- `Activity::list` or any other store read path
- the development executor
- config, preflight mappings, provisioning or owner code
- workflows

## Symbols

1. **`store.rs`:** extract `pub(crate) fn observed_id_valid(kind: ObservedKind,
   id: &str) -> bool` from `Metadata::validate`. That includes the 184-byte
   bound. `validate` must call it, so both checks stay identical.
2. **`executor.rs`, new types:** `Creation`, `Evidence`, `ElevationType`,
   `GroupState` and `Pin`, exactly as in ADR 0017.
   - Rework `Observation`: remove `matches`. Add `elevation_type` and
     `administrators` to `Windows`.
   - Write `Debug` for `Observation` by hand. It prints the variant and the
     token-class fields, and `<withheld>` in place of `sid`, `uid` and `gid`.
   - Delete `Observation::metadata`. Its replacement is `assess`.
   - Derive `Debug, Clone, Copy, PartialEq, Eq` where used, except that
     `Observation` has the hand-written `Debug`.
3. **`Report`:**
   - Add `CancelledBeforeSpawn`.
   - Replace `Failed { after_spawn: bool }` with `Failed { creation: Creation }`.
   - Replace `metadata: Metadata` with `evidence: Evidence` in `Discovered` and
     `Completed`.
   - `FakeForTest` is unchanged.
4. **`Pin`:**
   - `Pin::new(platform, identity, gid, rights) -> Result<Pin, Unavailable>`,
     following ADR 0017's pin-construction rules.
   - `Pin::from_config(&ServiceConfig, Platform)`, used only by
     `Execution::configured`. Take the platform from the layout, the same
     value `validate_platform_rights` uses.
   - `#[cfg(test)] Pin::for_test`.
   - Fields private, no setters.
5. **`Execution`:** hold `pin: Result<Pin, Unavailable>` for assessment and
   retain cached configured rights independently for capability reporting.
   - `unavailable()` uses `Err(NotConfigured)` and no configured rights.
   - `configured()` caches the rights from config even if pin construction fails.
     `capability()` uses those cached rights, preserving its pre-validator output
     for inventory-valid Unix configs with a missing GID. In particular an admin
     config still reports admin and service_state_protected=false. It does no
     native probe. The full implementation review identified this correction as B2.
   - In `preflight()`, check the pin after `backend.preflight()` succeeds.
6. **The pure `fn assess(pin: &Pin, evidence: Evidence, discovery_valid:
   Option<bool>) -> (Outcome, Metadata, bool /*quarantine*/)`** implements
   rows H–N, in precedence order.
   - Use exhaustive matches with no wildcard over `Platform`, `Rights`,
     `Observation`, `ObservationPoint`, `TreeBoundary`, `StopReason`,
     `ElevationType` and `GroupState`.
   - It sets `rights` from the pin and `identity_mode = DedicatedAccount`.
   - It sets observed fields only when the observation is admissible:
     - Windows: `observed_gid` NULL, `elevation = elevated`.
     - Unix: `integrity_rid` and `elevation` NULL.
   - It computes `identity_matches_pin` from the unredacted observation, then
     applies the identifier rule:
     - `observed_id` = the observed string only if it is byte-equal to the pin
       identity, else NULL.
     - `observed_gid` = the observed GID only if it equals the pin GID, else
       NULL.
   - It never copies `pin.identity` or `pin.gid` into an observed field.
   - It writes `cleanup_confirmed = Some(false)` under row H.
   - It never formats, logs or stores `Evidence`.
7. **`dispatch`:**
   - The dispatcher cancel path (currently a synthetic `Completed`) becomes
     row C, built directly, not through `assess`.
   - Map `CancelledBeforeSpawn` to row E.
   - Map `Failed { creation }` with an exhaustive `match` and no wildcard:
     `NotInvoked | ProvenNone` gives row F; `Created | Unknown` gives row G.
     Drop the dead reason literal (advisory 3) and set `rights` from the pin
     for G.
   - `Completed` and `Discovered` go through `assess`.
   - Remove `normalize`, `terminal`, `invalid_discovery` and `post_spawn_error`
     from the spawned path. Quarantine comes only from `assess` or row G.
   - Return `tools` only for row N.
   - Keep building `observed_identity` and `metadata` from the `Metadata` that
     `assess` returned. Add no second source.
8. **Docs:** update the `Executor` trait comment with the ADR 0017 report
   contract:
   - the boundary is the call to the creation primitive, not exec;
   - which `Creation` each failure may claim, including that Unix
     `Command::spawn` `Err` and Windows `CreateProcess*` `FALSE` are `Unknown`;
   - no cancellation between that call and the end of the identity read;
   - that a protected identifier must never leave `Evidence`.

## Required tests (real `dispatch`, durable row)

Each case does the following:

1. Call `start_dedicated`.
2. Call `backend_fixture(OnceReport)` with a `Pin::for_test`.
3. `dispatch(...).await`.
4. Assert the response `outcome`, `reason` and `observed_identity`.
5. Read the row with rusqlite: `outcome`, `reason`, `stop_reason`, `exit_code`,
   `rights`, `observed_kind`, `observed_id`, `observed_gid`, `integrity_rid`,
   `elevation`, `observation_point`, `identity_matches_pin`, `tree_boundary`,
   `cleanup_confirmed` and `identity_mode`.
6. Assert `quarantined`.
7. For quarantining rows, check that a second dispatch is `refused/quarantined`
   with zero backend calls.

Run at least one positive and one negative case through `Activity::list`
(`list_activity`) as well. Use one table-driven test with one named case per
row; the diagnostic `diagnostic_n2_report_acceptance` must not be adopted.

Use these synthetic identifiers. Each protected value differs from every pin
value:

| Role | Windows | Unix |
| --- | --- | --- |
| Pin A | `S-1-5-21-100-200-300-1001` (existing fixture) | UID 61001, GID 61001 |
| Protected B | `S-1-5-21-700-800-900-4242` | UID 62002, GID 63003 |

**Positive controls.** Each must succeed with no quarantine, and
`observed_id` must equal the observed value, which equals pin A:

- Windows standard: 8192, `Default`, not elevated, `Absent`, `windows_job`.
- Windows admin pin: 12288, `Full`, elevated, `Enabled`.
- Linux `pre_exec`, `linux_cgroup`; `observed_gid` = 61001.
- macOS standard, `mac_uid_sweep`.
- Discovery row N, returning the fixed table.

**Probes.** P0–P4 exactly as in the ADR 0017 disposition table. P2 stores
pin A's SID as the observed build identity.

**Negatives.** Each case, with its expected row:

- SID B (I; `observed_id` NULL).
- `DenyOnly` (I).
- `PresentOther` (I).
- `Limited` (I).
- `Default` plus elevated (I).
- Standard token against an admin pin (I).
- Unix UID B with GID A (I; `observed_id` NULL, `observed_gid` 61001).
- Unix GID B with UID A (I; `observed_id` "61001", `observed_gid` NULL).
- Unix observation on a Windows pin (J).
- Windows observation on a Linux pin (J).
- `pre_exec` point on Windows (J).
- `child` point on Unix (J).
- Malformed SID `S-1-x` (J), and a 185-byte SID (J). Neither may produce
  `StorageFailed` or leave the row `started`.
- `linux_cgroup` on Windows (H).
- `process_group` on macOS (H).
- `cleanup_confirmed = false` (H).
- `cleanup_confirmed = None` with SID B (H; `observed_id` NULL, matches 0).
- Stop NULL (K).
- `exited` without a code (K).
- `timeout` with a code (K).
- Stop `failed` with everything else valid (K).
- An invalid discovery table (K, no tools).

**Stop causes.** For each of `timeout`, `cancelled`, `output_limit` and
`shutdown`:

- With valid evidence: row L, no quarantine.
- With cleanup NULL: row H, quarantine.
- Exited non-zero: row M.

**Pre-spawn and creation phase.**

- Rows C and E: no observed fields, NULL rights, no quarantine.
- Row D for `MechanismUnavailable` and `Busy`.
- Row F for `Failed { NotInvoked }` and `Failed { ProvenNone }`: no quarantine.
- Row G for `Failed { Created }` and `Failed { Unknown }`: quarantine,
  `rights` = pin.
- **Required phase test, created-not-execed (review P1).** Use a
  `#[cfg(test)]` `PhaseScript` backend. Its script records "creation primitive
  called" and "child created", then fails at a pre-exec step. It sends no
  identity bytes and confirms no cleanup, and it returns
  `Failed { creation: Created }`. Assert through real `dispatch` and the row:
  failed / `cleanup_unconfirmed`, stop `failed`, every observed field NULL,
  `identity_matches_pin` NULL, `rights` = pin, `cleanup_confirmed` 0 and
  quarantined. Then assert that a second dispatch is refused/quarantined with
  zero backend calls. It must never be failed / `execution_failed` without
  quarantine.
- The same script, stopped at "called, outcome indistinguishable", returns
  `Failed { Unknown }`, with the same expectations.
- The same script, stopped before the call, returns `Failed { NotInvoked }`:
  row F, no quarantine. Stopped at its final cancel recheck before the call,
  it returns `CancelledBeforeSpawn`: row E.
- A `Created` child with no observation, but with cleanup confirmed inside the
  required boundary, uses `Completed` with `observation: None`: row J,
  quarantine.
- The existing cancellation and gated-preflight tests stay green.

**Precedence.** These cases check the order of rows:

- Cleanup NULL, plus a mismatch, plus stop `failed`, gives
  `cleanup_unconfirmed`.
- A mismatch plus stop `failed` gives `identity_mismatch`.
- No observation plus stop `failed` gives `execution_failed`, with observed
  fields NULL.

**Pins.**

- `Pin::new(Macos, _, _, Admin)` gives `Err(AdminUnsupportedOnPlatform)`.
- An `Execution` holding it refuses `refused/preflight_failed`, with zero
  backend calls.
- Shape errors give `PreflightFailed`.
- `unavailable()` and the configured `Native` still refuse with
  `mechanism_unavailable`, and `capability()` is unchanged.

### Privacy

Run these for SID B (row I), the Unix UID B and GID B cases (row I), and SID B
under row H:

1. **Immediate response.** Walk the whole JSON value recursively.
   Withhold each differing identifier separately in `observed_identity` and
   `metadata`: SID B gives id NULL and gid NULL; Unix UID B / GID A gives id
   NULL and gid 61001; UID A / GID B gives id "61001" and gid NULL; both B
   gives both NULL. Matching fields must remain the actually observed values.
   `observed_identity.kind` and
   `observation_point` are set. No JSON string contains SID B or equals "62002".
   No JSON number equals 62002 or 63003.
2. **Row.** The id/gid values equal the per-field expectations above.
   `observed_kind` is set and `identity_matches_pin` is 0. No row in the table has `observed_id` equal
   to SID B or "62002", or `observed_gid` equal to 63003.
3. **`Activity::list`.** The same assertions as step 1 on the serialized page.
4. **Second dispatch.** The refusal carries only fixed tokens.
5. **Database bytes.** After closing the store, the database file contains
   neither SID B nor the ASCII text "62002". If the protected TRUNCATE
   journal remains, check its bytes too; do not require deletion of an expected
   persistent journal. Neither file contains `S-1-x` or the 185-byte value.
6. **`Debug`.** `format!("{:?}", observation)` for each B observation contains
   neither SID B, "62002" nor "63003".
7. **MCP end to end** (`application/tests.rs`). Serve `crate::mcp::Inventory`
   over the in-memory duplex, as `dedicated_start_mode_precedes_development_and_survives_refusal`
   does. Inject `backend_fixture(OnceReport)` with the SID B row-I report as
   `activity.dedicated`, using a fixture principal. Then call `run_command`,
   then `list_activity`, then `list_hosts`. Each tool's text content must pass
   the step 1 string checks for SID B. `run_command`'s `observed_identity.id`
   must be null, `list_activity`'s row must have `observed_id` null, and
   `list_hosts.last_observed_identity` must stay null. Make the test portable,
   with no `cfg(windows)`, unless an existing fixture forces it; if one does,
   say so in PROGRESS.

Row and response keys are unchanged.

**Rewrite these existing tests, without weakening them:**

- `report_boundary_case_3`: an injected `reason` is now unrepresentable. Make
  it the stop-NULL row K case.
- `report_boundary_case_7`: start with plain `start` (NULL mode) and a valid
  Windows evidence. Assert that `dedicated_account` is written, so the S3
  "forced mode" mutation is still killed.
- `synthetic_observation_mapping_and_terminal_table` becomes `assess` table
  tests.
- `all_postpreflight_reason_mappings_and_cleanup_precedence` becomes the
  precedence cases above.
- The cases that build `Failed { after_spawn: false | true }` become
  `Failed { NotInvoked }` (F) and `Failed { Created }` (G). Add `Unknown` (G)
  and `ProvenNone` (F) beside them.
- `output_fits_sdk_double_json_worst_cases`: build the worst-case metadata
  through `assess`, using a 15-subauthority SID equal to the pin, so it is not
  withheld, and `i32::MIN`.

## Mutations

Each mutation must be killed by a named assertion. Record the run ID per
mutation in PROGRESS.

1. Make observation optional for success (P0).
2. Treat cleanup NULL as true (P1).
3. Drop the token-class check and compare only the SID (P2, `Limited`,
   `DenyOnly`).
4. Accept any observation point (P3).
5. Accept any boundary (P4).
6. Skip the downgrade of cleanup to 0 under row H (P4 row assertion).
7. Persist inadmissible observations (P3 and malformed-SID rows).
8. Fill `observed_id` from the pin when absent (P0 row).
9. Set matches to always true (SID B case).
10. Remove `identity_mismatch` from the quarantine gate.
11. Swap the cleanup and identity precedence.
12. Treat `CancelledBeforeSpawn` as a spawned report.
13. Write `rights` as a constant `standard` (admin positive control).
14. Treat stop NULL as `exited`.
15. Accept a code with a non-exited stop.
16. Remove the pin refusal for macOS admin.
17. Return tools on failure.
18. Use the `elevated` Boolean in place of `elevation_type`.
19. Treat `DenyOnly` as `Absent`.
20. Map `Creation::Created` to row F (created-not-execed phase test).
21. Map `Creation::Unknown` to row F (indistinguishable phase test).
22. Write the observed SID or UID even when it differs from the pin (SID B and
    UID B privacy steps 1–3 and 5).
23. Withhold in the response but not the row (privacy steps 2, 3 and 7).
24. Write the pin in place of a withheld identifier (SID B: `observed_id` must
    be NULL, not pin A).
25. Withhold even a matching identifier (positive controls and P2).
26. Drop the GID rule (Unix GID B case).
27. Compute matches after withholding, so a withheld value counts as a match
    (SID B: matches must be 0).
28. Print `sid`, `uid` or `gid` in `Debug` (privacy step 6).

Also run the PR36 recheck mutations that remain applicable (S2, S3, M8 and
M9) and record their results.

## Output-release amendment (accepted)

Contract: ADR 0017
[Output release](../adr/0017-dedicated-report-validation.md#output-release-privacy-normative-output-release-amendment-accepted)
and Amendment 3. Counterexample and matrix:
[output-release disposition](../reviews/0017-output-release-disposition.md).
Implementation base: `683b2db90ed5d51d7ea79cfa38164658ae65789d`.
Proposal base: `2f3b4863`. Its source equals the completed validator at `c394ec5`. Rebase
onto a newer integration head if one lands first, and keep the change inside the
`Completed` arm.

### Scope

Change only:

- `crates/arxd/src/service/executor.rs`: the `Report::Completed` arm of
  `complete()`, plus one private helper for the withheld value, if wanted.
- `crates/arxd/src/service/executor/tests.rs`
- `crates/arxd/src/service/application/tests.rs`, test only
- Docs: PROGRESS, and the ADR status line after review.

Do not touch `assess`, `Metadata`, reasons, precedence, quarantine, creation
phases, start mode, drain, `JoinSet` or stopping priority (N3), the store,
schema, `mcp.rs`, the development executor, `output_value`'s budget or
mapping, or workflows. Add no key, token, column, approval flow, native lookup
or fallback. Do not adopt `diagnostic_unverified_identity_output_is_returned`
from the [probe patch](../reviews/0017-unverified-output-probe.patch). It
asserts the disclosure.

### Symbols

1. In the `Completed` arm, after `assess` returns, decide release with an
   exhaustive match on `metadata.identity_matches_pin` and no wildcard:
   `Some(true)` → `output_value(stdout, budget)` and
   `output_value(stderr, budget)`, unchanged; `Some(false) | None` → the
   withheld value for both.
2. Withheld value: `json!({"head":"","tail":"","dropped_bytes":null})`, built
   without reading the `CapturedOutput`. Drop both `CapturedOutput` values
   unread: no `output_value`, no length, no `text()`, no `Debug`, no log.
3. Update the `Executor` trait comment: output is released only for a
   pin-matched child; `CapturedOutput` must hold only bytes from the observed
   child's own pipes.

### Required tests

Use real `dispatch`, an owned temporary store and `OnceReport`, as in the
existing matrix.

**Payload.** Every case below sends nonempty `head` and `tail` in **both**
streams. Use the synthetic markers only:

- `stdout.head` = SID B text `S-1-5-21-700-800-900-4242`
- `stdout.tail` = the non-identifier marker `ARX0017-OUTPUT-MARKER`
- `stderr.head` = `ARX0017-OUTPUT-MARKER`
- `stderr.tail` = `62002`
- backend `dropped_bytes` = 5 on stdout and 7 on stderr

Use the default request (`output_bytes` 4096, so 2048 bytes per head and
tail). Every marker then fits, and the released counts stay 5 and 7.

**Assertions for every case.**

1. Immediate response: `outcome`, `reason`, `observed_identity` and nested
   `metadata` equal the existing ADR 0017 expectation for that row. The
   amendment must not change them.
2. Withheld case: each stream equals exactly
   `{"head":"","tail":"","dropped_bytes":null}`. `assert_private` passes on the
   whole value, and no JSON string contains `ARX0017-OUTPUT-MARKER`.
3. Released case: `stdout` equals `{"head":"S-1-5-21-700-800-900-4242",
   "tail":"ARX0017-OUTPUT-MARKER","dropped_bytes":5}`, and `stderr` equals
   `{"head":"ARX0017-OUTPUT-MARKER","tail":"62002","dropped_bytes":7}`.
   Apply `assert_private` to every key except `stdout` and `stderr`. This
   proves that no identifier or string filter acts on released output.
4. Durable row: every column equals the existing expectation. Quarantine
   matches the row.
5. `Activity::list` page and, after closing the store, the database and any
   remaining protected journal bytes contain neither `ARX0017-OUTPUT-MARKER`
   nor SID B, in either released or withheld cases. Output is never
   persisted.

**Withheld cases** (Windows standard pin A unless stated):

| Name | Evidence change | Row |
| --- | --- | --- |
| `out_sid_b` | SID B | I |
| `out_same_sid_full` | SID A, 12288, `Full`, elevated, `Enabled` (P2) | I, `observed_id` = SID A |
| `out_same_sid_limited_denyonly` | SID A, `Limited`, `DenyOnly` | I, `observed_id` = SID A |
| `out_admin_pin_standard_token` | admin pin; standard token | I |
| `out_unix_uid_b` | Linux pin; UID 62002, GID 61001 | I |
| `out_unix_gid_b` | Linux pin; UID 61001, GID 63003 | I, `observed_id` = "61001" |
| `out_no_observation` | observation and point `None` (P0) | J |
| `out_wrong_kind` | Unix observation on a Windows pin | J |
| `out_wrong_point` | `pre_exec` on Windows | J |
| `out_malformed` | SID `S-1-x` | J |
| `out_sid_b_cleanup_null` | SID B, cleanup `None` | H, matches 0 |
| `out_none_wrong_boundary` | no observation, `process_group` | H, matches NULL |

**Released cases** (full match):

| Name | Evidence change | Row |
| --- | --- | --- |
| `out_ok` | none | N |
| `out_exit_nonzero` | exited 2 | M |
| `out_timeout`, `out_cancelled`, `out_output_limit`, `out_shutdown` | that stop, no code | L |
| `out_sid_a_cleanup_null` | cleanup `None` | H, quarantine |
| `out_sid_a_wrong_boundary` | `process_group` (P4) | H, quarantine |
| `out_stop_failed` | stop `failed` | K, quarantine |
| `out_linux_ok`, `out_macos_exit_nonzero` | Linux N; macOS M | N, M |

**Identity versus cleanup-only.** Put `out_sid_b_cleanup_null` and
`out_sid_a_cleanup_null` next to each other in one named assertion. Both are
failed / `cleanup_unconfirmed` and quarantined. Only the first withholds.

**Existing matrix.** Change `completed()` to carry the same nonempty payload.
Then every one of the 50 `adr0017_dispatch_durable_matrix` cases asserts
release exactly when its expected `identity_matches_pin` is 1. Matching cases
must not run `assert_private` over the released streams, because they contain
SID B text by design.

**MCP envelope** (`application/tests.rs`). Extend
`dedicated_report_mcp_privacy_in_memory` to the payload above and to four
cases over the in-memory duplex: `out_sid_b` (I), `out_no_observation` (J),
`out_ok` (N) and `out_sid_a_cleanup_null` (H). For each case, parse the
`run_command` text content once. Withheld cases: the raw text contains neither
SID B nor `ARX0017-OUTPUT-MARKER`, and both streams equal the withheld value.
Released cases: both streams equal the released values above, and
`assert_private` passes on every other key. Then call `list_activity` and
`list_hosts` in every case. Their raw text contains neither SID B nor the
marker. Keep the test portable.

**Bounds.**

- Keep `output_fits_sdk_double_json_worst_cases` unchanged. It is the released,
  matching worst case and remains the upper bound.
- Add a check that the withheld value is byte-identical for an empty capture
  and for 8192 bytes of `0xFF` with `dropped_bytes = u64::MAX`. That proves
  no length or count leaks.

### Mutations

Each must be killed by a named compiled assertion. Record the run IDs in
PROGRESS, as before.

29. Release unconditionally, the current code (`out_sid_b`,
    `out_no_observation`).
30. Gate on `observed_id.is_some()` (`out_same_sid_full`,
    `out_same_sid_limited_denyonly`, `out_unix_gid_b`).
31. Gate on `observed_kind.is_some()` (`out_sid_b`).
32. Gate on `reason.is_none()` or on no quarantine (`out_sid_a_cleanup_null`,
    `out_sid_a_wrong_boundary`, `out_stop_failed`).
33. Gate on `outcome == succeeded` (`out_exit_nonzero` and the four L cases).
34. Gate on `reason != identity_mismatch` (`out_no_observation`,
    `out_sid_b_cleanup_null`).
35. Treat `None` as release (`out_no_observation`, `out_malformed`).
36. Withhold only one stream (both-stream assertions).
37. Represent withheld as `dropped_bytes: 0`, or as the real count
    (exact withheld-value and constant-value checks).
38. Scrub SID- or UID-shaped strings instead of gating (`out_ok` must return
    SID B text and `62002` verbatim, and `out_sid_b` must also withhold
    `ARX0017-OUTPUT-MARKER`).
39. Withhold every stream, always (`out_ok` and the existing positive
    controls).
40. Change reason, outcome or quarantine when output is withheld (row
    assertions in every withheld case).

Rerun the full required local checks below. Hosted checks and a fresh
independent Opus implementation review remain required.

## Checks and limits

Run these locally and report them truthfully:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo nextest run --workspace --locked --no-fail-fast`

Hosted Linux and macOS CI must also pass.

Do not do any of the following:

- create a native backend or production constructor;
- run SCM, LSA, account, privilege, VM or Lab Manager operations;
- try S4U again;
- add any fallback, approval flow, hash, column or vocabulary.

Native, installed and dedicated M2 remain **unverified on real hardware**.
This change makes nothing available.
