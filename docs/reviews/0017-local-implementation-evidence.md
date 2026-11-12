# ADR0017 local implementation evidence

Builder: OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-11.
Prepared base: `ad5acb368212c7e7410f362658bc726b1348845b` (verified clean).
The supplied checkout is bound to local branch `t3/m2-report-validator`;
the requested integration destination is `integrate/dedicated-report-validation`.
No branch switch, push, PR, GitHub write, merge, hosted dispatch or watch occurred.

## API and scope

Backend-authored store metadata is replaced by `Evidence` and typed
`Observation`; rights, reason, identity mode, request fields and match results
cannot be authored through that API. `Creation` distinguishes NotInvoked,
ProvenNone, Created and Unknown; cancellation before the primitive has its
own report. Immutable `Pin` comes from validated service configuration.
`assess` compares full observations before withholding each differing
identifier independently. One sanitized Metadata drives storage, immediate
response, nested metadata and history. Observation Debug withholds every ID.

Assessment lives in the existing `complete()` helper. Worker registration,
JoinSet transfer/drain and quarantine-before-stopping priority are preserved.
Native remains MechanismUnavailable; no native child/discovery mechanism,
owner code, schema/config/preflight mapping, development executor, MCP
production wrapper or workflow was changed. The store helper preserves the
existing 184-byte bound and syntax checks.

The portable Application test uses an in-memory MCP duplex and synthetic
OnceReport, with SID B withheld and SID A as a matching positive control.
No actual pipe/socket or production constructor is used by the new tests.

## Evidence boundaries

The five-case baseline already exists in
[the retained diagnostic](service-report-validation-probe.md), run
`7798caf5-2cb6-4c60-bb56-6655b8e7e7ec`. It is not adopted as a passing
regression. Local source mutations below exercise real dispatch and durable
rows; no compile failure is credited as a killed behavioral mutation.
Backend rights/reason/matches injection and malformed numeric Unix IDs are
type-level unrepresentability, separately from executed runtime mutations.

Initial development failures: an early executor build ran before newly
referenced test helpers were written (four missing symbols; no Nextest run).
The first Clippy run found collapsible_if and two test type_complexity
diagnostics; these were corrected. A mutation harness first used the Windows
default code page for temporary Rust writes: all 32 attempts failed before
Nextest, with no run ID and no kill credit. An intermediate UTF-8 conversion
also produced mojibake in existing superscript test literals; fmt and build
rejected it before Nextest. Original Unicode was restored and explicit UTF-8
is now used. These are harness/development errors, not red/green evidence or
three distinct failed validator fixes.

Fresh independent Opus implementation review must include the four Sol
consistency edits in `226d1f2`, the accepted brief `658f6f2`, this implementation,
and preservation of N3 `a26570d` / review `fa6f297`. Hosted Linux/macOS/Windows
checks and normal agent-review/ci-time-budget gates remain required before
publication or merge. Installed/native/dedicated M2 is **unverified on real
hardware**; M2 stays open and this slice makes nothing available.


## Executed mutations

All 28 required mutations have compiled runtime failure evidence, plus S2,
S3, M8 and M9. Additional 7b isolates malformed-ID admissibility.
Mutation 13 initially survived (3 passed); expected rights had incorrectly
come from assess itself. The matrix now constructs expected metadata from
the named fixture and contract, never from assess. Recheck 13 fails on
`windows_admin pinned rights`. Rechecks 8 and 12 use the final stronger test
assertions; 12 now routes pre-call cancellation through spawned assessment.
Mutation 8 is caught as an unacceptable StorageFailed dispatch rather than
a falsely successful row. The final named assertion requires durable completion.
S3's actual mutation forces final mode NULL; it does not claim that removing
a redundant assignment alone is behaviorally observable.

