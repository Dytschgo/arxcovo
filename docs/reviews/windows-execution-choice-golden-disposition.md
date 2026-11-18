# PR183 independent golden-failure disposition

Reviewer: GPT-6.1 Sol / high, fresh independent read-only review sub-agent, separate from builder. 2026-11-18. Single reviewer; no delegation.

**Disposition: stale source fixture, not an established production regression. PR183 delivery remains HOLD. A narrow golden refresh is justified in principle; merely copying the new source into the golden is insufficient behavioral verification. No new fix authority follows from this review.**

## Source and scope

Branch work/windows-execution-choice.
- Exact reviewed HEAD: d30c79af8ac363fdb4e39420b0e15425ab5738cd.
- Exact base: cd8907500dcd89fddc0c659590f962cbf03c7ed5.
- Qualifying full-source review: docs/reviews/windows-execution-choice-final-review.md, source 7a42ffedac495c9b7aeaa0e10c7323dd2a074404.
- Current schema-family source GO: D:/Temp/arx-pr183-schema-family-review/review.md. That verdict is prior evidence, not a substitute for the present failure disposition.

Read PROGRESS first, status, AGENTS, KICKOFF, dev-scratch, ADR0029, full qualifying review, current schema review, account documentation, original E7 brief/ADR/evidence, full failing test, relevant caller/domain code, actual base..head MCP and related configuration/execution changes, and qualifying..head crate delta. This is a bounded disposition, not a new whole-feature security audit.

Initial checkout was clean at the exact requested head. During review concurrent coordinator documentation appeared: modified docs/PROGRESS.md and untracked docs/evidence/windows-execution-choice-pr183-golden-observation.md. Read those additions as contextual evidence only. HEAD stayed pinned, and git diff -- crates/arxd remained empty. Reviewer made no repository/index edits.

Relevant exact Git blobs at HEAD:
| Path | Blob |
| --- | --- |
| crates/arxd/src/mcp.rs | 390e937cc3e01ea001cc7e87bd65ea9ac4d93392 |
| crates/arxd/src/development/mod.rs | 3f6da0fad9415e7640cc225733d6fc8b4bd2bad0 |
| crates/arxd/src/mcp/discovery_tests.rs | ccf3e75733aba9ab07e25ac8032f152693be8f21 |
| crates/arxd/src/service/unix_bridge/tests/e7.rs | 0f3df557475c2a496b47de3dffe8527303cc45bf |
| crates/arxd/src/service/unix_bridge/tests/e7-no-cache-golden.txt | 1263f1c1bd5752c6ba981d928e4bfd36eb14919c |

Both E7 blobs are identical at base and head. Qualifying7a..d30 crate changes are confined to five files: Windows-only admission test-helper gates and four test expectation files (application, executor, hot journal, inventory). Inspected actual delta; it changes no production body. Thus the discovery predicate was already in the qualifying production source, not introduced by the latest schema corrections.

## Failure evidence and chronology

Independently hashed retained Ubuntu log D:/Temp/arx-pr183-d30-first-ci/134945073127.log: 209857 bytes, SHA256 b73726d6f134cefd626529a9c53a72c2c4012c477d1d81493b44c1dbedae1cc0. Manifest agrees, reports bounded nontruncated download. No fetch or poll performed.

Retained jobs metadata binds job134945073127 to d30, completed failure, 16:15:18–16:17:39 UTC (141 seconds). Log lines1775–1778 identify e7.rs:488:5, assert left==right, actual capability predicate versus old Option-presence predicate. Summary line1786: 593/678 executed, 592 passed, 1 failed, 22 skipped, 85 unrun, body28.206s. The saved run.json is an earlier queued snapshot; it does not establish current whole-workflow outcome. No Windows/macOS conclusion is inferred.

Preserve three distinct normal-source observations, each attempt1:
1. bfd8aeb4 / CI44334571677: Ubuntu Clippy unused Windows-fixture helpers; Windows schema expected3/actual4; macOS cancellation before runner acquisition. Subsequent integration followup dd9d602b plus f5cee7b6 addressed helper gates and application/hot-journal schema expectations.
2. f5cee7b6ef9dde1345435be7046346fc06115387 / CI44340307270: all three platforms failed executor schema expected3/actual4. Complete schema-family followup d30 corrected that expectation and unsupported-future-version seed4→5.
3. d30 / CI44345053750: present Ubuntu golden failure. Preserve prior failures and unrun cases; this is not another fix attempt or a rerun.

The first two observations are established by committed progress/evidence and prior review, not newly downloaded original logs in this task. Engine Sol3/3 exhausted and historical Opus0/2 remain unchanged. Do not reinterpret these three source observations or two integration followups as permission for a speculative retry loop.

## Standards

