# Independent review: Windows execution choice, round 3

Reviewer: GPT-6.1 Sol / high, independent of the builder, single read-only review sub-agent. Date: 2026-11-18.

## Verdict and scope

**Source: GO. Standards: PASS; Spec: PASS; Security: no confirmed bypass or new blocking finding. S1 remains closed; F1 is closed for this exact source. Delivery/merge acceptance remains pending required hosted gates. Separately, one bounded v2 production observation is GO under the conditions below.**

HEAD: `7a42ffedac495c9b7aeaa0e10c7323dd2a074404`, branch `work/windows-execution-choice`; base: `cd8907500dcd89fddc0c659590f962cbf03c7ed5`. Correction reviewed against `ba959133eea5b1b8be35b39ef0ce393e21743e7b`; intervening `1f2b21cc` has no crate changes. Read AGENTS, KICKOFF, dev-scratch, ADR0029, account documentation, actual first and second reviews, correction/source and relevant surrounding configuration, transport, executor, store, tests and pinned SDK.

All 14 frozen source files match. HEAD stayed pinned. Initially the checkout was clean; a concurrent coordinator-only docs/PROGRESS.md addition later appeared, recording v2 fixture preparation. No WIP source was substituted. This report does not emit a hosted approval or grant merge authority.

## Standards

**No outstanding material finding.** S1's narrow closure applies unchanged: the populated legacy test compares all eight base columns plus 16 metadata fields, canonical index SQL and its principal/actor_kind/id columns, host identity, retained sequence and representative constraints. The empty-v1 deleted-row sequence and damaged-row schema/rows/sequence rollback tests remain present and pass in supplied final-source run05. See [store tests](/D:/Code/Arxcovo/crates/arxd/src/store/account_choice_tests.rs:118) and [rollback test](/D:/Code/Arxcovo/crates/arxd/src/development/account_choice_tests.rs:502). Neither these files nor store/schema changed since the second review. This preserves S1 closure without extending it into a claim about the ten red integration cases.

The correction is one lifecycle concern, with meaningful negative-before/positive-after SDK regression evidence. No material Fowler smell or documented source-standard violation identified. Correction `git diff --check ba959133 HEAD` produced no diagnostics. Required exact-head hosted checks remain delivery requirements.

## Spec

**F1 closed: admission and completion now share a synchronized boundary.**

[Execution::admit](/D:/Code/Arxcovo/crates/arxd/src/development/admission.rs:42) increments the live count with CAS only while the CLOSED bit is clear, including an overflow guard. [drain](/D:/Code/Arxcovo/crates/arxd/src/development/admission.rs:77) sets CLOSED by RMW on that same atomic before observing zero. Any racing successful admission is ordered before closure and counted; any later attempt fails. Detached SDK handlers accepted but not yet polled therefore cannot create a new row after acknowledged quiescence.

[Inventory::call_tool](/D:/Code/Arxcovo/crates/arxd/src/mcp.rs:107) acquires admission before discovery observation or any activity start/read. Authentication/label selection before it has no activity access. Denial returns shutdown without writing a row. get_info/list_tools do not access activity. A successful start is followed synchronously by started(); Activity::start_typed returns only after transaction commit. No await separates these operations.

Branch audit:
- Authentication failure, admission refusal, discovery-observation error or rolled-back start releases no open row. The ticket still counts while a synchronous store operation is in progress.
- Invalid execution arguments, invalid list_activity arguments, unknown tools/arguments, successful list_activity and successful list_hosts clear the open flag only after finish/finish_typed returns successfully.
- Intermediate host/list/decode errors, terminal write errors and panics after start retain the open flag. Drop latches completion_unconfirmed **before** decrementing pending. An Acquire observation of the final count cannot bypass that failure publication. Serialization errors after a successful terminal commit do not invent an uncommitted row.
- Command/discovery transfers the existing ticket into [dispatch_admitted](/D:/Code/Arxcovo/crates/arxd/src/development/mod.rs:370). Worker spawn is synchronous before the returned future; its closure owns the ticket through finish_typed's actual commit. Success and refusal mark completed only afterward. Storage error, worker panic or destruction of queued work drops an open ticket and latches failure.
- SDK cancellation drops the returned future and cancels the child via CancelOnDrop; the blocking worker retains audit ownership. Await failure does not manufacture successful completion. Busy, shutdown, unavailable/dedicated and quarantine refusals also run through terminal audit. There is no release-before-commit path in production foreground handling.

