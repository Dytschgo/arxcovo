# Dedicated report-validation gap: bounded synthetic evidence

- Author: OpenAI / gpt-6.1-sol / medium, coordinator, 2026-11-11.
- Pinned source: reviewed PR36 head `5c3c01e3ee11d8b97ebe3dd249f8d9bc88300b7b`.
- Scope: the existing N2 advisory from the [independent recheck](https://github.com/Dytschgo/arxcovo/pull/36#issuecomment-6806673472), which must close before a native adapter lands. This is evidence collection, not an architecture decision, security review, fix or request to activate execution.

## Reproduction

Applied the adjacent temporary test patch to the pinned source. Its backend is `OnceReport`, with preflight returning Ok and configured rights Standard. Each case starts a dedicated `run_command` row, dispatches through the real `Execution::dispatch`, and reads the committed outcome and identity mode through an ordinary-user temporary SQLite store. There is no process spawn or identity switch. The Windows SID is the existing synthetic fixture constant.

All cases begin with a Windows Standard observation (Medium integrity, not elevated, matches=true, post_switch), exit 0, cleanup=true and Windows Job, then change exactly one fact family:

| Case | Changed report fact | Observed response and durable row |
| --- | --- | --- |
| 0 | All observed identity fields and matches flag absent | succeeded; dedicated_account; no reason; no quarantine |
| 1 | cleanup_confirmed absent | succeeded; dedicated_account; no reason; no quarantine |
| 2 | rights=Admin despite configured Standard | succeeded; dedicated_account; no reason; no quarantine |
| 3 | observation_point=child, which ADR0009 excludes from M2 acceptance | succeeded; dedicated_account; no reason; no quarantine |
| 4 | tree_boundary=process_group instead of Windows Job | succeeded; dedicated_account; no reason; no quarantine |

Actual command: `cargo nextest run -p arxd --locked diagnostic_n2_report_acceptance --no-fail-fast --status-level fail --final-status-level fail`. Run `7798caf5-2cb6-4c60-bb56-6655b8e7e7ec`: 1 diagnostic test passed, covering all five cases, 221 tests skipped by the filter. The diagnostic asserts the observed acceptance to establish reproduction; it must not be adopted as a regression that preserves this acceptance. Source bytes were restored after the run. No full suite, Clippy or native runtime claim is made for this docs-only report.

## Source explanation and contract work remaining

`terminal` accepts exited/0 without requiring identity, cleanup or containment evidence. Dispatch forces dedicated mode and sets program/argc from the request, but does not compare rights with configured rights. Metadata's store validation bounds an observed ID when present; it does not require the observation to exist. Thus these reports are syntactically accepted and durably recorded as success. No native producer exists today; production preflight remains MechanismUnavailable.

Before connecting a native adapter, the architecture/security reviewer must settle a report matrix tied to ADR0009, including:

1. Required observation, matching rights, observation point and containment for each supported platform; compare the observed identity with immutable configured identity rather than treating an arbitrary matches=true as independent proof.
2. Distinguish pre-observation spawn failure, observed identity mismatch, completed target and discovery reports. Never manufacture observed identity or cleanup facts that the backend did not observe.
3. Decide the fixed outcome/reason and quarantine consequence for missing or contradictory fields, preserving cleanup_unconfirmed precedence and all truthful observed facts. N1's identity-mismatch quarantine policy remains undecided here.
4. Apply the same checks to command and discovery, with tests for actual dispatch and persisted rows, all stop causes, matching positive controls and failed-after-spawn reports. Existing before-spawn cancellation/refusal paths legitimately have no child observation and must retain their meaning.
5. Keep schema 3, fixed tokens, output bounds and no credential/current-user fallback. Native authority, privileged acceptance and the separate N3 drain race are outside this probe.

The [ADR0009 execution outcome table](../adr/0009-service-lifecycle-and-dedicated-execution.md) specifies identity and cleanup facts, Windows token observation before resume, Unix pre-exec observation and per-platform containment. This evidence does not amend it. Obtain independent Opus architecture review before implementing the N2 matrix, then Sol implementation and independent review under normal gates. No new PR is opened during the queue-drain pause.

## Limits

Windows ordinary-user synthetic backend and owned-temp storage only. No production constructors, SCM/LSA, account/rights/privilege changes, service manager, VM, root/admin action or ignored fixture ran. There was no GitHub write, dispatch or retry. The E1 candidate and its active review are untouched. Installed/native/dedicated M2 paths remain **unverified on real hardware**; M2 is incomplete.