**S1 — P2, stale source golden blocks required CI.** e7.rs:472–488 pins the complete pre-cache MCP source block. Golden line19 still says self.execution.is_some(); mcp.rs:331–336 now checks capability available==true. Static LF-normalized text comparison independently returned:
- base extracted pre-cache block equals golden: true;
- head extracted block equals golden with only that predicate replaced: true.

This isolates the entire golden mismatch to the reviewed predicate and its formatting. It is not a schema, cache-data, native execution or nondeterministic runtime failure. Do not remove the assertion, shorten its scope, skip E7, or restore inaccurate production availability merely to satisfy the old source pin.

**S2 — P2, direct behavioral coverage missing for the changed no-cache foreground branch.** AGENTS requires every behavior to have a meaningful test; KICKOFF repeats “a test for every behavior.” The failing test uses activity.dedicated=Some(service executor), execution=None throughout. Its MCP assertions at e7.rs:422–470 validate empty dedicated cache, successful discovery, released tools/time/identity and privacy. They run before the lexical assertion. They never enter the changed foreground predicate. Updating the fixture alone cannot prove unavailable foreground rendering. Capability unit tests and no-fallback dispatch tests do not assert its list_hosts result.

No additional material Fowler-smell finding in this bounded delta. Broad source pinning is intentionally used here; its maintenance requirement does not justify deleting the fence.

## Spec

**No confirmed production regression or missing product requirement found within this failure scope.** ADR0029 requires dedicated-unavailable reporting without fallback and truthful capability/activity. The actual caller local.rs:228 passes immutable startup config.execution; Config::load constructs current-user, dedicated setup-required, or historical development execution. Foreground Execution::capability is false for dedicated setup-required, quarantined or shutdown states. work() refuses dedicated before current-user work. Installed-service dedicated execution uses activity.dedicated, has capability available=false, and remains a separate branch. No new authority or backend activation follows from the predicate.

No-cache behavior derived from source:
| Configuration/state | Discovery result |
| --- | --- |
| inventory-only, neither executor | not_configured |
| installed-service dedicated, including Unix test bridge | unavailable, empty tools/null observation fields |
| available current_user | discover_tools |
| available legacy development_current_user | discover_tools |
| foreground dedicated_account setup-required | not_configured, execution.reason remains dedicated_account_setup_required |
| foreground current/legacy shutdown or quarantine | not_configured, execution.reason remains shutdown/quarantined |
| any released prior cache | cached, tools/time/identity from cache; current execution capability remains independent |

The generic not_configured label describes discovery availability imprecisely for configured-but-unavailable modes, but execution.mode/reason expose the actual state. No spec requires a new label here; changing that label would expand this correction unnecessarily. A capability snapshot is descriptive, not a guarantee that later dispatch will succeed.

Original E7 brief W6 (docs/briefs/0020-linux-hierarchy-implementation.md:555) demands byte-identical no-cache output against d44035d; ADR0020 similarly preserves constants. That promise protects the inert dedicated/Unix domain. The test approximates it with a shared source-block pin, inadvertently covering later foreground logic too. ADR0029 supersedes the Windows default and introduces Some(execution) with no available backend. Both dedicated/Unix and absent-executor output branches remain unchanged; available current-user/legacy output also remains unchanged. Unavailable foreground output intentionally becomes more truthful. Legacy configuration/provenance remains distinct; preserving historical provenance does not require advertising unavailable discovery as callable.

Therefore refresh only the predicate in the source fixture, with a clear statement that historical dedicated/Unix rendering is preserved while foreground availability follows ADR0029. No evidence supports reverting production logic. This is source reasoning, not executed comparison of rendered base/head hosts.

## Existing coverage and bounded related-pin impact

Meaningful existing protections:
- mcp/discovery_tests.rs: released_discovery_roundtrip_retains_cache_after_failures_and_shutdown protects installed-service unavailable rendering, actual SDK cache identity/time, failures/refusal and retained cache despite unavailable execution.
- development_producer_roundtrip_only_releases_complete_audited_inventory protects legacy available no-cache discovery, real audited producer release, incomplete/failure/cancel/storage-error rejection and cached inventory after shutdown.
- account_choice_tests.rs: normal_current_user_dispatch_and_activity_keep_distinct_legacy_provenance protects current-user command/discovery labels and historical rows. It does not call list_hosts.
- dedicated_selection_is_setup_required_and_never_invokes_current_user_backend protects capability and both command/discovery refusals using a panic-on-fallback probe; it does not call list_hosts and is Windows-only.
- development/tests.rs: capability_reports_honest_mode_and_fixed_unavailability checks legacy available/shutdown/quarantine capability, not MCP no-cache presentation.
- service/application/tests.rs:647 checks dedicated unavailable/tools/null fields.

