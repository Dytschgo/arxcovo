# ADR0017 report validator and output release: independent Opus implementation review

Reviewer: Anthropic / claude-opus-5-5 / high, review sub-agent, single agent, no
subagents, 2026-11-11. Local only: no push, PR, GitHub status/comment/watch,
merge or dispatch. The four-PR queue pause is preserved.

Verdict: **CHANGES REQUIRED.** There are two blockers. Both are small: B1 is
test-only, and B2 is a one-field fix plus a test. The assessment, privacy and
output-release logic is correct as written, and every runtime check that ran
passed.

## Target and independence

- Prepared head `07ae3c89e1d6f845a460bc13c5d112508ff7060c` (verified clean).
  Implementation base `ad5acb368212c7e7410f362658bc726b1348845b`. Range
  `ad5acb3..07ae3c8`: validator `c63f6cb`, evidence `c394ec5`, probe `2f3b486`,
  disposition `56f8c9c`, Opus amendment `0626270`/`0132c3e`, Sol contract
  acceptance `683b2db`, output runtime `68a3943`, evidence `07ae3c8`.
- This is a full first review of all N2 code, not only the output delta:
  `executor.rs`, `executor/tests.rs`, the `store.rs` syntax helper and the
  `application/tests.rs` MCP test.
- **Cross-provider (independent):** Sol's implementation (`c63f6cb`, `68a3943`),
  its evidence, and Sol's edits to Opus documents: `226d1f2` (four consistency
  edits), `658f6f2`, and the status, anchor and pointer edits in `07ae3c8`
  to ADR 0017, the brief, the disposition and ADR 0009.
- **Not independent:** the ADR 0017 architecture, the output amendment and the
  brief's required-test list are Opus work. B1's missing cases were also
  missing from that Opus-authored list, so part of that gap is my provider's.

## Read

AGENTS, KICKOFF, CONTRIBUTING; ADR 0009 (pointer, lifecycle, exit codes,
ownership); ADR 0017 in full; the brief in full; the Sol contract review, recheck
and correction disposition; the output-release disposition and Sol review; the
probe and patch; both evidence files and both mutation JSONL sets; the PROGRESS
diff. The full source of `executor.rs`, `executor/tests.rs` and the
`application/tests.rs` diff, `store.rs` helper diff, and `config.rs`
validation and `application.rs` composition for the pin source.

## Commands run (actual)

| Check | Result |
| --- | --- |
| `cargo fmt --all --check` | passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | passed (35.3 s) |
| `cargo nextest run --workspace --locked --no-fail-fast --status-level fail --final-status-level fail` (single run, no retry) | **228 passed, 2 skipped**, run `d0ffb118-fc4b-4933-9619-521e94d0fe6a`, 10.441 s. Matches the builder's 228/2. No ExecutionFailed recurrence. |
| N3 preservation | `git diff a26570d ad5acb3 -- crates` is empty. The `drain` through `complete` segment at `07ae3c8` differs from `a26570d` only by replacing the synthetic cancelled `Completed` with `Report::CancelledBeforeSpawn`. Registration, the stopping branch, `JoinSet` transfer and quarantine-before-stopping priority are byte-identical. |
| `diagnostic_*` adoption | `grep -rn diagnostic_ crates/` finds nothing: neither probe diagnostic is in the tree. |
| SDK worst case | `output_fits_sdk_double_json_worst_cases` is byte-identical between `c63f6cb` and `07ae3c8`. The validator rewrote it through `assess`, with a 15-subauthority pin SID and `i32::MIN`, as the brief requires. |

Source mutations: 15 of mine, plus one probe. Each was an exact
single-occurrence replacement, followed by the focused filter
`test(service::executor::tests::) | test(dedicated_report_mcp_privacy_in_memory)`
(31 tests). After each one the original bytes were restored and the SHA-256
compared. `git status` was clean afterwards. Every run compiled, so no build
failure gets kill credit.

