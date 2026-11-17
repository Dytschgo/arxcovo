# ADR 0024: Windows three-node test quarantine

Status: accepted under delegated steering, 2026-11-16; hosted gates pending.

Three suite-level repair attempts left hosted Windows cluster tests intermittent. The independently reviewed disposition recommends quarantining exactly T1–T4, U6 and three R6 tests on Windows. Their bodies still compile and pass Clippy; each skip names the disposition. Ubuntu/macOS continue executing all eight. Other consensus, enrollment and storage tests remain required everywhere. No assertion, deadline, retry, scheduling or production code changes.

Delegated babysitter clarification supersedes the pending owner question: a reviewed quarantine restoring green main may merge after all five PR gates pass even while its base main is red. This applies to this repair, not feature merges. The next exact-main CI must be green before parallel M3 resumes. No manual rerun or fourth Sol fix is authorized.

Windows three-node coverage is unverified on hosted Windows and **unverified on real hardware**. Earlier green Windows runs do not prove stability. M3 stays unchecked; M2 remains open, A7 FINAL NO-GO and A12 consumed/no-retry unchanged. Opus owns a separate diagnosis/re-enable follow-up, requiring independent review and fresh hosted evidence before removing all eight gates. Revisit before M3 acceptance or Windows cluster use in M4.
## R5 addendum: local implementation, 2026-11-16

The accepted [R5 contract](../briefs/m3-r5-quarantine-restoration.md) adds a
separate Windows-only ignore for `r5_shutdown_read_owners`, with reason
`Windows R5 post-release quarantine: docs/adr/0024-windows-three-node-test-quarantine.md`.
This is a quarantine exception, not a repair or cause attribution. The original eight
attributes and bodies remain unchanged; R5's body and 2 s post-release wait remain unchanged.
Sol shared fix attempts remain 3/3 exhausted; Opus remains 0/2.

The guard separates eight sampled names in `EXPECTED` from R5 in
`WINDOWS_UNSAMPLED`. The fixed sampler filter excludes exactly R5 while retaining
all eight IDs, ordering, grammar, profile and budgets. **Coverage loss: Windows R5
does not run in either the required gate or the diagnostic sample.** Linux/macOS
R5 remains required. A green eight-test sample cannot provide R5 re-enable credit.
The contract's separately reviewed observation path and ten natural hosted Windows
passes with maximum clear time <=1000 ms are required before re-enabling R5.

Initial builder verification stopped on three no-op mutation fixtures. Root inspected
and verified the corrected source: guard2/2, sampler60/60, fmt and workspace Clippy
passed. Actual nextest inventory has exactly8 sampled matches and R5 excluded;
ordinary focused R5 ran0tests/571skipped (expected no-tests exit4). The historical
stop and logging-console error remain in [evidence](../evidence/m3-r5-quarantine-restoration.md).
Independent Opus source review remains pending; no hosted credit or merge authority.

Known main `c612152` / CI43880904887 is RED: 1475 s wall / 738 s charged,
uncredited because Windows failed. ADR0025 acceptance alone cannot clear the active
budget gate. All five gates and a later explicit owner budget decision remain required;
this addendum does not extend the prior restore-green exception to the budget.
M2 remains open, M3 unchecked, Windows three-node **unverified on real hardware**.

## Whole-fixture addendum: owner authorization, 2026-11-17

The [full-fixture contract](../briefs/m3-windows-fixture-quarantine.md) supersedes
R5-only scope and its independent re-enable criteria above. Quarantine all 13 async
three_node fixture cases and all 11 issuance fixture cases on Windows; Ubuntu/macOS
must execute all 24. The pure read-attribution unit remains required. The three D1
controls retain their pre-existing global ignores. Original eight attributes and
EXPECTED/REASON declarations remain byte-identical. R1–R5 and issuance share the
whole-fixture reason; R5's earlier reason is normalized. Bodies remain unchanged.

Coverage loss: all 24 are excluded from ordinary Windows execution. Only the
original eight may execute through the bounded non-gating diagnostic sample;
R1–R5 and issuance have no Windows execution coverage. Five fixed FILTER exclusions
keep the sampler at eight; issuance is outside its namespace. A green sample is no
Windows acceptance or re-enable evidence. Windows fixture coverage is unverified
on hosted Windows and **unverified on real hardware**. Re-enable as one reviewed
fixture suite after a separately bounded timing recovery plan with meaningful
slow-storage evidence and hosted acceptance; no further per-test expansion.

The prior PR164 I4 red is retained. Opus's injected-latency follow-up reproduced
exact I4 once, counterfactual passed, hooks removed; hosted cause and reproduction
rate remain unproven. Sol attempts 3/3, Opus 0/2 unchanged. M2 open, M3 unchecked.
Current main c612152 remains RED (1475 s wall / 738 s charged, uncredited).
Independent Opus review is required before push. After four hosted gates pass,
root must ask the owner for a one-use budget-only exception for the final exact
head. ADR0025 is accepted but inactive; bootstrap remains unauthorized. No
publishing or hosted action was performed by this implementation task.

## Consumed owner-authorized restoration exception - 2026-11-17

Owner explicitly authorized root to remove only ci-time-budget for exact PR164
head60f30c8673e9c9653d1a177d79379a1975f3107f, merge, and immediately restore
protection. Merge39b3dd7 completed; full protection snapshot matches readback
exactly afterward. The exception is consumed and grants no further merge. See
[checkpoint](../evidence/ci-windows-fixture-restoration-checkpoint.md). First-main
CI44051366327 is pending; no feature merge until green. ADR0025 stays inactive.