The exact old/new source edits, source SHA-256, commands, run IDs, compiled
flags, test counts and failing assertions are in
[the mutation records](0017-local-mutations.jsonl).
Each source mutation was restored in a Python `finally` block, byte for byte.
The first 32 code-page compile failures receive no kill credit and are
excluded from these compiled mutation records. No compilation failure was
manufactured as baseline evidence.
| Mutation | Compiled assertion / result | Nextest run ID | Executed counts |
| --- | --- | --- | --- |
| 1 | adr0017_dispatch_durable_matrix | `1dd7fe6f-e9de-4334-a8cd-a776b1c36892` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 2 | adr0017_dispatch_durable_matrix | `dc0bd5c8-9def-4b1b-8ea8-bdd093a20556` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 3 | adr0017_dispatch_durable_matrix | `29bf5d8f-fc39-48cf-b3ae-2cf4c8c1af86` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 4 | adr0017_dispatch_durable_matrix | `cc433636-41f9-4f22-a7a4-064b2d5863fa` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 5 | adr0017_dispatch_durable_matrix | `fb09a67f-6aeb-40f1-a9c1-5bfa20d50d8e` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 6 | adr0017_dispatch_durable_matrix | `271a4896-5055-4893-aff7-34624c1e55ac` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 7 | adr0017_dispatch_durable_matrix | `34e676fb-f0c4-46bc-b32e-40c603d83618` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 8 | adr0017_dispatch_durable_matrix | `7deafac0-5433-4fe0-98ab-311a976436ec` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 9 | adr0017_dispatch_durable_matrix | `adc4ba7a-0af4-4578-a445-44ae2fa08274` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 10 | adr0017_dispatch_durable_matrix | `871d9b84-57f9-4b27-8d3c-d253c01db389` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 11 | adr0017_dispatch_durable_matrix | `a9f67d27-d64c-42fe-91b3-8eba8ac5ebbe` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 12 | adr0017_creation_phases_and_pin_refusal | `a24a6ed5-c2ff-48df-8db7-2ff2222be9bf` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 13 | SURVIVED; test oracle corrected | `39f75a7d-2101-4a70-9535-c7aae3341a91` | 3 tests run: 3 passed, 224 skipped |
| 14 | adr0017_dispatch_durable_matrix | `6396c029-12bd-4bcc-b198-c7b78a35e542` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 15 | adr0017_dispatch_durable_matrix | `08c3dcef-bc68-4697-b6a4-51f39bfbe19f` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 16 | adr0017_debug_and_pins, adr0017_creation_phases_and_pin_refusal | `7d902a36-315f-4441-9f84-6f833ccb5738` | 3 tests run: 1 passed, 2 failed, 224 skipped |
| 17 | adr0017_dispatch_durable_matrix | `9f3480ab-6f0f-4273-919a-d06afe4e9b8f` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 18 | adr0017_dispatch_durable_matrix | `30bf857f-7eb4-4ab9-af21-bf94c41fdcd1` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 19 | adr0017_dispatch_durable_matrix | `d186c5d8-8946-4cbe-9363-6467d06a234d` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 20 | adr0017_creation_phases_and_pin_refusal | `249d1856-c99b-4607-bb30-55bb19afceae` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 21 | adr0017_creation_phases_and_pin_refusal | `15978e95-025d-4b5b-b76d-94e146174546` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 22 | adr0017_dispatch_durable_matrix | `8e518597-f81e-43d2-bd41-a066be7b22aa` | 3 tests run: 2 passed, 1 failed, 224 skipped |
| 23 | dedicated_report_mcp_privacy_in_memory | `9c693427-dd85-41f3-a108-b7be22bb48b8` | 13 tests run: 12 passed, 1 failed, 214 skipped |
| 24 | adr0017_dispatch_durable_matrix | `2325bc2b-5f57-4489-b8ce-c7f66d5fd076` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 25 | adr0017_dispatch_durable_matrix | `918a5086-8050-49d6-a9d5-fccf2b4f1db3` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 26 | adr0017_dispatch_durable_matrix | `e8e117ef-d7b9-4716-b16c-50d280177d1e` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 27 | adr0017_dispatch_durable_matrix | `fa108881-2a2a-4f6e-a0d8-39f28b054ef0` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 28 | adr0017_debug_and_pins | `598b647b-1b9d-4483-86ec-5a34bf9fe241` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| S2 | cancellation_after_gated_preflight_never_enters_backend | `e7724b69-e350-4bbc-9db0-ff7314bc48ef` | 1 test run: 0 passed, 1 failed, 227 skipped |
| S3 | report_boundary_case_7 | `b70a3c1b-2d1f-4b76-a449-53f1d8c680f3` | 1 test run: 0 passed, 1 failed, 227 skipped |
| M8 | report_boundary_case_0, report_boundary_case_3, report_boundary_case_1, report_boundary_case_6, report_boundary_case_2 | `4a548d17-2504-42e3-8e74-8446cd82a44d` | 8 tests run: 3 passed, 5 failed, 220 skipped |
| M9 | report_boundary_case_5, report_boundary_case_4 | `337a2d4d-7fa6-481a-a11d-762ef1e01312` | 8 tests run: 6 passed, 2 failed, 220 skipped |
| 8 (recheck) | adr0017_dispatch_durable_matrix | `f40ff675-840a-418e-a3ce-d8b9657c21c4` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 12 (recheck) | adr0017_creation_phases_and_pin_refusal | `72e61177-adb9-4ffe-9016-396c4f38b4a1` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 13 (recheck) | adr0017_dispatch_durable_matrix | `c88b8e6c-8079-44d8-b509-a9f44b07fb87` | 4 tests run: 3 passed, 1 failed, 224 skipped |
| 7b (recheck) | adr0017_dispatch_durable_matrix | `d3953847-1a71-4e19-a9e9-195a5367d454` | 4 tests run: 3 passed, 1 failed, 224 skipped |

## Final verification and integration

Implementation commit: `c63f6cb5a5682a822fc1da2ff614971dd2ea67b3`.

- `cargo fmt --all --check`: passed.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: passed.
- `cargo nextest run --workspace --locked --no-fail-fast`: **227 passed,
  2 skipped**, seven binaries, 11.305s, run
  `4de890f8-0024-4fc4-944c-c23d3d6949d9`. Single final full normal run;
  no retries. Existing normal parent tests ran as authorized.
- `git diff --check` and strict UTF-8/no-BOM/no replacement/mojibake checks passed.
- Source comparison confirms exact preservation of N3 drain, registration
  and stopping-priority segments, with the required typed cancellation
  replacement as the sole dispatch-body change.

The final matrix has 50 named cases; expected metadata comes directly from
the fixture and contract. It also checks both immediate metadata and every
durable metadata column. Five phase/cancel scripts and two backend refusal
cases supplement pin, Debug, MCP privacy and preserved lifecycle coverage.
One later broad test edit caused ten missing-symbol compiler diagnostics
before Nextest; it was corrected. A final Clippy tuple-type diagnostic in the
new refusal test was resolved with a type alias. Neither is mutation credit.

Apply the source commit and following evidence commit in order onto the
exact prepared integration base. On a newer base, reconcile the complete()
assessment call site and preserve N3; rerun required checks on the integrated
head. Obtain fresh independent Opus review before publication and all hosted
gates before merge. No hosted or hardware result is claimed.