The process-lifetime failure latch now also covers read-tool terminal commit failure whenever a foreground executor is configured. **Accepted as intentional:** those reads write durable started rows, so acknowledging a clean drain after losing their terminal write would repeat the truthfulness defect. Successful reads clear normally. This specific read-storage fault has source analysis rather than a dedicated executed fault-injection result; the shared open-ticket failure behavior has the supplied worker panic/storage tests. Inventory-only and HTTP configurations have execution=None and retain their prior lifecycle scope.

shutdown() cancels execution but intentionally leaves admission open for cached inventory compatibility. Handlers admitted before drain closes remain counted; drain preserves the existing 500 x 10 ms polling budget and returns false on outstanding tickets or latched failure. Foreground maps unconfirmed drain to nonzero. Actual signal/process-exit behavior was not exercised; the two foreground units prove mapping and HTTP response drain only. The listener join remains useful connection cleanup, not proof of SDK handler completion.

Optional foreground dedicated_account is the setup-required Execution object, so it uses the same admission/audit path and never invokes the current-user backend. Installed-service dedicated dispatch remains separate and unchanged. The theoretical Inventory with both a service executor and foreground executor is not constructed by production configuration; it is not a reachable bypass finding.

## Regression quality and test seams

The actual rmcp 3.9.0 source at service.rs:1609 detaches request tasks. Its waiting/response-drain lifecycle does not join them. New tests use real authenticated named pipe, bridge_io, listener/connection and rmcp client/server; the held execution probe panics if execution unexpectedly reaches it. They exercise the admission lifecycle without claiming real Job/token qualification.

[New SDK cases](/D:/Code/Arxcovo/crates/arxd/src/local_execution_tests.rs:782) hold after started commit or before admission, then follow executor shutdown -> listener stop/join -> drain. Run03 fails at the intended assertions: two early acknowledgements with rows ["started"], one late handler writing ["refused:shutdown"]. Run02's invalid endpoint is malformed fixture input and supplies no regression proof.

Run05 passes the same three cases. The full local_execution_tests.rs hash is identical in failing-before source records and final source. The seam-only diff adds checkpoint/test support without adding the fix. Entry is a hold at first handler poll before admission, rather than a scheduler instrument proving an SDK task literally remained unpolled; it faithfully exercises the relevant pre-admission continuation. The CAS closure argument covers genuinely unpolled tasks too. The started-row hold proves the original write-before-dispatch interval; the retained store-mutex test covers audit commit after work.

Checkpoint/Hold/probe constructors and direct dispatch/register are cfg(test). Production uses dispatch_admitted and cannot register after closure through direct dispatch. Test fixtures omit native qualification by design; none are production switches. The direct-dispatch dropped-before-first-poll real Job test remains UNRUN locally, including its changed cfg(test) adapter. The new cases do not substitute for that test or full real execution SDK acceptance.

## Security, boundary impact and reversibility

No source path makes SYSTEM/service/session0/elevated/enabled-admin tokens the default. Current-user construction still performs production preflight, with suspended-child policy and actual TokenUser equality before resume. No raw identity/password selector or HTTP execution was introduced; local principal is immutable authenticated connection state. Dedicated selection remains explicit, setup-required and without fallback/password collection. Legacy config and history retain distinct meanings.

The correction changes no token/Job engine, schema, SDK, lockfile, limits, timeout, quarantine, capacity or CI implementation. Windows token checks and migration changes in the full feature retain the prior review's supported conclusions. Actual unsafe-token matrix, dedicated native/S4U, engine/founder and VM paths remain **unverified on real hardware** by this review.

Schema v4 still affects every Activity::open, including inventory-only and other OS state. Earlier binaries refuse v4. The documented stopped-daemon private backup and restore procedure is necessary for deployment against existing history; rolling the binary back alone is insufficient, and restore loses post-backup activity. No restore or existing-owner-state upgrade was tested here. A fresh-state v2 observation avoids that deployment risk but proves no restore claim.

## Artifact checks and limits

Reviewer ran no builds/tests, runtime/native probes, retries, admin operations, delegation, remote CI/PR/watch operations or owner-data reads. Commands only inspected source/Git, hashed/parsed supplied evidence and wrote this report. Every command explicitly set TMP=TEMP=D:/Temp/arx-execution-choice-review-r3/tmp, CARGO_TARGET_DIR=D:/Temp/arx-execution-choice-review-r3/target, the requested D: Cargo home, and task scratch/observer roots. Initial measured C: free was **42,629,701,632 bytes**, above 20 GB. No large build was attempted.

