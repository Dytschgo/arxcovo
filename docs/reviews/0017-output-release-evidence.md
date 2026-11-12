# ADR0017 output-release local implementation evidence

OpenAI / gpt-6.1-sol / high, implementation sub-agent, 2026-11-11.
Verified clean base `683b2db90ed5d51d7ea79cfa38164658ae65789d`.
Initial isolated branch `t3/5e18b801`; current branch `t3/gate-output-release`.
Runtime/tests commit `68a3943d31a09feb4b50165905db9f2f2b111f10`; parent integration destination
`integrate/dedicated-report-validation`. No subagents or external writes.
The [Sol PASS](0017-output-release-sol-review.md) accepts Opus proposal `0132c3e`.
The existing validator evidence at `c394ec5` remains historical and unchanged.

## Implementation and API

Only the Completed arm in complete() changes production behavior: exhaustive
match on metadata.identity_matches_pin, Some(true) calls unchanged output_value
for both streams, Some(false) or None constructs two fixed empty/null values
without inspecting either capture. Trait documentation states the native adapter's
own-child-pipes provenance obligation and the full-match release obligation.
No new API, helper, output budget, key, row, reason or quarantine policy.
Assessment, Pin, creation/start, N3 registration/drain/priority, store, mcp.rs,
development, native backend and workflows are unchanged. `git diff` shows exactly
the trait comment and Completed output gate as the two production hunks.
A full-source comparison against the exact base passes after removing only
those intended changes and normalizing newlines. The first comparison used
a locale-dependent read and failed on superscript encoding; rerun with explicit
UTF-8 passes. No source correction was needed.

The existing 50-case durable matrix uses nonempty synthetic captures in every
Completed case (discovery reports have no captures). Added 23 explicit named
cases, exactly 12 withheld/11 released, with expected metadata derived from the
contract fixtures rather than assess. All 73 cases check row columns, nested
metadata, observed identity, reason/outcome, quarantine and Activity::list.
After store closure, DB and remaining journal bytes exclude SID B and the
non-identifier output marker. The named cleanup pair shares failed /
cleanup_unconfirmed / quarantine but differs on output release. Matching output
returns SID B and UID-like text verbatim. The empty versus 8192FF/u64MAX case
asserts exact byte-identical withheld JSON for both streams. SDK double-JSON
worst-case test remains unchanged (verified against base after newline normalization).

MCP uses the real rmcp in-memory duplex for four named cases and all three tools:
run_command, list_activity, list_hosts. Exactly one text envelope is required and
parsed once per call. Raw text privacy and exact output values are checked;
matching cases apply metadata privacy outside streams. Final fixture refinements
add explicit envelope/raw-text/reason assertions and collapse a nested test if.

## Before and after

Command: `cargo nextest run -p arxd --locked -E 'test(adr0017_dispatch_durable_matrix) | test(dedicated_report_mcp_privacy_in_memory) | test(adr0017_withheld_capture_shape_constant)' --no-fail-fast --status-level fail --final-status-level fail`.

Before gate, run `89f20c07-660d-4d10-ba98-7480f4ded8f1`: 0 passed, 3 failed,
226 skipped. Compiled assertions fail on P0/out_sid_b disclosure and false zero
count for empty withheld capture. Initial test-construction compile failure
(Failure lacks Debug for unwrap) was fixed in test code, with no kill credit.
After gate, run `3c4b415f-aa98-4e58-8892-35ae2e5d59b5`: 3 passed, 226 skipped.
The named follow-up cases were then ordered first for mutation diagnostics.

## Actual mutations

[Exact source replacements, hashes, commands, run IDs and full assertion logs](0017-output-release-mutations.jsonl).
All 20 variants of 29–40 ran through compiled tests and were killed. No setup or
compiler failure is credited. Both one-stream variants, both count variants,
both reason/no-quarantine gates and all three assessment-change variants ran.
38 first overwrote the synthetic identifier fields, then also ran literal
SID/UID string-replace variants both with and instead of gating. All were killed.
Each mutation ran with try/finally restoration and an immediate byte comparison.
The final restored production SHA256 was
`9e56fd38cc15b4dae8324e5fcd9db6d0f7a9402666053ddede98ed22c0d0f95d`.
No probe diagnostic was adopted or run.

