# Arxcovo progress

## Current state — 2026-11-11

- Milestone 1 is accepted. Milestone 2 is in progress. Milestones 3–6 remain unimplemented apart from research and scaffolding.
- Last merged PR: [#19](https://github.com/Dytschgo/arxcovo/pull/19), `d7f3ebd26e69438abb41ff5d671959e576832803`, 11:00:14 UTC. Exact-head independent Opus review passed on `9e7a603`; all three hosted platforms and both status gates passed. Caller-scoped activity v2 is merged.
- Main now has authenticated HTTP and local IPC/stdio host inventory, persistent host identity and durable activity. It has no command-execution surface, account-scoped tool discovery or installed service.
- Current work: review the service lifecycle foundation, integrate fixed configuration with merged activity v2, and complete the owner-authorized current-user Windows development executor after its other-provider handoff. No third privileged Windows probe is planned.

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
- Windows rooted-path fix #16 remains open; its bare-prefix correction `8c3856a` passed 39 local tests and awaits fresh review. Activity-integrity #18 is closed as superseded by merged #19, including its completion and fixed unknown-tool-label regressions.
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

Exact provider allowances and costs are not exposed; no percentages or spend are invented.

## Service lifecycle integration checkpoint

- The coordinator merged current main 3a7edc8 into the lifecycle branch. Its Rust source is unchanged from the revision that passed all 40 Windows tests, formatting and Clippy. Fixed service artifacts are prepared for this branch. Production entry still fails closed until configuration/store/inventory hooks are connected; no installed-service acceptance is claimed.

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

- Codex / gpt-6.1-sol, medium: merged main `d7f3ebd` into the local configuration candidate, preserving the service foundation and current activity implementation. Integration verification is pending; earlier 51-test evidence applies to the builder base, not this merged revision. No native operations were performed.