Independent artifact verification:
- **14/14** frozen source hashes and byte sizes match; **7/7** returned source hashes/sizes match.
- **9/9** command-log SHA256 values match the Opus manifest.
- Binary, source.diff, pinned SDK source and v2 draft: **4/4** hashes match; binary size **14,917,120 bytes**.
- Saved source.diff exactly equals the committed correction for its six changed crate files after newline normalization.
- Unchanged driver hash matches **45487510762f56886a46c5abe674c32f985997925c1751e1e25190fa3d17ecfe**.
- Copied failing-before run03 hash matches its command log; recorded failing-before test-file hash matches final source.

The returned manifest describes precommit HEAD 1f2b21cc plus uncommitted source; the frozen 7a42ffed manifest and matching hashes/diff bind that evidence to this review. This is provenance validation, not an independent rebuild.

Read executed logs: run05 **31 PASS** in one run (13 account/store + 9 legacy pure + 5 MCP + 1 listener + 3 new), 8.422 s command/7.96 s body; fmt PASS; all-target workspace Clippy -D warnings PASS, 9.684 s; foreground 2 PASS, 14.183 s; normal binary build PASS, 13.943 s. These are supplied executed artifacts, not reviewer-run checks.

Ten activity_v2 cases remain **RED at ACL fixture setup**, SetNamedSecurityInfoW AccessDenied=5, before behavior. No base runtime comparison exists. They were not rerun, waived, repaired or classified pre-existing. Full real execution SDK/engine/token/founder and non-Windows checks are UNRUN locally. Green hosted required checks, ci-time-budget and agent-review are still mandatory; none was queried or dispatched here. Source GO is separate from those unresolved acceptance gates.

## Separate v2 observation disposition

**GO for one new-source production observation**, using binary
`D:/Temp/arx-identity-d-validation-f4e7e857/target/debug/arxd.exe`,
SHA256 **f889367307c8561cd813b115b6a6fb908631bf736f233f2d2ca8f7f7d6224b48**.
Recheck the hash immediately before launch and require exact equality. The overwritten e856d838 binary's v1 provenance remains historical.

Reviewed draft hash **fccaeaa76c0bde40f9292b406b5921e55370b5c11642c7a09b50430dc4341089** uses fresh-v2 state and valid 17-character synthetic account with assertions, the same private DACL, no HTTP/password/account creation and D: child scratch. The unchanged driver checks current-user inventory, cargo --version, actual Job identity/cleanup, correlated terminal activity/privacy and restart persistence, then optional dedicated refusal.

During this review the coordinator's progress WIP and sanitized v2 preparation-proof.json report fixtures now prepared, all preparation booleans true, exact reviewed source/binary, runtime UNRUN. I read only sanitized proof and scripts, not private configs/credentials. Thus the draft's original "not executed" provenance describes its return time; this review itself performed no preparation or runtime action. Do not rerun preparation against the existing root.

Keep a **90-second outer deadline**, with hidden launch/cleanup limited to owned Popen handles. The outer deadline is necessary because synchronous pipe writes/process creation are not independently bounded by RPC deadlines. Preserve sanitized outcome and cleanup-unconfirmed failures; no broad process-name cleanup, elevation, bypass or automatic retry. Forced restart on the successful path must occur only after observed terminal commit and child cleanup. Failure cleanup must not be reported as graceful proof.

V1 already passed its current-user steps and failed the combined journey at the invalid 21-character optional dedicated fixture. That is invalid input, not a backend bug; preserve its FAIL/exit1 and historical evidence. V2 is a separate new-source observation, not rewriting v1. A v2 pass proves neither real graceful signal shutdown nor Claude Code acceptance. Isolated Claude Code preflight remains unauthenticated; no copied credentials or model call is credited.

M2 OPEN, M3 UNCHECKED, full v1 incomplete. Engine Sol 3/3 exhausted, historical Opus 0/2 and PR182 HOLD/NOFIX remain unchanged; the single owner-authorized Opus feature attempt is complete. No new attempt or engine/founder diagnostic/fix follows. All Lab Manager/state/Hyper-V, native/admin, cache/user-data and babysitter fences remain.

Standards: 0 outstanding material findings (S1 closed). Spec: 0 outstanding material findings (F1 closed). Security: no confirmed bypass. Source GO; hosted acceptance pending; separate bounded v2 observation GO.
