# PR183 third source observation: Unix discovery golden mismatch

Exact source `d30c79af8ac363fdb4e39420b0e15425ab5738cd`, normal
[CI44345053750](https://github.com/Dytschgo/arxcovo/actions/runs/44345053750),
attempt1. Initial partial evidence below is preserved; terminal matrix follows.

Fresh independent Sol6.1/medium schema-family review gives SOURCE GO,
Standards/Spec PASS with zero material findings. The
[attestation](https://github.com/Dytschgo/arxcovo/pull/183#issuecomment-6984414642)
and agent-review success bind exactd30. Report retained at
`D:/Temp/arx-pr183-schema-family-review/review.md`. Source review does not
substitute for hosted checks.

Ubuntu job134945073127 failed after141s. Clippy passed. Nextest executed
593/678 cases:592PASS,1FAIL,22skipped,85unrun; test body28.206s.
The failing case is
`service::unix_bridge::tests::e7::w6_w7_mcp_cache_and_no_cache_golden`,
at `service/unix_bridge/tests/e7.rs:488`. Its real SDK cache assertions ran
before a final source-text golden comparison failed. Actual MCP source uses
`self.execution.as_ref().is_some_and(|e| e.capability()["available"] == true)`;
the golden text expects `self.execution.is_some()`. This establishes the
mismatch, not whether changing the golden is sufficient or justified.
The schema-family tests reached before this failure passed; the complete
inventory binary and later steps remain unrun on Ubuntu.

At the captured read, Windows job134945073798 was running and macOS
job134945073704 was queued without a runner. No outcome is inferred for either.
No cancellation, CI rerun/dispatch or source push follows from this observation.

Bounded Ubuntu log retained at
`D:/Temp/arx-pr183-d30-first-ci/134945073127.log`:209857bytes,
SHA256 `b73726d6f134cefd626529a9c53a72c2c4012c477d1d81493b44c1dbedae1cc0`.
Download cap8MiB, deadline45s, no truncation, exit0. Metadata, download manifest
and parsed distinct test names are retained in the same D: root.

Coordinator requested a fresh independent Sol6.1/high READ-ONLY disposition:
check original golden intent against current-user/dedicated/legacy capability
semantics and related source-pin assumptions before any further correction.
This does not grant another fix attempt. Two prior test-integration correction
followups and all failed first-attempt observations remain preserved. No
production/runtime/permission/schema/engine/timer/quarantine/capacity change.
PR183 held. EngineSol3/3 exhausted, historicalOpus0/2 unchanged; PR182 HOLD/NOFIX.
M2 OPEN / M3 UNCHECKED; native paths unverified on real hardware.

## Terminal matrix and independent disposition

On the subsequent failed-check notification, one status read found all three
jobs terminal FAILURE on this same golden assertion. No rerun/dispatch or
cancellation occurred. Workflow16:15:15–16:24:38UTC,563s wall.

| Job | Job seconds | Test seconds | PASS | FAIL | Skip | Unrun |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Ubuntu134945073127 | 141 | 28.206 | 592 | 1 | 22 | 85 |
| macOS134945073704 | 258 | 44.780 | 572 | 1 | 8 | 73 |
| Windows134945073798 | 335 | 101.130 | 625 | 1 | 36 | 66 |

macOS acquired a runner; its duration above is execution, excluding queue time.
This does not establish general runner stability. Node/browser/Electron and
later Rust binaries remain unrun. Windows's short failed job is not a successful
full-job time-budget acceptance result.

Additional bounded logs at the same D: root:

- macOS207301bytes, SHA256
  `1e05b6917ee51fe9c65f62c522917879cd16bd02ee8159d02af6d59054375d5c`.
- Windows219152bytes, SHA256
  `87b7a0873013cf9bda956b11b8083544087a802c112354534424954c79a6e9bf`.

Both downloads exit0, no truncation, same8MiB/45s bounds. Original metadata/log
manifests preserved; terminal run/jobs and distinct case lists retained separately.

Fresh Sol6.1/high read-only
[disposition](../reviews/windows-execution-choice-golden-disposition.md) found
two Standards issues: stale source fixture and missing direct SDK coverage of
unavailable foreground no-cache output. No confirmed production regression.
The old golden pin spans shared foreground logic beyond the original inert Unix
domain. ADR0029 requires truthful current-user/dedicated availability. Historical
dedicated/Unix output and accepted historical pin manifests remain intact.
The reviewer recommends a narrow fixture refresh plus behavioral verification;
its read-only task creates no new fix grant.

Coordinator applies the owner's existing routine current-user feature authority
only to these two test artifacts: the complete golden block's predicate and
development/account_choice_tests.rs. No production function is changed. New SDK
cases exercise fresh no-cache current/legacy/dedicated states, shutdown and
quarantine refusals without probe fallback, inventory-only output and released
cache retention. The real Windows dedicated-settings test also checks list_hosts.
All historical engine/native holds and counters remain unchanged. These test-only
changes are not a founder/engine fix, reset, new diagnostic or acceptance waiver.

## Local correction verification

Coordinator Sol6.1/high updated only the golden predicate/formatting and the
test-only account-choice module. The nine-case SDK matrix covers current-user,
legacy and dedicated identities with available/shutdown/quarantine states;
dedicated remains unavailable in every state. Fresh unavailable cases refuse
discovery with zero probe calls. Available current/legacy cases produce a real
fake-probe audited cache, then retain its tools/time/identity through shutdown
and quarantine. Separate inventory-only SDK coverage and the actual Windows
dedicated-settings path assert truthful no-cache output. No native child runs.

Initial focused run6PASS/1FAIL: the added list_hosts call correctly produced an
additional audited read, invalidating the existing exact2-row total. Next run
6PASS/1FAIL: its new read assertion incorrectly expected no execution mode.
Inspection of mcp.rs:127ff shows every foreground read uses start_execution
with the immutable selected mode. The final test requires exactly3rows: one
successful dedicated-mode list_hosts read with no raw identity/rights, and the
same two dedicated refused executions with original privacy assertions. Neither
failed log is deleted or counted as a pass; no production correction followed.

Final focused run7/7PASS,0.747s test body (18.50s compile). Includes the two new
SDK tests, real Windows dedicated-settings refusal test, E7 full golden, adjacent
W8 source/domain pins, installed-service discovery cache test and legacy producer
roundtrip.721cases were outside this focused selection. Windows all-target
Clippy with locked/offline and -D warnings PASS (5.69s tool report), fmt check
PASS and git diff-check PASS. Logs at `D:/Temp/arx-pr183-golden-correction`.
No former-predicate mutation, engine/native probe, Linux/macOS compile, complete
suite, normal binary rebuild or production observation was run. Behavioral
regressions are now guarded by actual SDK assertions; no mutant-kill claim.

The golden still pins the complete block. Historical accepted fixtures/manifests
remain unchanged, and production mcp.rs is unchanged. The held transport source
is absent from this PR. Fresh exact-head independent review and all normal
required hosted checks remain mandatory. Stop for disposition on new failures;
no automatic fix/rerun loop. M2 OPEN / M3 UNCHECKED.