| Mutation | Named assertion failure | Nextest run ID | Result |
| --- | --- | --- | --- |
| 29 | empty versus 8192FF/u64MAX stdout withheld constant | `c98a8402-c0bb-45c0-a7ae-a70e4dc2039f` | compiled kill |
| 30 | out_same_sid_full stdout withheld constant | `e03c7db2-5695-4d3e-baa4-ad8e5443a936` | compiled kill |
| 31 | out_sid_b stdout withheld constant | `44b289c7-4920-4fba-897c-f41a40de0377` | compiled kill |
| 32-reason | out_sid_a_cleanup_null stdout released verbatim | `df6c611d-9eca-45ab-b2e2-2bd4fba7e609` | compiled kill |
| 32-quarantine | out_sid_a_cleanup_null stdout released verbatim | `250fcff5-d6fc-4fd7-990a-1ffcd4b3e154` | compiled kill |
| 33 | out_sid_a_cleanup_null stdout released verbatim | `2c4d1c5b-a996-47a0-8ca1-9bbd8b6a9e53` | compiled kill |
| 34 | empty versus 8192FF/u64MAX stdout withheld constant | `56513660-3d9f-481c-b0a1-af64176a85b8` | compiled kill |
| 35 | empty versus 8192FF/u64MAX stdout withheld constant | `b21cf6ac-255f-44cd-9758-e467215253be` | compiled kill |
| 36-stdout | empty versus 8192FF/u64MAX stdout withheld constant | `7ce86e84-2674-4c2b-b1bf-c6ed78b0ff55` | compiled kill |
| 36-stderr | empty versus 8192FF/u64MAX stderr withheld constant | `bbe8ddcb-190d-414c-aee4-0c47c4e613d3` | compiled kill |
| 37-zero | out_sid_b stdout withheld constant | `b76a8cbb-272c-4bb5-baf8-11a49277d65e` | compiled kill |
| 37-real | out_sid_b stdout withheld constant | `6757da18-99cb-48cf-a041-ac5000f4a62c` | compiled kill |
| 38-instead-of-gate | empty versus 8192FF/u64MAX stdout withheld constant | `8629a210-992d-47a1-90a6-abffaa79b77b` | compiled kill |
| 38-matched-scrub | out_ok stdout released verbatim | `4a4b9b63-6ef5-440c-ae90-877fc8be7ad8` | compiled kill |
| 39 | out_ok stdout released verbatim | `13a919bf-dce5-4598-ba05-e42fe5f0372b` | compiled kill |
| 40-reason | SID_B | `752e237e-ff49-47d2-95f5-5faeb443b482` | compiled kill |
| 40-outcome | SID_B | `c474951f-0e88-469c-8e19-9f03f2819891` | compiled kill |
| 40-quarantine | SID_B | `f4c67109-d145-4303-94bd-c23d8ad4170d` | compiled kill |
| 38-instead-of-gate-string-replace | out_sid_b stdout withheld constant | `a5a1e7ff-ca50-4967-b037-e8b627c7fc19` | compiled kill |
| 38-matched-scrub-string-replace | out_ok stdout released verbatim | `5dd7007c-991d-4c0a-9ec5-bce430aad2c7` | compiled kill |

## Final checks and limits

`cargo fmt --all --check` passed. `cargo clippy --workspace --all-targets --locked -- -D warnings`
passed, 30.13 seconds, no Clippy fix/retry needed.
`cargo nextest run --workspace --locked --no-fail-fast --status-level fail --final-status-level fail`
ran exactly once after all restorations and final fixture refinements:
`b713539f-afae-4b96-8a26-4b2af388906c`, **228 passed, 2 skipped**, seven binaries,
10.644 seconds test wall time (9.55 seconds build). No ExecutionFailed recurrence;
its historical cause remains unresolved. No retry-to-green. No ignored subprocess fixture
was directly invoked; only existing normal parent tests exercise them.

No production constructor, actual process backend/pipe/connect/system-path,
SCM/LSA/privilege/account/service/admin/owner/VM/Lab Manager/S4U operation.
Only synthetic backends, owned temporary stores and in-memory MCP for new tests.
No push, PR, GitHub, watch, merge, hosted dispatch or babysitter changes.
Public queue pause remains. Full independent Opus review of validator `c394ec5`
plus this delta and hosted required gates remain pending. Native execution stays
unavailable; installed/native/dedicated M2 is **unverified on real hardware** and
incomplete. No new architectural deviation or product approval flow.

Final documentation QA initially caught Windows-code-page punctuation in new prose;
converted those invalid bytes to UTF-8, then reran UTF-8/LF and local-link/anchor checks.
