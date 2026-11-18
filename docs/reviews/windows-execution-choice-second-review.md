# Independent Windows execution choice re-review

Reviewer: GPT-6.1 Sol / high, independent read-only review sub-agent. Date: 2026-11-18.

## Verdict and exact scope

**Exact-source acceptance/merge: NO-GO. S1 closed; F1 remains open at the SDK admission/shutdown boundary. Bounded current-user smoke: GO for evidence collection only**, under the restrictions below. No fix authority, extra attempt, test rerun, CI dispatch or merge authority follows.

Reviewed committed HEAD `ba959133eea5b1b8be35b39ef0ce393e21743e7b`, branch `work/windows-execution-choice`, against base `cd8907500dcd89fddc0c659590f962cbf03c7ed5`, including corrections `96592cf4..HEAD`. Read AGENTS, KICKOFF, dev-scratch, ADR0029, account documentation and first review, actual feature code/tests and surrounding configuration, local/HTTP transports, executor, store and SDK lifecycle. This is a source/impact re-review, not production acceptance.

Initial checkout was clean. During review, a concurrent owner/coordinator change appeared in docs/PROGRESS.md only; HEAD and the 13 pinned source files remained the review target. No WIP source was substituted. The late progress note about a possible owner routing exception is outside this committed source verdict; no delegation or fix was undertaken.

## Standards

**S1 closed for its specific missing migration coverage.** The tests now exercise the real Activity::open transaction and compare all 24 columns across populated NULL-mode, dedicated, development and daemon histories, preserving host ID, canonical visibility-index SQL and its three columns, retained high sequence and representative constraints: [store/account_choice_tests.rs:118](/D:/Code/Arxcovo/crates/arxd/src/store/account_choice_tests.rs:118). The empty v1 deleted-row sequence case exercises all upgrades through v4: [line 190](/D:/Code/Arxcovo/crates/arxd/src/store/account_choice_tests.rs:190). Damaged history compares rows, schema and sqlite_sequence before/after rejection and checks user_version stays 3: [account_choice_tests.rs:502](/D:/Code/Arxcovo/crates/arxd/src/development/account_choice_tests.rs:502). These are meaningful preservation tests, not merely assertions of new mode labels; their passing artifacts are verified below. This closure does not turn the ten red integration cases green.

No additional material documented-style breach or Fowler smell found in feature source. Required hosted checks and independent exact-head approval remain unmet delivery gates, not waived by this report. Source correction diff-check passed. Full base-to-head diff-check returned exit 1 for trailing blank lines at docs/adr/0028-owned-tls-append-transport.md:88 and docs/briefs/m3-owned-tls-append.md:387; those unrelated documentation whitespace notices are recorded, not promoted into a feature blocker.

## Spec

**F1 — P2, still open: joining connection tasks does not close SDK admission or join request handlers.**

Relevant changed shutdown call site: [main.rs:138](/D:/Code/Arxcovo/crates/arxd/src/main.rs:138), followed by drain at line 149 and process exit(0) at line 62. [local.rs:203](/D:/Code/Arxcovo/crates/arxd/src/local.rs:203) aborts/joins the outer connection tasks. The connection awaits RunningService::waiting at [line 245](/D:/Code/Arxcovo/crates/arxd/src/local.rs:245). These are not the SDK request-handler tasks.

I inspected the actual pinned rmcp 3.9.0 source, not its cleanup comments:

- [SDK service.rs:1104](/D:/Temp/arx-identity-d-validation-f4e7e857/cargo-home/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.9.0/src/service.rs:1104) takes its JoinHandle and awaits it. Aborting this waiting future drops that handle; the spawned service task remains detached. RunningService's DropGuard cancels cooperatively, without joining.
- [SDK line 1609](/D:/Temp/arx-identity-d-validation-f4e7e857/cargo-home/registry/src/index.crates.io-1949cf8c6b5b557f/rmcp-3.9.0/src/service.rs:1609) spawns each request handler and discards its JoinHandle. Handler execution is not guarded by an outer cancellation select. Cancellation reaches RequestContext cooperatively.
- SDK lines 1774–1819 provide bounded response draining (Closed: 5 s; Cancelled: 2 s), then cancel the child token. They do not establish that every detached handler has returned. Even awaiting SDK close alone would not prove that after its bounded response drain.

