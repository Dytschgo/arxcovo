# Arxcovo progress

## Current state — 2026-11-11

- Milestone 1 is accepted. Milestone 2 is in progress. Milestones 3–6 remain unimplemented apart from research and scaffolding.
- Last merged PR: [#28](https://github.com/Dytschgo/arxcovo/pull/28), `0ceb1745e2e6dafad6daf5394747a3e094102c35`, 13:46:02 UTC. Independent Opus review and all required checks passed; kernel object-identity regression merged through normal gates.
- Main has authenticated host inventory/activity over HTTP and local IPC/stdio, persistent host identity and opt-in local-only Windows current-user command execution. The non-test Claude Code development acceptance passed; per-OS installed services and dedicated-account execution remain incomplete.
- Current work: record the passed Windows development acceptance, finish service lifecycle review corrections, integrate fixed/native service configuration and wire the real service application. Dedicated-account/installed-service M2 acceptance and M3–6 remain open. No third S4U probe is planned.

## Accepted milestone 1 evidence

- Public AGPL-3.0 repository, Cargo/npm workspaces, pinned tools and lockfiles, React/Electron scaffolds, contribution/security guidance and post-v1 roadmap are merged.
- Three-OS CI runs Rust formatting, Clippy with warnings denied, nextest, UI lint/test/build, Chromium smoke and Electron isolation tests. Electron coverage merged in #7 as `9c62344`; final reviewed head `8b899ba` passed run 42986780669 on all three OSes. Five protection-removal mutations failed in the independent review.
- Main protection requires three platform checks, `ci-time-budget` and `agent-review`. CI/budget are pinned to Actions app 15368. Strict mode and administrator enforcement are on, zero GitHub-user approvals are required, and force-push/deletion are blocked. No gate was bypassed.
- Owner selected `Dytschgo` for the agent-author allowlist. Outsider PRs never auto-merge. Default Actions tokens are read-only; fork workflows require owner approval.
- Trivial PR #3: auto-merge enabled 03:00:12, independent review passed, last CI passed 03:04:03, and GitHub merged at 03:06:17 without a later coordinator merge command.
- Slow-main experiment: #4 introduced a temporary Linux delay. Main run 42952698370 succeeded in 1016 seconds. Probe #5 at `690bf5d` had every other gate green, but budget failed; auto-merge was enabled at 08:05:19 and the PR remained blocked.
- Recovery #6: its successful run 42953952528 took 275 seconds and passed the measured `ci-speed` exception. It merged as `e692b20` at 08:05:25. The following main run 42984281226 took 251 seconds. Probe #5 was updated and freshly reviewed at `aaaf52c`, then auto-merged at 08:12:14 through all gates.
- Evidence PR #8 merged as `3148241`. Milestone 1 is complete. The trusted evaluator uses default-branch `workflow_run`, real first-attempt API durations, deduplicated statuses and isolated PR failures. History does not gate merges. API availability and the open-PR cap remain limitations.
- CI duration history is in docs/ci-duration-history.json; additional API-derived main records are preserved on docs/m2-validation-checkpoint and still need publication.

## Milestone 2: merged and open work

- Inventory/activity #11 merged as `fa5e414` after its blocking inherit-only Windows ACL bug was fixed and independently re-reviewed. Persistent host identity #13 merged as `90d3ce7`, including atomic migration and legacy replay evidence.
- Windows probe #12 merged as `03adb434` after four review blockers and the deny-only regression were corrected. Diagnostic follow-up #17 merged as `2299ad4` after exact-head review and three-OS CI passed. Its pure suites passed 115 helper and 90 setup assertions; 17 guard mutations were killed. These are software checks, not S4U acceptance.
- First manual Windows run 42995659949 failed without locating its setup failure; cleanup reported true. Second run [43003724132](https://github.com/Dytschgo/arxcovo/actions/runs/43003724132), at reviewed immutable `2299ad4`, returned task code 2 after successful privilege and account API diagnostics. Setup reported result-validation failure and cleanup true. No S4U success is established. The dispatch variable was deleted and read back absent.
- Owner steering: use the current unelevated Windows user for the development test rather than spend more time on the dedicated-account blocker. ADR 0010 is committed on docs/windows-current-user-development. S4U diagnosis cancellation was requested. This is an explicit development deviation, not proof of dedicated-account isolation or authorization for owner-PC administration.
- Transport #15 is merged. Independent review cleared peer-error isolation and strict Windows TokenUser ownership. Hosted run 43004413465 passed 28 workspace tests on each platform, including macOS initialization, early-close and endpoint collision cases. Same-user development boundaries remain explicit.
- Unix library/harness #14: latest correction `4383b56` fixes the positive-control directory's restrictive-umask issue. Local Python guards: 29 tests and ten killed mutations; Windows workspace: 22 tests. Fresh independent review and hosted acceptance are pending. No privileged Linux experiment has run.
- Windows rooted-path fix #16 is merged after fresh independent review; its bare-prefix correction passed 39 local tests. Activity-integrity #18 is closed as superseded by merged #19, including completion and fixed unknown-tool-label regressions.
- Activity v2 merged in #19: atomic migration/recovery, caller isolation, bounded keyset pagination, separate read/action retention, conditional completion and 8 KiB request/64 KiB response bounds. Exact-head independent review and all hosted checks passed; see ADR 0011.
- Lifecycle candidate `522ed2b`, integrated on feat/service-lifecycle: state machine, shutdown ordering, Windows SCM adapter, Unix signals/notify and exact CLI entry. Coordinator passed 40 Windows tests, formatting and Clippy after transport integration. Production entry remains fail-closed until fixed config, store and inventory hooks are wired. Native service-manager operation is unverified.
- Constant systemd/launchd artifacts are committed on feat/service-install-artifacts. INI/plist syntax, fixed argv and absence of interpolation were checked; no manager or installation was invoked. Full ADR 0009 and the owner service guide remain on the service branch.
- Still required: actual service/configuration integration, execution backend and rights enforcement, account-scoped tool discovery, cancellation/restart cleanup, and the real Claude Code MCP command/activity check. Development-current-user results must identify that mode and avoid private owner identity in output. The original dedicated-account criterion remains open.

## Milestone acceptance checklist

- [x] 1. Repository, CI/review gates, trivial auto-merge and slow-main rejection/recovery.
- [ ] 2. Per-OS service, host/tools inventory, command execution and persisted activity; Claude Code acceptance.
- [ ] 3. Join codes, mTLS, Raft, cross-host command, three-node failure and expired/reused-code acceptance.
- [ ] 4. Windows/Ubuntu templates, guest agent, snapshot/lifetime/caps/reserves and real VM lifecycle acceptance.
- [ ] 5. Versioned scripts/templates with throwaway validation and broken-version rejection.
- [ ] 6. Authenticated browser/Electron dashboard with hosts, VMs, activity, terminal and console; Playwright and screenshots.

## Blockers and owner actions

- Dedicated local Arxcovo account and QEMU were absent at preflight. Owner account/service installation and later WHPX/QEMU/media/disk-cap/CPU-RAM reserve setup remain owner actions. The current-user development authorization permits the development test without doing those administrative steps.
- docs/dev-host-setup.md and early feasibility ADRs are preserved in open PR #10. Policy regression PR #9 is also open. These have not been accepted or merged.
- All VM/hypervisor paths, S4U/cross-account execution and installed services remain **unverified on real hardware**. Hosted software tests do not establish owner-PC installation or VM acceptance.
- Never touch the old Lab Manager installation/state or Hyper-V VM `agent-windows-test`. No agent performed owner-PC account, rights, service or task administration.
- Post-v1 scope stays in docs/ROADMAP.md. No milestone 3 implementation starts before milestone 2 acceptance.

## Oversight

- [Arxcovo babysitter](t3-thread://v2/950f5165-f55d-4ffd-9b15-4b87e10b4d15/mcp:launch:6ddd1ab43a55d1439cb40e2639d67190751dd74482d63b63ba999264bd36f7b0).
- Requested schedule: `scheduled-task:91fbb832-3283-440c-8a0d-e4a89b2d19d0`, every three hours. It has not been cancelled or rescheduled.
- The babysitter's stall-recovery steering was applied: work was checkpointed, slow-CI rejection/recovery proved, and pending work preserved on branches. No protection was weakened.

## Provider history

The rows below preserve task-time reports from the prior progress records. Pending states in these historical rows are superseded by the current-state sections above; they are not current merge or acceptance claims.

| Task | Provider/model | Effort | Task-time outcome |
| --- | --- | --- | --- |
| Coordination, integration and settings | Codex / gpt-6.1-sol | medium | PRs #1–2 merged; full protection read back; first baseline verified |
| Initial scaffold/CI builder | Codex / gpt-6.1-sol | medium | PR #1 built and locally verified |
| CI gate architecture | Claude / claude-opus-5-5 | high | App-free workflow_run design and staged installation advised |
| PR #1 review round 1 | Claude / claude-opus-5-5 | high | Failure on 835d06c: history deadlock, error handling, labels and extra approval requirements |
| First gate corrections | Codex / gpt-6.1-sol | medium | Evaluator tests and trusted workflow triggers; history separated from gate |
| PR #1 review round 2 | Claude / claude-opus-5-5 | high | Failure on 0218ba8: status exhaustion/cross-PR error propagation |
| Status-limit corrections and UI test | Codex / gpt-6.1-sol | medium | Idempotent publication and failing-head isolation; 14 policy tests plus UI test passed |
| PR #1 review round 3 | Claude / claude-opus-5-5 | high | Success on 5b07ea1; installation-ready, not milestone acceptance |
| PR #2 error fallback and guard tests | Codex / gpt-6.1-sol | medium | Discovery-only awaited fallback, checks-only payload; 18 policy tests passed |
| PR #2 independent review | Claude / claude-opus-5-5 | high | Success on 44563ce, evidence comment linked by agent-review |
| PR #3 baseline documentation | Codex / gpt-6.1-sol | medium | API-derived history and reconciled progress; auto-merge acceptance passed |
| PR #3 independent review | Claude / claude-opus-5-5 | high | Success on f432908; baseline and protection independently verified |
| Slow-main fixture | Codex / gpt-6.1-sol | medium | Temporary Ubuntu delay and recovery procedure prepared; acceptance pending |
| PR #4 independent review | Claude / claude-opus-5-5 | high | Success on 720f47c; experiment approved, live fence/recovery still pending |
| PR #5 independent review | Claude / claude-opus-5-5 | high | Success on 690bf5d; doc-only probe, slow baseline and otherwise-green rejection still pending |
| PR #5 recovered-main review | Claude / claude-opus-5-5 | high | Success on aaaf52c; rejection evidence independently checked, final Windows CI and normal merge pending |
| PR #6 independent review | Claude / claude-opus-5-5 | high | Success on 1da52c2; all real CI checks preserved, measured recovery still pending |
| Probe PR #5 builder | Codex / gpt-6.1-sol | medium | Rejection observed on 690bf5d; recovered aaaf52c auto-merged through all gates |
| Recovery PR #6 builder | Codex / gpt-6.1-sol | medium | Removed fixture, successful measured ci-speed exception and merge; fast main verified |
| Evidence PR #8 builder | Codex / gpt-6.1-sol | medium | API-derived history and acceptance outcomes; correcting stale ADR/progress after review |
| PR #8 review round 1 | Claude / claude-opus-5-5 | high | Failure on ebdb8f4: stale ADR status; acceptance evidence independently confirmed |
| PR #8 review round 2 | Claude / claude-opus-5-5 | high | Success on 3b6c819 after documentation corrections; all CI passed and #8 merged |
| MCP inventory/activity tracer | Codex / gpt-6.1-sol | high | Builder commit e53b582: fmt/clippy/12 nextest tests and UI checks passed locally. Integrated for independent review and hosted CI; execution remains unavailable and M2 incomplete |
| PR #11 security review round 1 | Claude / claude-opus-5-5 | high | Failure on 6abff15: inherit-only Windows ACL grants exposed newly created activity files; remaining path/ownership/DoS observations recorded as follow-up work |
| PR #11 ACL correction | Codex / gpt-6.1-sol | medium | New Windows inheritance-only regression failed before the fix; all 13 workspace tests, formatting and Clippy passed after rejecting those grants before file creation. Fresh review and hosted checks required |
| Windows execution feasibility probe | Codex / gpt-6.1-sol | high | Commit 24d3c7e prepared separately; build/publish and 21 unprivileged self-test assertions passed. Privileged disposable-CI probe is unrun; review required before any such execution |
| PR #11 security review round 2 | Claude / claude-opus-5-5 | high | Success on 181ddff; 13 tests and old-code mutation confirmed correction; all hosted checks passed and PR merged |
| Unix execution library | Codex / gpt-6.1-sol | high | Candidate 46cba1f: 19 portable Windows tests passed; Unix metadata compilation passed, runtime and privileged harness unverified; independent review required |
| PR #12 security review round 1 | Claude / claude-opus-5-5 | high | Failure on d42c33e: four blockers; no privileged execution performed |
| PR #13 independent review | Claude / claude-opus-5-5 | high | Success on 978065b; migration/recovery atomicity mutation and legacy rollback replay passed; all hosted checks passed and PR merged |
| PR #12 blocker corrections | Codex / gpt-6.1-sol | high | Candidate 94be783: four blockers addressed with unprivileged reproductions and guard mutations; independent re-review required before privileged dispatch |
| PR #12 pre-dispatch N1 regression | Codex / gpt-6.1-sol | medium | f9e3b92: independent elevation-type check; 34 unprivileged assertions pass, deny-only removal mutation fails at the new assertion |
| PR #12 pre-dispatch recheck | Claude / claude-opus-5-5 | high | Success on f9e3b92; independent build/self-test and mutation confirmed N1 fixed. Requested final docs count/date correction; no privileged execution |
| Local MCP/stdio integration | Codex / gpt-6.1-sol | high | PR #15 base 6f55eb2 received blocking Opus review; correction on t3/fix-pr15-client-isolation. Local Windows 28/28 workspace tests pass; three discriminating mutations fail. Fresh independent review and hosted native CI required |
| Awaited policy-failure regression | Codex / gpt-6.1-sol | medium | PR #9, originating commit 0c16a9e; 10 evaluator tests pass; independent review/merge pending |
| Single-host security contract | Claude / claude-opus-5-5 | high | Advisory design running while setup acceptance waits; no implementation or acceptance claimed |
| Electron verification follow-up | Codex / gpt-6.1-sol | medium | Local Windows test passed; not yet merged |
| PR #7 independent review | Claude / claude-opus-5-5 | high | Success on b8c2187; hosted Electron passed on three OSes, all five protection-removal mutations failed locally |
| Oversight | Claude / claude-opus-5-5 | medium | Initial check-in on track; recurring schedule active |
| Single-host security contract | Claude / claude-opus-5-5 | high | Advisory design prepared; implementation and M2 acceptance remain incomplete |
| Electron verification follow-up | Codex / gpt-6.1-sol | medium | Local and hosted three-OS verification passed; merged in #7 |
| PR #12 full corrected security review | Claude / claude-opus-5-5 | high | Success on 9fbbb76; N1 requested before dispatch, subsequently corrected |
| PR #12 final evidence review | Claude / claude-opus-5-5 | high | Success on 42d1165; all required checks passed, merged as 03adb434 |
| First Windows disposable experiment | Codex / gpt-6.1-sol | medium | Run 42995659949 failed with generic setup error; cleanup reported true; dispatch variable cleared |
| Windows experiment diagnosis | Codex / gpt-6.1-sol | high | Isolated implementation active; bounded diagnostics required before another reviewed run |
| PR #14 security review | Claude / claude-opus-5-5 | high | Failure on c508de0: three disposable tooling/test blockers; Rust library had no blocking finding |
| PR #14 corrections | Codex / gpt-6.1-sol | high | Isolated implementation active; no privileged run or acceptance claimed |
| Local MCP/stdio transport | Codex / gpt-6.1-sol | high | Candidate integrated in #15; 24 local Windows tests pass, hosted Windows/macOS initialization fails |
| PR #15 independent security/failure review | Claude / claude-opus-5-5 | high | Active on pinned 6f55eb2; verdict pending |
| PR #14 corrections | Codex / gpt-6.1-sol | high | Corrections integrated at a5777bb; second review found R1 in positive-control directory permissions; no privileged run or acceptance claimed |
| PR #15 corrected transport re-review | Anthropic / claude-opus-5-5 | high | Success on 282b84a; all three hosted platforms passed; merged as 3a7edc8 |
| PR #17 preflight re-review | Anthropic / claude-opus-5-5 | high | Success on d64e2aa; 115/90 pure assertions and 17 mutations verified; merged as 2299ad4 |
| Activity v2 builder | Codex / gpt-6.1-sol | high | Candidate 6469862; 38 local tests and five restored mutation failures; integration/review pending |
| Lifecycle builder | Codex / gpt-6.1-sol | high | Candidate 522ed2b; 40 local tests; installed-service integration/acceptance pending |
| Current-user development contract | Claude / claude-opus-5-5 | high | ADR 0010 candidate d4725f1; docs only, user-authorized deviation; implementation pending |
| Coordination and integration | Codex / gpt-6.1-sol | medium | Merged #15/#17 through gates; cancelled S4U diagnosis request, reconciled this record, integrated local candidates without claiming acceptance |
| PR20 scoped docs review | Codex / gpt-6.1-sol | high | Changes required on `9b02fe2`: P1 macOS admin cleanup false assurance, P2 audit vocabulary mismatch with merged activity v2, P2 stale status/development contract |
| PR20 service architecture corrections | Claude / claude-opus-5-5 | high | ADR 0009 and the owner guide corrected for all three findings; docs only, fresh Sol review required; no acceptance claimed |

| Windows development execution implementation | Codex / gpt-6.1-sol | high | Incomplete candidate 68c6dbd; stopped after three failed junction-fixture fixes |
| Windows development execution continuation | Anthropic / claude-opus-5-5 | high | Candidate complete locally; fixture root cause fixed; 57 tests and 25/25 mutations killed; fresh independent Sol review pending |

Exact provider allowances and costs are not exposed; no percentages or spend are invented.

## PR20 architecture correction checkpoint

- Claude / claude-opus-5-5, high: architecture corrections after Sol's scoped PR20 documentation review. ADR 0009 now refuses macOS admin mode at configuration, start and per-run preflight until a separately reviewed containment ADR provides timeout/cancel/restart evidence with an elevated descendant, and states exactly what `cleanup_confirmed` attests per OS and rights. Preflight detail is response-only and maps exhaustively onto merged v2 `Reason`; Windows observations persist as `post_switch` and Unix as `pre_exec`; no schema migration is added. The mapping, readback and privacy tests are implementation obligations that have not run. A PR21 config follow-up (macOS admin refused before resolution) is specified in the ADR but not implemented. Status reflects two failed S4U experiments with no third planned, and the separate owner-authorized ADR 0010 development mode. M2 dedicated-account acceptance remains open.

## Service lifecycle integration checkpoint

- Codex / gpt-6.1-sol, medium: the lifecycle branch now integrates main d7f3ebd and its activity v2. Integrated-revision evidence: formatting, all-target workspace Clippy with warnings denied and all 50 runnable nextest tests passed on Windows; one direct-only fixture was skipped and its parent exercised it. Production entry still fails closed until configuration/store/inventory hooks are connected; no installed-service acceptance is claimed. The separate fixed-configuration candidate and native observation implementation remain unmerged work.


## Service contract coordinator integration

- Codex / gpt-6.1-sol, medium: integrated Opus corrections c149cb4 and path-main b29f531. Scoped Sol review findings are addressed in the contract: macOS admin refusal and activity-v2 mappings. Production lifecycle/platform source is unchanged from the prior 50-test revision; main adds the path regression. Integrated formatting, Clippy with warnings denied and all 51 runnable nextest tests passed on Windows (one direct-only fixture skipped and exercised by its parent). Fresh scoped Sol recheck and full independent Opus implementation review remain pending. No service installation or administrative operation ran.

## Historical configuration candidate integration

## Fixed config and owner-plan foundation checkpoint

- Implementation sub-agent: Codex / gpt-6.1-sol, high. Base
  `62dd9a9976602e7673370fd89ecd7db7a63fecc6` (PR20 foundation). Added fixed
  Layout, pure permission policies, bounded service config with injected account
  resolution, typed owner plans and recording/model manager tests. No native
  owner adapter or agent/CLI integration. Last merged PR above is unchanged.
- Acceptance evidence: Windows `cargo test --workspace`: 51 passed, one ignored
  subprocess-only fixture (the existing parent test invokes it explicitly);
  original 40-test baseline retained plus 11 new unit tests. Formatting and
  all-target workspace Clippy with warnings denied pass. Package parser tests:
  three pass, native systemd-analyze/plutil checks skipped on Windows. No UI rerun.
- Deliverable is foundation, not installed-service acceptance. Production
  identity resolver, local-disk/reparse/ACL inspection, atomic privileged writes,
  SCM/LSA and Unix manager actions remain **unverified on real hardware**.
  Atomic writes are plan steps only. Native configure needs recovery for a
  batch grant followed by failed config publication; no transaction is claimed.
- Owner action blockers and M2 acceptance remain open. No owner commands,
  account/rights/task changes, installation, GitHub writes, push or PR occurred.
  The active development executor and Host/platform adapters were not edited.
  Independent different-provider review and hosted checks remain coordinator work.
- API and remaining native obligations: [service-foundation.md](service-foundation.md).

## Service configuration integration with merged activity

- Codex / gpt-6.1-sol, medium: merged main `d7f3ebd` into the local configuration candidate, preserving the service foundation and current activity implementation. Formatting, workspace Clippy with warnings denied and all 61 runnable nextest tests passed (one direct-only fixture skipped; its parent exercised it). Package parser checks passed three tests; systemd/plutil native checks were skipped on Windows. These supersede the earlier 51-test builder-base evidence for this integrated revision. No installation or account/right/manager operations were performed.
- Next service work: replace injected identity and permission observations with native read-only adapters and bounded configuration reads. Owner mutation adapters and service application wiring remain incomplete; native checks will receive their own independent security review before activation.

- Publishing boundary: fixed service configuration and typed owner plans are a separate PR stacked on lifecycle #20. Native read-only observations are being implemented separately by Codex / gpt-6.1-sol, high; owner mutations and installed-service acceptance remain pending.

## macOS administrative execution refusal

- Codex / gpt-6.1-sol, medium: integrating the reviewed-contract correction from PR20. A regression must reject macOS/admin before resolver observations and owner plan construction, while retaining Linux/Windows admin validation and refusing an admin account in macOS standard mode. The new regression failed before the guard (account lookups occurred) and passed after it. Its twelve cases cover all three platforms, both rights settings and both observed membership states, including zero account lookups and no configure plan for macOS/admin. Formatting, Clippy with warnings denied and all 63 runnable nextest tests passed; one direct-only fixture is exercised by its parent. Package checks passed three tests and skipped two native checks. No owner operation was performed.

## Service guide negative-check correction

- Scoped Sol high recheck of 302dbb5 resolved the original architecture findings but found one guide contradiction (https://github.com/Dytschgo/arxcovo/pull/20#issuecomment-6795284384). Codex / gpt-6.1-sol, medium corrected a renamed/missing account to account_missing; a configured name resolving to a different SID/UID remains account_identity_changed. UTF-8 and diff checks pass. No runtime or administrative action ran; full lifecycle implementation review remains pending.

## PR20 lifecycle review corrections

- Opus high full review failed d281c89 with two blockers (https://github.com/Dytschgo/arxcovo/pull/20#issuecomment-6795458828). Codex / gpt-6.1-sol, medium: the TokenOwner parent now checks the exact successful fixture row and one-pass count, and exercises an actual zero-test subprocess as a negative control. Renaming the fixture now fails the parent assertion; original bytes were restored and the parent passed.
- Drain has a separate 10-second budget and stop audit a separate 5-second budget; a timed-out drain still produces no stop row, audit timeout is non-clean, and earlier errors are retained. The late-drain regression failed on the prior combined budget, then passed. Two advisory functional issues are also fixed: cooperative startup is bounded at 30 seconds despite heartbeats, and an unrequested successful serve return is endpoint failure. Both regressions failed before their fixes. ADR0009 records the exact contracts and remaining native blocking-call limitations.
- Formatting, workspace Clippy with warnings denied and all 55 runnable Windows nextest tests passed; one direct-only fixture was exercised by its parent. No SCM/systemd/launchd installation or privileged action ran. Fresh exact-head independent review and hosted checks remain required. CI runtime PR23 is separately reviewed and awaiting its full hosted run; no budget or required check was weakened.


## Historical development implementation records

These task-time records retain the failed Sol handoff and later Opus resolution; the newest checkpoint below governs current status.

## Current-user Windows implementation assignment

- ADR 0010 is integrated from the Opus architecture candidate d4725f1 (read by the coordinator). The owner explicitly authorized the existing unelevated user for development testing; no additional approval is requested. Implementation will keep that mode explicit, local-only, bounded and audited, without persisting or returning private owner identity. Production dedicated-account acceptance remains open.
- Starting from the integrated activity candidate, the next Sol high task implements the Windows Job-based current-user executor and real MCP tools. No owner account, rights, service or task mutation is part of this assignment.

## Windows development execution candidate — implementation handoff (2026-11-11)

- Provider/effort: GPT-6.1 Sol, high. Outcome: incomplete candidate; stopped under
  the task's three-different-failed-fixes rule. Independent other-provider review
  and further implementation are required. No push, PR, dispatch, settings change,
  account/rights/service/task change or positive owner acceptance was performed.
- Base: `895324614e8ff5122ebaf5885004dfec05b8cc79` (activity schema v2 and ADRs
  0010/0011 already present). Last merged PR is unchanged by this work.
- Added: explicit private development config; Windows-only startup preflight;
  current-token suspended CreateProcessW executor; three inherited handles;
  unnamed Job memory/process/UI limits; token comparison before resume;
  cancellation/shutdown/output/time bounds; cleanup confirmation and quarantine;
  MCP execution/discovery/capability wiring; worker-owned audit completion;
  transactional explicit v2→v3 identity_mode migration and development-row CHECKs.
- Actual validation: `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` passed after final code
  edits. The last full `cargo test --workspace --no-fail-fast` failed: 12/13
  library tests passed, 32/32 inventory integration tests passed, 1/1 daemon
  unit test passed, and both CLI integration tests (one per binary) passed.
  The final edits to junction fixture construction were exercised only by its
  focused test, which still failed. No final full-suite pass is claimed.
- Native fixture mechanics passed: timeout kills Job tree, cancellation,
  shutdown, overflow, explicit standard handles and non-inherited sentinel;
  quoter roundtrip via CommandLineToArgvW; production token policy and independent
  elevation query. These use only a cfg(test) policy fixture, not production
  current-owner acceptance. Positive current-owner path, hosted OS checks,
  dedicated-account and service paths remain **unverified on real hardware**.
- Stop evidence: `cargo test -p arxd --lib junction_cwd -- --nocapture` fails at
  junction fixture creation, before executor reparse checking or handle-count
  assertions. Initial argv-form mklink failed with fixed OS text:
  `The filename, directory name, or volume label syntax is incorrect.`
  Three different subsequent fixes also failed: (1) explicit raw quoted cmd argv,
  stripping verbatim prefixes; (2) normalizing forward slashes in fixture paths;
  (3) relative literal `mklink /J junction outside` with fixture working directory.
  Final test reports `junction fixture creation failed`. No more attempts were
  made; coordinator must hand this evidence to the other provider. No delegates
  were started because the implementation brief forbids them.
- Remaining work: repair the junction fixture, verify actual executable-reparse
  refusal and stable handle counts; add execution-specific real authenticated
  local SDK/stdio tests with the unit-only policy fixture, including cancelled
  request audit completion, start/completion storage faults, busy/quarantine,
  principal isolation and mode/schema negatives; broaden exact child-identity
  mismatch and configuration tests; review fixed-error/redaction edge cases and
  resource cleanup. No mutation test suite was run. Do not claim the requested
  complete execution acceptance from the existing inventory transport tests.
- M2 dedicated-account/service criterion remains open. This candidate is only
  the owner-authorized development deviation in ADR 0010. Instructions for the
  coordinator's post-review private acceptance are in
  `docs/windows-development-execution.md`.

## Windows development execution — Opus continuation (2026-11-11)

- Provider/effort: Anthropic claude-opus-5-5, high (other-provider continuation
  under the three-failed-fixes rule). Outcome: candidate complete locally and
  pending a fresh independent Sol review. Base `8953246`, Sol candidate
  `68c6dbd`, head recorded by the coordinator at handoff. No push, PR,
  dispatch, settings, account, rights, service or task change, elevation, S4U
  probe or owner acceptance was performed. Last merged PR is unchanged by this
  work.
- Junction root cause (deterministic loop first): the failure reproduced, and
  captured stderr was `md`'s own error. cmd.exe scans its whole command line for
  switches, so the fixture's argv0 `...\System32/cmd.exe` was parsed as
  `/c md.exe" ...`. All three earlier fixes kept that forward slash. A
  backslash-only path alone made the focused test pass. This was a fixture bug:
  no production code and no reparse guard was weakened. The candidate also had a
  31 KB mojibake literal in the quoter test, now replaced by escapes.
- PR16 dependency: the candidate needed PR16's bare-prefix skip (17b6bc9) for
  canonical verbatim workspaces. On the coordinator's update it also carries the
  root correction 8c3856a (physical `RootDir` required). A new unit test was red
  on the prior code: it accepted bare `\\?\C:` and `\\?\Z:`. The lines are
  identical to PR16.
- Production fixes found in review: `redact` looped forever when a label
  contained its needle (for example a user named `user` or `temp`), wedging the
  single execution slot. The worker was spawned lazily, so a request cancelled
  before first poll left its audit row `started`. It is now spawned on dispatch
  and starts no process if already cancelled. Completion could relabel a row's
  identity mode. The v3 CHECK now also pins development rows to elevation 0,
  integrity 8192, `pre_exec` and `windows_job`. The environment block is now
  sorted case-insensitively, as CreateProcessW requires. SIDs are redacted
  anywhere in the text and the token profile directory is redacted. Results
  carry dropped-byte counts; capability reports `shutdown`.
- New coverage: development execution over the real local transport, with an SDK
  client, the real stdio bridge code (`bridge_io`) and a real named pipe with
  kernel identity checks. Requests reach the authenticated listener and SDK
  server, then the policy-fixture executor. Covered paths: schema without
  mode/identity/executable fields, honest `list_hosts`, a redacted success row
  with fixed development facts, seven argument negatives after `started`, busy,
  SDK cancellation completing the row once with confirmed cleanup, principal
  isolation, start-failure runs nothing, completion-failure withholds the
  result, and identity mismatch never resuming the child and then quarantining.
  Also covered: a dispatch dropped before first poll, junctioned cwd and PATH
  directory (executable reparse) with controls, stable handle counts, nonzero
  exit, config classification and a valid `run_command` when not configured.
- Actual validation at the final code commit: `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` passed.
  `cargo test --workspace --no-fail-fast` passed 58 tests (arxd lib 23,
  inventory 32 including the activity suites, daemon unit 1, CLI 2), all on this
  Windows host only. Hosted macOS/Linux/Windows runs were not performed.
- Mutations (literal, one at a time, each file restored by git): 25 applied,
  25 killed. Two (handle-level reparse check, pre-spawn cancellation check)
  first survived; the tests were strengthened and both are now killed. Covered:
  token policy, child TokenUser comparison, reparse/cwd/executable pins,
  quarantine, Job termination and process limit, dropped counts, redaction loop
  and SIDs, env ordering, busy set/release, eager spawn, env allowlist, HTTP
  refusal, physical root, identity relabel, dev-row CHECKs, MCP argument
  validation and shutdown capability.
- Remaining limits: the positive production current-owner path is
  **unverified on real hardware** until the coordinator's post-review run in
  `docs/windows-development-execution.md`. The `admin_group_enabled` and
  `elevated_identity` token refusals are tested on injected facts and an
  independent elevation query only. ADR 0010's service-config rejection awaits a
  service schema. Redaction stays best effort, and nothing here isolates the
  owner. M2's dedicated-account criterion remains open.

## Current-user development coordinator integration

- Codex / gpt-6.1-sol, medium: integrated completed Opus candidate f1541ce with merged main b29f531. Main differed from the candidate activity base only in the already-carried checked_path correction, the committed Windows path regression and README encoding. The candidate source is retained, the main regression is added, and progress is reconciled. The Opus candidate passed 58 tests and 25 mutations. Integrated-revision formatting, workspace Clippy with warnings denied and all 59 nextest tests passed on Windows; executor, audit, MCP and bridge source remain byte-identical to that completed candidate. Fresh independent Sol review and hosted checks are required before any private acceptance run. No production owner execution or administrative operation has occurred.

- Review ownership: Sol prepared the initial candidate and handed it off after three failed fixture fixes. Opus completed and took ownership of the executor, fixed production defects and strengthened tests. A new independent Sol review must cover the full integrated diff before merging or private production-path acceptance. Hosted checks are pending.

## PR #22 hosted portability correction

- Codex / gpt-6.1-sol, medium: first hosted run 43014149384 failed Linux/macOS Clippy because Windows production helpers and Windows-only fault hooks were compiled unused. Added cfg gates matching their callers, retaining portable redaction/bounds tests under cfg(test); warning enforcement is unchanged. Local formatting, Clippy and all 59 Windows nextest tests pass after the correction. Fresh hosted checks and independent Sol review remain pending. No positive owner MCP acceptance ran.

## CI runtime correction after Windows timeout

- Codex / gpt-6.1-sol, medium: PR22 run 43015177578 passed 59 Windows Rust tests and UI lint/test/build, then hit the unchanged 12-minute job limit during browser installation; browser/Electron smoke steps did not run. No merge or budget gate was bypassed. Hosted logs show activity_retention_and_interrupted_restart at 247.801 seconds and read_flood_preserves_actions_and_lifecycle at 318.726 seconds; the latter controlled the test phase.
- The old restart test still exceeded the pre-v2 1000-row limit even though reads retain 200 rows. It now performs 205 real durable starts. The separate-bucket test still fills/overflows the 1000-action and 200-read buckets, then performs two real additional action starts instead of filling a second complete action bucket. New assertions verify action classification, oldest-action pruning and oldest-read preservation. Production store, workflow, timeout, policy and all checks are unchanged.
- Local focused tests passed before and after. After the correction, formatting, Clippy with warnings denied and all 39 workspace nextest tests pass. Three temporary production mutations (read retention removed, buckets merged, restart recovery removed) each caused assertion failures; the exact original store bytes were restored and verified unchanged. These prove coverage of the retained behaviors, not hosted runtime. Independent Opus review and a fresh complete three-OS CI run remain required.
- Current priority: unblock PR22 through real green gates, then perform private current-user Claude Code acceptance. PR20 service implementation review found two small blockers (owner-fixture false pass and drain/audit deadline mismatch); fixes remain pending. No dedicated-account/service or M2 acceptance is claimed.

## PR22 CI-runtime integration checkpoint

- Codex / gpt-6.1-sol, medium: integrated main 2be8268 into reviewed PR22 head72a5e5f. Only the two reviewed retention tests and progress changed; all arxd production source, Cargo manifests/lock and pinned toolchain are byte-identical to the successful independent Sol review. That review is https://github.com/Dytschgo/arxcovo/pull/22#issuecomment-6795099160. Integrated formatting, workspace Clippy with warnings denied and all 59 nextest tests passed on Windows. Fresh exact-head recheck and hosted checks are pending. No real cargo/Claude Code acceptance has run.
- Service lifecycle review corrections are preserved on feat/service-lifecycle at b32ce16: TokenOwner execution proof, separate drain/audit budgets and bounded startup/unrequested serve failure, with 55 local tests. Configuration macOS-admin refusal is on PR21 at 8975520 with 63 local tests. Native macOS observation candidate b261457 has local type/policy tests but remains unmerged and unverified on native macOS. No service Application or owner installation adapter is activated.

## Service lifecycle integration with Windows development execution

- Codex / gpt-6.1-sol, medium: preserved PR20 review corrections while integrating main 8fafbe7, including the current-user executor and reviewed retention runtime correction. Combined formatting, workspace/all-target Clippy with warnings denied and all 75 runnable Windows nextest tests passed; one direct-only TokenOwner fixture is skipped and exercised by its passing parent. Fresh independent review and hosted checks remain pending. The real development acceptance record is independently reviewed in PR24; installed-service and dedicated-account M2 acceptance remain open.

## Real Claude Code development acceptance

- Codex / gpt-6.1-sol, medium coordinated private acceptance after PR22 merged. Claude Code 2.1.322 was requested to use Opus 5.5 high through its existing login, with unrelated instructions, memory, hooks, connectors and built-in tools disabled; only the explicit arxcovo MCP server and named calls were allowed. Actual MCP events were inspected in memory, not a natural-language success report.
- A non-test arxd binary built from independently reviewed source 72a5e5f listed the host and executed literal cargo --version under the current unelevated Windows user. Production source equivalence to reviewed 59293a8 was checked. SQLite and MCP confirmed the caller's successful row, development identity mode, current-user match, Medium integrity and null raw owner identity/rights. A daemon restart and fresh Claude Code session returned the same row.
- The native executor reported confirmed Job cleanup. The harness stopped its owned processes, an independent native process query found zero remaining acceptance arxd instances, and temporary config/credential files were deleted. Only sanitized booleans, fixed labels, build/source hashes and outcomes are committed in docs/evidence/windows-development-acceptance.json. No raw CLI stream or private owner values were logged or committed.
- This is a passed development acceptance under the owner's explicit alternative, not M2 completion: dedicated-account/service execution, rights enforcement, service Application and native owner adapters remain unfinished. S4U/service and VM paths remain unverified on real hardware. No account, right, task, service, elevation or old Lab Manager action occurred.

## Service lifecycle exact-head review integration

- Anthropic / claude-opus-5-5, high independently passed PR20 at1d41f0d: both prior blockers resolved; 75 runnable Windows tests and seven of eight valid scratch mutations passed their intended checks. The surviving owner-default mutation and other advisory findings remain recorded in the review. Codex / gpt-6.1-sol, medium merged docs-only main66c1b17 and corrected stale evidence wording. Production lifecycle source is unchanged. Fresh exact-head recheck and hosted gates are pending; no service installation or M2 acceptance is claimed.

## Service credential uniqueness correction

- Codex / gpt-6.1-sol, medium found service config allowed the same decoded token hash for distinct principals, unlike foreground Config. The new regression first failed because the duplicate was accepted. Service parsing now rejects duplicate hashes regardless of hex casing while retaining a positive distinct-credential control. Formatting, workspace/all-target Clippy with warnings denied and all 88 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. Independent review and hosted checks are pending. This enables no service or owner operation.

## Windows CI handle-probe investigation

- Opus PR20 exact-head recheck passed d8f034f but identified a Windows failure shared by main66c1b17 and PR25/PR20: actual_job_tree_timeout_cancel_shutdown_overflow_and_handles sees child exit101. Production source is unchanged from earlier green runs. Codex / gpt-6.1-sol, medium ran the focused existing test twice locally; both passed, so no cause is claimed. The parent assertion now includes bounded child stderr as well as stdout for diagnosis. Dedicated high-effort diagnosis is pending; no gate is bypassed and no rerun is claimed as a fix.

- Scoped follow-up, Codex / gpt-6.1-sol / high: base 0de0b725ee976eaa0ac21a531fdd9ea6ab2131e9 (main66c1b173 plus diagnostic). Read the public PR20 addendum and all three exact failed Windows job logs. Locally the original cargo and nextest focused baselines passed once each. A temporary real-executor 64-slot sampler failed 1/64 children in each of two runs, surfacing GetHandleInformation return 1 versus 0 and child exit101. This proves the numeric-validity assumption is invalid locally; hosted logs still do not identify their child assertion.
- Fixed only cfg(test) fixture code: randomly named sentinel, snapshot candidate before opening reference, compare kernel object identity. Child-local bounded native allocation makes reuse deterministic; standard handle type checks, production CreateProcess/three-handle list, Job cleanup and token checks remain unchanged. Restoring numeric-validity checking fails the regression (1/1); temporarily adding the sentinel to the actual production inheritance list also fails (1/1, explicit inherited-object assertion). Both mutations restored. Fixed controlled regression passed directly and under nextest (2/2). Final unforced named-event sampling passed 64/64, with zero spontaneous reuse markers in that run; do not interpret it as proof of a hosted fix.
- Actual validation: cargo fmt --all --check; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo nextest run --workspace --locked: **60/60 passed**, zero skipped, 7.369-second test phase. Original combined test also passed directly. Detailed loop, hypotheses, mutation results and limitations are in tools/windows-identity-probe/DEBUGGING.md. No architectural deviation. Last merged PR remains #24 / main66c1b173. Blockers: fresh hosted Windows result and independent other-provider review pending, coordinator integration required. No push/PR/merge/watch or privileged actions. Dedicated-account/service/VM paths remain **unverified on real hardware** by this fix.

## Windows handle regression merge integration

- Codex / gpt-6.1-sol, medium merged PR28 at reviewed460b617 with all five required gates green. Independent Opus review reproduced the old-test failure3/3, deliberate real inheritance failure3/3 (plus combinedtest2/2), and restored native executor tests passed20/20 repeatedruns. This fixes the reproduced test defect; the exact old hosted assertion was not retained and is not claimed proven. Service lifecycle integrates the correction; formatting, all-target Clippy with warnings denied and all 76 runnable Windows nextest tests passed (one direct-only fixture exercised by its parent). Fresh exact-head review and hosted gates remain pending. Native ancestor-policy blocker in25/27 remains open.