| # | Mutation | Run ID | Result |
| --- | --- | --- | --- |
| MA1 | Drop the Windows SID syntax guard in `assess` | `e47373f5-da62-4e96-83ae-dfb8998709db` | killed: matrix `malformed` |
| MA2 | Unix match ignores GID | `ae761dd1-a287-4f12-93e2-ff60865eeec1` | killed: matrix `GID_B` |
| MA3 | Unix accepts the `post_switch` point | `76fb1c2f-2bb3-424b-8bb1-eafe81d59fb6` | **survived** (31/31) |
| MA4 | Linux accepts the `mac_uid_sweep` boundary | `42592a8b-be23-44e5-9263-0d7e6272738c` | **survived** |
| MA5 | Standard pin drops `integrity_rid == 8192` | `43f175b6-a21d-4986-a3e9-1b832d049aba` | **survived** |
| MA6 | Admin pin drops `integrity_rid == 12288` | `9620767e-2e62-4b97-9ac6-0185502522c1` | **survived** |
| MA7 | Admin pin drops the `TokenIsElevated` check | `f16d1290-fe44-422a-97b9-70f653c4d3f9` | **survived** |
| MA8 | Store ID bound 184 → 185 | `f090e24a-8830-4f9c-8f78-4c0a359229ed` | killed: matrix `overlong` |
| MA9 | Check the pin before `backend.preflight()` | `87823acd-36cf-4d46-9a64-56b7d29f40a2` | **survived** |
| MA10 | Return discovery `tools` whenever not quarantined | `27529649-5136-4d8c-b377-eea7c46cbe29` | **survived** |
| MA11 | Also require confirmed cleanup to release (over-withhold, reverse-positive) | `35ab7314-e8c7-4e52-9679-9f3391ff9561` | killed: `out_sid_a_cleanup_null` (matrix and MCP) |
| MA12 | Row G `cleanup_confirmed` NULL instead of 0 | `fe818d1a-533f-4303-ba9a-6763557fd4ee` | killed: creation-phase row |
| MA13 | Windows pin accepts a GID | `afd639d2-e3e2-44cf-8e37-65f18c080f6b` | killed: `adr0017_debug_and_pins` |
| MA14 | Release on `None` (missing identity) | `8a75ef3b-9d42-41f4-9049-b2cd05faea26` | killed: matrix, MCP raw text, constant shape |
| MA15 | Withheld `stderr` reports `dropped_bytes: 0` | `f397e51b-253f-418b-a7b4-9c5a722ef418` | killed: matrix, MCP, constant shape |
| Probe | Temporary test: Linux admin config with no build GID → `Execution::configured(..).capability()` | `b3bdde22-6ae0-4470-a844-7e2b65873d25` | Shows B2: `{"rights":null,"service_state_protected":true}`. Probe file restored, hash verified. |

The probe called only `ServiceConfig::parse_inventory` with the test
`FakeResolver`, `Layout::for_test`, and `capability()`, which reads cached
values. It did not call the native preflight closure, `NativeResolver`, the state
guard or any system path.

## Standards

Documented standards (AGENTS.md and CONTRIBUTING):

- **Met.** Conventional, focused commits. Every behaviour has a test.
  Evidence keeps the mutation 13 survivor and the compile and code-page
  failures without kill credit. PROGRESS has provider, effort and outcome rows.
  All IDs are synthetic, with no secrets or owner data. No schema, MCP, workflow
  or approval-flow change. Native stays `MechanismUnavailable` with no selector.
  The **unverified on real hardware** label is kept.
- **S1 (documented, "Every behavior needs a meaningful test"):** the five
  surviving guard mutations MA3–MA7. They are listed under B1 on the Spec axis,
  because that is where the requirement comes from.

Smell baseline (judgement calls only):

- **Possible Primitive Obsession / fragile oracle** in
  `adr0017_dispatch_durable_matrix`: expected `admissible`, `observed_id` and
  `matches` values come from case-name string lists (`["P0","P3",…]`,
  `["SID_B","mismatch_failed",…]`). The oracle is independent of `assess`, which
  is what mattered after mutation 13. But renaming a case or adding one without
  updating the lists silently changes its expected values. Consider per-case
  expected fields.