These are source-inspected protections; none was executed by this reviewer. The failing E7 log reaching488 proves its earlier assertions passed in that Ubuntu invocation only.

Bounded repository search for golden references, include_str mcp pins, Option-presence and tool_discovery found one exact MCP golden consumer (E7), its adjacent W8 production/domain source pins, the existing discovery tests, and historical evidence pins. W8 still targets service Native/configured/capability inertness and test-only startup/domain ownership, not the changed foreground availability predicate. Inspected its body and service configured/capability caller; no same-change stale W8 expectation identified. Remaining mcp.rs presence checks at117 and203 control audit/discovery observation and argument handling, not availability display; do not mechanically change them.

scripts/test_discovery_e7_mutations.py targets E7's identity assertion for identity_metadata_bypassed; its anchors are in store/application, not the changed predicate. A baseline golden failure could falsely make its “test failed” criterion look like an intended mutant kill. Do not claim fresh mutation evidence until baseline passes and any later authorized control fails at its intended behavioral assertion. No mutation run performed.

Historical E7/A7/A12 source-pin manifests and older roundtrip evidence describe earlier exact sources. Preserve them as history; a new golden hash must not be silently substituted into those old accepted artifacts. No broader audit is implied.

## Scope if separately authorized later

1. Change only the stale predicate in e7-no-cache-golden.txt; preserve the complete block, dedicated defaults, all E7 assertions and adjacent production/domain pins. Record the ADR0029 compatibility rationale in the new correction evidence rather than rewriting historical acceptance.
2. Add focused SDK tests using existing safe fake/probe seams for no-cache available and unavailable foreground current-user and legacy modes, explicit dedicated setup-required, inventory-only and installed-service dedicated. Assert tool_discovery plus execution_available/mode/reason and truthful identity/isolation; unavailable discovery must refuse without backend invocation. Include shutdown/quarantine before any successful cache release and verify cached inventory still survives unavailability. Keep existing provenance/refusal/cache tests.
3. First establish the targeted behavioral regression with the former presence predicate, then passing corrected behavior, only if such local verification is separately authorized. Golden source equality alone is not this proof. No native/token/Job/founder/engine/VM probe is needed for these renderer branches.
4. Run the focused E7/discovery/account-choice checks, applicable fmt/Clippy, and normal corrected-head required hosted checks under the D: policy, if authorized. Unix E7 needs an eligible Unix environment; Windows-only dedicated construction coverage needs its applicable platform. Stop on new failures for disposition; no automatic retry, manual dispatch/rerun, timeout/settings/quarantine/capacity adjustment or scope expansion.

This task implements none of those steps. Test-only fixture edits are reversible; the feature's schema4 upgrade retains its existing stopped-daemon backup/restore requirement, and binary-only rollback is insufficient.

## Limits and preserved holds

Reviewer ran only Git/source/document/evidence inspection and static text/hash/metadata comparisons; no tests, builds, runtime probes, CI operations, posts, statuses, link/watch, merges, delegation or admin actions. Some initial overly broad output was truncated and relevant required material was read again in bounded slices. Two PowerShell rg wildcard path arguments failed and were replaced by literal directory searches; neither error is a source/test failure.

Every shell command explicitly set TMP=TEMP and ARX_OBSERVER_ROOT/ARX_SCRATCH_ROOT to D:/Temp/arx-pr183-golden-disposition/tmp; CARGO_TARGET_DIR=D:/Temp/arx-identity-d-validation-f4e7e857/target; CARGO_HOME=D:/Temp/arx-identity-d-validation-f4e7e857/cargo-home. Effective environment printed and verified before substantive work. Initial C free37,712,171,008 bytes (>20GB), D free2,776,884,613,120 bytes. Only this report is written under the requested D: task root. Output ancestors checked as ordinary directories, rejecting reparse points before write.

All proposed correction/verification remains UNRUN. No full matrix result, native qualification, source-fix pass, new mutation proof or merge readiness is claimed. Prior S1/F1 closures are not reopened by this isolated failure, and their prior evidence is not independently rerun. Current schema source GO does not make red CI green.

PR183 HOLD; PR182 HOLD/NO FIX; M2 OPEN; M3 UNCHECKED; v1 incomplete. Engine Sol3/3 exhausted and historical Opus0/2 unchanged. Native paths **unverified on real hardware**. No Lab Manager, state, Hyper-V agent-windows-test, VM, user data/cache, owner admin or babysitter action. Owner current-user/full-access authorization does not override this review's explicit read-only/no-new-fix scope.

Standards: 2 findings (stale required golden; missing direct no-cache foreground behavioral coverage). Spec: 0 confirmed production findings in this bounded scope. Stale fixture disposition; correction requires separate authorization and meaningful verification; delivery HOLD.