Concrete reachable interleaving: an authenticated run_command handler starts/commits its durable started row at [mcp.rs:109](/D:/Code/Arxcovo/crates/arxd/src/mcp.rs:109), then is preempted before dispatch at line 217. Foreground cancels execution and joins the outer connections. That does not join this handler. Pending is still zero, so [development/mod.rs:342](/D:/Code/Arxcovo/crates/arxd/src/development/mod.rs:342) returns true immediately. Foreground can exit 0 with that started row uncompleted, or the detached handler can resume and register its worker after the final zero-count observation. Cancellation is only selected after dispatch. Shutdown will refuse a newly started worker, but its refusal audit still needs to commit. Restart interruption recovery does not establish the previous drain was observed.

This is a source-supported lifecycle race, not a runtime reproduction or evidence of an escaped child/identity bypass. Requirement: ADR0029 says a drain timeout must not be presented as observed cleanup or a clean foreground exit. The same truthfulness requirement rules out a zero-count check before admission has actually stopped.

Actionable closure criterion, not authorization to fix: establish one synchronized admission/completion boundary covering handlers before any started-row write and through terminal commit, and prove shutdown cannot observe quiescence while a detached handler can still enter it. Track/join or otherwise fence the real SDK handlers, including already accepted but not yet polled requests. Preserve existing budgets. A deterministic test must hold a real SDK request before dispatch registration (including the started-row interval), shut down, and prove either completed terminal activity or an unconfirmed/nonzero result. Sending only a later request is insufficient.

**What the correction does close:** Pending is incremented synchronously before spawn_blocking, with RAII retained through finish_typed's actual transaction commit. Its drop latches completion_unconfirmed before decrement on panic, missing-row/storage failure or queued-task destruction. Dropping the dispatch future cancels the child but leaves the worker owning audit completion. The queued-worker, held real store mutex, panic and missing-row tests meaningfully cover those registered-worker intervals. The counter and failure-latch ordering support that narrower guarantee. The real listener test completes list_hosts first, then shuts down an idle connection with execution=None; it proves later traffic fails, not the admitted-handler race. Foreground unit testing proves exit-code mapping and HTTP response drain, not a real signal/exit journey. Thus F1 is partially repaired, not closed.

Other requested behavior is source-supported: omitted normal mode selects launching-process current_user only when execution is configured; no arbitrary desktop impersonation, acknowledgement or HTTP execution; immutable authenticated local Principal; explicit dedicated selection reports setup-required without invoking current-user backend or collecting passwords; legacy config labels and provenance remain distinct. No additional product approval flow appeared.

## Security, impact and reversibility

Production new/current_user goes through token preflight. Facts reject built-in service users, SERVICE logon group, session 0, elevation, non-Medium integrity and enabled administrator membership; failures refuse. The same policy and actual parent/child TokenUser equality run while the child is suspended before ResumeThread. cfg(test) policy/probe seams cannot be configured in the production binary. No confirmed SYSTEM/service/elevated default or dedicated fallback found. Actual unsafe-token matrix and dedicated paths remain **unverified on real hardware**.

The schema rebuild affects every Activity::open, including inventory-only and other OS state. It copies eight base columns plus all 16 metadata fields inside the transaction, preserves sequence, recreates canonical constraints/index and rolls back damaged-row failures. Started-row interruption recovery retains its existing intentional meaning. Current-user metadata rejects raw owner identity/rights; start-time mode stays authoritative during finish. S1 evidence now supports these preservation paths.

Earlier binaries refuse schema v4. [Account documentation:61](/D:/Code/Arxcovo/docs/windows-execution-accounts.md:61) now requires a stopped-daemon private state backup before upgrade and backup restoration for rollback, with post-backup activity loss stated. This is operational reversibility guidance, not a tested restore. Fresh-state smoke does not exercise owner-history upgrade or restore.

Actual Windows source diff preserves the existing polling, child cleanup, capture/HTTP budgets and quarantine mechanism; only token policy and mode/claim rendering changed there. Foreground HTTP drain remains five seconds. No engine, timeout, quarantine, capacity or privileged native fix was made or authorized. Paths/config disjointness and local bridge peer-identity-before-credential checks remain technical fences; current-user mode truthfully shares the launching account's access.

## Artifact checks and executed-evidence limits

Reviewer executed no builds, tests, runtime/native probes, retries, admin, remote/PR/watch operations or agents. Commands were source reads, Git inspection and hashing, with requested TMP/TEMP/target/Cargo home and observer/scratch roots explicitly on D:. Initial C: free: 52,920,303,616 bytes. Only this report and permitted task directories were created.