- **Possible Duplicated Code:** `request.map_or(4096, |r| r.output_bytes)` is
  computed twice in the `Completed` arm, and the `expect("preflight pin")`
  appears in two arms. The panic is unreachable because `preflight()` checks the
  pin before any backend report. Advisory only.
- Repeated platform `match`es in `assess` (point, observation, boundary) are
  endorsed by the brief's no-wildcard rule, so they are not flagged.

## Spec

### Blockers

**B1. Several normative identity and containment cells have no test. Five
mutations survive, and four of those guards gate output release.**

ADR 0017's admissibility table requires `integrity_rid = 8192` with
`elevated = false` for a standard pin, and `integrity_rid = 12288` with
`elevated = true` for an admin pin. It names "a `post_switch` point on Unix" as
inadmissible. It also requires `linux_cgroup` on Linux.

The code implements all four correctly, but MA3, MA5, MA6, MA7 and MA4 all
survive the full focused suite. Since the amendment, `identity_matches_pin`
also decides `release(stdout, stderr)`. So dropping the integrity, elevation or
point check would release output from a child of an unvalidated token class,
and would let it succeed. Nothing in the suite would notice. MA4 would let a
UID sweep confirm Linux cleanup.

Required, test-only (add each to the durable matrix with the release
assertion):

- Standard pin, SID A, `Default`, `Absent`, not elevated, with integrity 4096 and
  with 12288. Expect I and withheld.
- Admin pin, SID A, `Full`, `Enabled`, elevated, integrity 8192. Expect I and
  withheld.
- Admin pin, `Full`, `Enabled`, integrity 12288, **not** elevated. Expect I.
- Linux pin with `post_switch`. Expect J and withheld. Add macOS too if cheap.
- Linux pin with `mac_uid_sweep` and macOS pin with `linux_cgroup`. Expect H.

Then rerun MA3–MA7.

**B2. `capability()` is no longer byte-identical for a reachable production
configuration, and the change produces a false protection claim.**

The brief says (Symbols 5): "`capability()` derives `rights` from
`pin.as_ref().ok()`. Its output must stay byte-identical for both production
constructors."

Production startup loads config through `native::read_config` →
`ServiceConfig::parse_inventory`. That path accepts a Unix config whose
`build_account.gid` is missing (`validate_inventory` rejects a GID only on
Windows). For such a config, `Pin::from_config` returns
`Err(PreflightFailed)`. `capability()` then emits `rights: null` and
`service_state_protected: true`, even when the configured rights are `admin`.
Before N2 it emitted `rights: "admin"` and `service_state_protected: false`.
Probe run `b3bdde22` shows the regression.

Reachability is narrow: it needs a hand-edited, owner-protected config, which
`configure` would reject. Execution also stays refused. But `list_hosts` would
show agents a protection claim the configured rights contradict.

Fix: keep the configured rights for the capability independently of pin
validity, as before. Add a pure test of `Execution::configured(..).capability()`
for that config (the preflight closure is never called), plus a test that a
failing backend preflight wins over a pin error. That second test also kills
MA9, which ADR 0017 requires: "checked after `backend.preflight()` succeeds …
so the production `Native` refusal is unchanged."

### Verified correct (Spec)

- **Typed API.** Backends cannot author `reason`, `rights`, `identity_mode`,
  matches or rows. `Observation` has no `matches` field. `Debug` withholds `sid`,
  `uid` and `gid`. `Evidence`'s derived `Debug` delegates to it, and `Pin` has
  no `Debug`.
- **Pin.** It is immutable, with private fields and no setters. macOS admin
  returns `AdminUnsupportedOnPlatform`, and shape errors return
  `PreflightFailed`. It is checked after the backend preflight and before the
  busy check. The production pin comes only from `configured`.
- **`assess`.** Every match is exhaustive, with no wildcard. The point is
  checked before the observation. The SID syntax guard falls through to an
  inadmissible result (MA1 is killed). Token class is checked independently of
  type, elevation and group. The match is computed from the unredacted values
  before withholding. `observed_id` and `observed_gid` are withheld per field,
  and the pin is never copied in. Rights come from the pin.