Independently checked correction-manifest.json: all **13 source hashes/sizes**, **12 artifact hashes/sizes**, production binary and smoke-driver hash/size match. Manifest base/head match target. Production arxd.exe: SHA256 `e856d8381f5a2a56582a2d4b25cc8d67f54cbb7ce2ca06c05b25d9fe0fe62829`, 14,913,024 bytes. The manifest is provenance evidence, not an independently rebuilt binary.

Read passing logs: 13 account/store cases, 1 real local listener stop case, 9 legacy pure cases, 2 foreground units; final workspace/all-targets Clippy -D warnings and production rebuild passed. These are supplied executed artifacts, not checks run by this reviewer. Final main comment-only change follows the foreground tests; final production rebuild is retained.

Unsuccessful evidence remains explicit: paused-clock variant was stopped at 59.9689184 s, exit -1/0xffffffff; intermediate Clippy exited 101 for collapsible_if before the final passing result. Neither is counted as a pass.

Historical artifact manifest: all 20 log hashes/sizes match. Selected six logs independently yield exactly 49 unique passing names, identical to selected-test-proof.json; they span compatible prior revisions, not one current-head 49-case run. Historical source manifest's head field is the docs commit 94fd32c0; its 12 file hashes match initial feature git blobs at 96592cf4, with checkout CRLF conversion required for local_execution_tests.rs and activity_v2.rs. Its old production binary path has since been rebuilt: old hash 54aebb... is historical, not current proof. Correction manifest supersedes it.

SDK service.rs SHA256: `ab8b0162620da8ca98853dc6c06908a228bfb97289193ff6d5c38e7b12b01e49`; lockfile pins rmcp 3.9.0 (package checksum d24a7e92...). This identifies the lifecycle source inspected.

Ten activity_v2 integration cases remain **RED**, all failing ACL fixture setup at inventory.rs:485, SetNamedSecurityInfoW=5, before behavior. There is no base runtime comparison. They were not rerun, waived, fixed or classified as pre-existing. Hosted required checks, ci-time-budget and agent-review remain mandatory; no remote gate was queried here.

## Separate bounded-smoke disposition

**GO for the described narrow production evidence journey; NOT RUN.** Reviewed driver SHA256 `45487510762f56886a46c5abe674c32f985997925c1751e1e25190fa3d17ecfe` and read prepare_smoke.ps1 (SHA256 `84c62a8010449e9f1d290c8f00dc5b8416c337bff9e152391e014b7d18fa05ec`). Preparation refuses an existing task root/reparse parents, creates a private D: tree and ephemeral credential, pins the actual daemon user, configures no HTTP and no legacy acknowledgement. Do not print/read back private configs into review logs.

The driver uses the production daemon and stdio bridge, fixed cargo --version (10 s, 4096-byte output), bounded RPC/frame/queue waits, and sanitized result fields. It checks capability, actual identity match/Job cleanup, correlated durable terminal row, absence of raw owner identity/rights, and persistence across forced daemon restart. Optional dedicated refusal cannot execute via fallback. It launches only its own hidden processes and cleanup targets only those Popen handles.

Keep the runner under a finite outer task deadline as well: synchronous pipe writes/process creation are not independently timed by Python's RPC deadline. Use the same D: environment and verified binary; refuse production preflight failure without bypass, elevation or retry. Forced stop/restart occurs after terminal command activity and observed child cleanup on the successful path; failure cleanup must remain failed/unconfirmed evidence. The driver correctly sets graceful_shutdown_proved=false. It must not be used as an F1 graceful-shutdown reproduction or closure, and its Python MCP client is not Claude Code acceptance. A passing smoke does not clear source NO-GO, red integration tests or hosted gates.

M2 OPEN / M3 UNCHECKED. PR182 HOLD/NOFIX, engine Sol 3/3 exhausted and historical Opus 0/2 unchanged. No founder diagnostic/fix, S4U/dedicated/native VM dispatch, Lab Manager/state/Hyper-V access, owner-admin action, cache/user-data modification or babysitter change occurred.

Standards: 0 outstanding material findings (S1 closed); Spec: 1 outstanding P2 finding (F1 admission/audit shutdown race). Bounded-smoke evidence collection GO; exact-source acceptance/merge NO-GO.