- **Precedence.** H comes first and forces cleanup to 0, then I, then J/K, then
  L/M/N. Quarantine is exactly `reason.is_some()`. Success requires `exited`
  with 0.
- **Creation and cancellation.** The `Creation` match is exhaustive. F is not
  quarantined. G is quarantined, with pin rights, stop `failed` and cleanup 0.
  `CancelledBeforeSpawn` and the dispatcher's own cancel both build rows C/E
  directly, never through `assess`. N3 is unchanged.
- **Output gate.** It is an exhaustive match on `Option<bool>`. `Some(true)`
  uses `output_value` unchanged. `Some(false)` and `None` build the constant
  `{"head":"","tail":"","dropped_bytes":null}` for both streams, and both
  captures are dropped unread. There is no string filter: mutation 38 is
  killed, and `out_ok` returns SID-B text and `62002` verbatim. Partial or error
  output with a full match is released, including cleanup-only failures
  (`out_sid_a_cleanup_null`, P4 and `failed`).
- **Oracles and payloads.** All 50 original and 23 named cases build their
  expected metadata from fixtures and the contract, never from `assess`. Every
  `Completed` case carries the nonempty payload in both streams (discovery has
  no streams). Each case checks every durable column, nested metadata,
  `observed_identity`, `Activity::list`, quarantine and the second refusal. It
  also scans the closed database and journal bytes for SID B, the marker,
  `62002`, `63003` and the malformed and overlong values. The cleanup pair and
  the counts (12, 11) are asserted. The four MCP cases parse one text envelope
  per tool and check raw text for every tool. Empty versus 8192×`0xFF` with
  `u64::MAX` produces byte-identical withheld JSON.
- **Sol consistency edits to Opus documents.** All are correct and faithful:
  the failed-run phase wording; Job-assignment evidence (it fixes an
  over-strong original sentence); per-field UID/GID withholding test
  expectations (they now match ADR rule 2); persistent TRUNCATE journal
  inspection; status lines; anchors (`#output-release-privacy-normative-output-release-amendment-accepted`
  and `#output-release-amendment-accepted` resolve); and the ADR 0009 pointer
  naming Amendment 3.

### Advisories

- **A1.** MA10 survives. ADR 0017 says "its `tools` list is returned only for
  row N". Add a discovery case with valid tools in row M (exited non-zero) and
  row L, and assert no `tools`.
- **A2.** The row C (dispatcher-side cancel) test asserts only outcome and stop.
  NULL rights and observed fields, and no quarantine, are asserted only on the
  backend's `CancelledBeforeSpawn` path. They share the mapping, so this is low
  risk.
- **A3.** The macOS-admin pin refusal test proves "zero backend calls" with
  `PhaseScript { NotInvoked }`, which records no events whether or not it runs.
  The `Err(PreflightFailed)` result proves the refusal indirectly. A counting
  backend would make the check direct.
- **A4.** ADR 0017's last paragraph under "Relation to ADR 0009" says the
  amendments "take effect only when this ADR is accepted". The status now reads
  "accepted for local implementation". Tighten the wording when merge gates
  pass.
- **A5.** Integration: Target B (`7781239`) also appends a test at the end of
  `application/tests.rs` and edits PROGRESS, so expect a trivial textual
  conflict. Keep both tests.

## Limits

No acceptance is claimed. Hosted Windows, Linux and macOS checks, agent-review
and ci-time-budget gates are still required. The native adapter's
own-child-pipe provenance and creation-outcome classification remain adapter
obligations that this gate cannot prove. Installed, native and dedicated M2 is
**unverified on real hardware** and incomplete. No production guard, layout,
session, install, native backend, process, pipe, system path,
SCM/LSA/privilege/account/service/owner, VM, Lab Manager or S4U operation ran.
No ignored fixture was invoked directly.

Standards: 1 documented finding (S1, the surviving guard mutations, raised as
B1) and 2 smells; worst is S1. Spec: 2 blockers and 5 advisories; worst is B1.
