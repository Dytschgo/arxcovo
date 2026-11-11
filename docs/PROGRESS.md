# Arxcovo progress

## Current state — 2026-11-11

- Milestone 1 is accepted. Milestone 2 is in progress; milestones 3–6 remain unimplemented apart from research and scaffolding.
- Last merged PR: [#20](https://github.com/Dytschgo/arxcovo/pull/20), `64b50e9187fd469bc886373230e324ace6dd4449`, 17:31:46 UTC, through all five required gates. Service lifecycle foundations are merged; installed-service and dedicated-account acceptance remain open.
- Main includes authenticated inventory/activity, persistent host identity and accepted Windows current-user development execution. Dedicated-account execution and installed per-OS services remain incomplete; M2 acceptance stays unchecked.
- Queue priority: land configuration #21 next, then native validation #25 and Application #27 in dependency order. No new feature PRs while the queue drains. #31 is closed as fully folded into #29, with its commit history and original evidence preserved.
- Main push CI for `3b6170b` passed on its first attempt in run 43066365961: 17:15:24–17:24:32 UTC, **548 seconds**, below 15 minutes. No third S4U probe, owner-PC administration, old Lab Manager or protected VM work is authorized.

## Queue-drain oversight

- Codex / gpt-6.1-sol, medium applied the babysitter's 17:03 UTC queue-drain steering: stop new feature PRs, finish #32 first, then merge existing M2 dependencies and fold or close superseded work. Current state is restored as the first section. Local mirror/progress evidence is committed; the babysitter and its schedule are unchanged.
- The signed-mirror execution path has passed hosted Ubuntu; no further DNS root-cause investigation is planned. If the mitigation fails, record it under the three-attempt handoff rule rather than keep investigating. Do not bypass checks or increase the CI timeout.
- The detailed preparation and failed-draft records below are historical evidence. The first successful hosted result includes the caveat that the original DNS names also resolved, so no causal DNS-fix claim is made.

## CI mirror evidence

### Signed Ubuntu mirror first hosted execution

- Codex / gpt-6.1-sol, medium verified completed [Ubuntu job 130669921798](https://github.com/Dytschgo/arxcovo/actions/runs/43062892241/job/130669921798) at independently reviewed source `400903d`. The helper updated one source. All four resolute suites fetched InRelease metadata from the Kernel Archives HTTPS mirror; no signature/insecure/unsigned/failed-fetch warning appeared. Playwright dependency installation, browser smoke and Linux Electron smoke passed. The complete job took **130 seconds**, below the unchanged 12-minute limit.
- All six DNS controls, including all three original Ubuntu-domain names, resolved on this run. The original DNS timeout did not reproduce. This establishes the reviewed signed-mirror execution path, not a proven DNS fix or root cause. Exact sanitized results are retained in `docs/evidence/ubuntu-signed-mirror-run.json`.
- Independent Anthropic / claude-opus-5-5 / requested high published-head review passed at `400903d` ([evidence](https://github.com/Dytschgo/arxcovo/pull/32#issuecomment-6802018766)). Earlier preparation/unactivated notes below are historical. This documentation-only follow-up requires its own exact-head recheck and green required checks before merge. No owner-machine APT/admin change occurred; no installed-service or dedicated-account M2 acceptance is established.

### Mirror mitigation review correction

- Independent Anthropic / claude-opus-5-5 / requested high review at `a97fd74` found B1: the actual Ubuntu image uses `mirror+file:/etc/apt/apt-mirrors.txt`, so the original direct-URI candidate would refuse unchanged. Its report is retained in `docs/reviews/ubuntu-mirror-mitigation-review.md`; nothing had been published or executed. Root cause of the DNS failure remains unknown.
- Codex / gpt-6.1-sol, medium added guarded support for that one exact mirror-list path, validating all entries and optional priorities before source replacement. A regression failed first. The 144-byte observed entry format now passes through the complete owned-temp plan/recheck/publication loop; the mirror list and third-party sources remain unchanged. Tests cover stale identity/bytes, atomic staging cleanup, mode requests, all-plan refusal, parent/file-count guards and an empty plan.
- The helper now refuses alternate insecure boolean spellings, tests the missing deb822 weak/downgrade options, recognizes lowercase URI fields, opens files nonblocking before type checks and prepares isolated root Python. Twenty portable tests pass locally. Eight restored source mutations now fail assertions: indirect-list validation, fresh bytes, fresh identity, mode preservation, file-count limit, parent policy, empty-plan refusal and rewritten-output bound. Root facts and fchmod are injected only in owned-temp tests; actual root APT publication/installation is still unrun. No owner-machine APT access or sudo ran. The revised candidate requires a fresh independent review before publication and does not establish a CI fix or M2 acceptance.

### Ubuntu signed-mirror mitigation candidate

- Codex / gpt-6.1-sol, medium retained the PR32 observation: job 130635247058, source `b475bf1`, Ubuntu 26.04 image `20261101.186.1`, three Ubuntu-domain timeouts and resolved Kernel Archives/GitHub/Microsoft controls. The existing Playwright dependency install still failed; root cause is unknown.
- Prepared an unactivated, fixed-context CI helper that changes only known Ubuntu APT URI values to the Kernel Archives HTTPS mirror. Suites/components/architectures/signing-key selection and third-party sources are preserved; no insecure option, new key, resolver edit or gate exemption is introduced. Both legacy and deb822 forms have byte-preservation tests and unsafe target sources refuse. All source files are validated before changes, bounded, no-follow, root-owned, single-link and not group/other writable.
- Eleven portable tests passed locally, including synthetic native file facts. No APT file was opened or modified and no privileged operation ran. Independent review is required before workflow activation; actual root publication, signed metadata/dependency installation and a green CI run remain unverified. This is a mitigation candidate, not a claimed fix or M2 acceptance.
- The exact workflow step is now prepared locally for review, before the unchanged Playwright installation. It forwards only four non-secret intent fields through sudo in the disposable Ubuntu runner. Four source mutations (foreign keyring acceptance, missing file-owner guard, hardlink acceptance and HTTP mirror) failed assertions and were restored. Nothing has been pushed or executed; no owner-machine sudo or APT operation ran.

### Ubuntu mirror DNS investigation preparation

- Codex / gpt-6.1-sol, medium added a read-only Linux CI diagnostic for three
  fixed Ubuntu mirror names and GitHub/Microsoft controls. Five getent children
  run concurrently, each with a five-second deadline, no shell and discarded
  stdout/stderr. Only fixed labels and statuses are emitted (at most 1024 bytes).
  No IPs, owner values, environment values or exception messages are retained.
- Diagnostics do not decide merge eligibility or skip any existing check. The
  workflow retains every previous step, platform, permission and timeout. There
  are no retries or resolver/network modifications. The cause is still unknown;
  this is evidence collection rather than a claimed DNS fix.
- Six pure Python tests passed on Windows. Three restored mutations (capturing
  output, removing the child deadline and reversing success classification)
  each failed assertions. YAML structural comparison confirms only the bounded
  diagnostic step was added. Native getent/network behavior on Ubuntu remains
  pending, along with independent review and hosted checks.

### Ubuntu DNS observations and next control

- Codex / gpt-6.1-sol, medium inspected Ubuntu job [130608817837](https://github.com/Dytschgo/arxcovo/actions/runs/43045667306/job/130608817837) at source `2cf5ab9`. Before Rust tests, all three Ubuntu mirror probes timed out; GitHub and Microsoft resolved. The later Playwright dependency step failed resolving those Ubuntu names. Windows and macOS jobs passed. Sanitized source-bound observations are retained in `docs/evidence/ubuntu-mirror-dns-observations.json`. This rules out an application-test DNS change in that run, but does not establish the resolver failure's cause.
- Added one fixed, read-only DNS control for `mirrors.edge.kernel.org`. The [Linux Kernel Archives mirror service](https://mirrors.edge.kernel.org/) lists its Ubuntu archive; this is a comparison target, not a selected package source or an availability guarantee. Six concurrent children retain the same five-second individual limit and 1024-byte output cap. No package source, resolver, signature check, install command or merge gate changes. The new control has not run on a hosted runner.
- Six pure diagnostic tests passed locally after pinning all six expected labels and hostnames. Independent review and hosted checks remain pending. No installed-service, dedicated-account or M2 acceptance is established.

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

- Publishing boundary: fixed service configuration and typed owner plans are a separate PR stacked on lifecycle #20. Native read-only observations were committed separately by Codex / gpt-6.1-sol, high; owner mutations and installed-service acceptance remain pending.
## Read-only native service adapter checkpoint

- Implementation sub-agent: Codex / gpt-6.1-sol, high. Prepared base
  `b63959b681e3cc1b106b0dfad25c20c972917488`, branch
  `t3/native-service-adapters`; implementation commit `678287d`.
  Added service/native.rs, native_windows.rs, native_unix.rs plus necessary
  module/Cargo wiring. No other task branch or development/store/MCP file edited.
  This task changes no acceptance status: M1 is accepted and M2–6 remain unchecked.
- Unchanged Windows baseline: `cargo test --workspace` passed 61 tests, one
  directly ignored subprocess fixture exercised by its parent. Added nine
  native/policy tests: real current-token/local-SAM observations (positive
  control when local SAM matches), current-user file refusal under SYSTEM
  private policy, actual writer/rename exclusion, hard-link refusal, metadata
  workspace checks, lexical negatives, bounded reads, protection negatives and
  rights/pin revalidation in both directions. No fixture claims SYSTEM/root.
- Six temporary source mutations each failed at an assertion in the intended
  focused test: removing SYSTEM owner, DACL presence, DACL protected, Unix root
  UID, Unix root GID, or dot-dot path refusal. All mutated sources restored;
  no policy relaxation was committed. These are guard-removal checks, not
  evidence of installed ACL ownership or privileged execution.
- Final Windows workspace result: 70 passed, one directly ignored fixture
  exercised by its parent; formatting and workspace/all-target Clippy with
  warnings denied pass. Artifact parser checks: three passed, two native
  systemd/plutil checks skipped on Windows. No UI changes or UI checks.
- Cross-target evidence is limited: full arxd Linux/macOS `cargo check` stopped
  at bundled SQLite because x86_64-linux-gnu-gcc / Darwin cc are unavailable.
  An uncommitted isolated manifest under target/native-check imports the ACTUAL
  layout.rs, config.rs and native.rs modules by path with libc/serde/serde_json
  and tempfile. `cargo check --tests` and `cargo clippy --tests` passed for
  x86_64-unknown-linux-gnu and aarch64-apple-darwin. Isolated Clippy uses
  `-A dead_code -D warnings` only because config::serialized has no owner-plan
  caller in that harness. This is Rust metadata/type-check evidence, not a
  linked arxd build, native framework link validation, or Unix test execution.
- Limits/blockers: modern macOS OpenDirectory/UID501 identity support refuses
  pending a reviewed local-native adapter. Linux uses conservative local
  passwd/NSS equality and bounded sudo/wheel groups, refusing system UID ranges,
  unknown filesystems and ACLs. Windows protected SYSTEM layout success,
  Linux/macOS runtime, native ACL checks on Unix and all installed service
  paths are **unverified on real hardware**. Exact race boundaries, supported
  APIs and pending obligations are in service-foundation.md. PR16's independently reviewed
  stable-path fix is documented, not copied into unrelated foreground modules.
- Production entry remains fail-closed. Application config/store/audit/endpoint
  integration and owner mutation adapters (including atomic publication and
  batch-right compensation) remain pending. Current-user development mode is
  independent. No owner admin steps, installation, account/right/task changes,
  service-manager mutation, push, PR, merge, settings, dispatch or watch occurred.
  Independent other-provider security review remains coordinator work.

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

## macOS local-account observation candidate

- Implementation sub-agent: Codex / gpt-6.1-sol, high, working alone from
  `0a16eb2c81014093126eefa77f0901b86a485867`. Added a fixed-local-node
  OpenDirectory resolver, conservative native Users/login-profile classification,
  unique name/UID/UUID checks, native nested admin membership and nonserialized
  incarnation revalidation. Linux conservative identity behavior and Windows
  policy are retained. No admin execution is enabled; the concurrent architecture
  correction remains separate. No ADR0009, service-install or development modules
  were edited. Last merged PR and milestone acceptance statuses are unchanged.
- ACL review found that acl_get_fd_np does not by itself distinguish absent ACL
  property from retrieval failure. The adapter now checks filesec presence after
  successful fstatx_np; absent/valid-empty pass observation, entries/errors refuse.
  Added macOS-only unprivileged identity and temporary-file ACL positive/error
  controls. User-owned files still refuse root private policy. Apple API/header
  references and exact supported profiles are in service-foundation.md.
- Executed on Windows: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, and
  `cargo test --workspace`: 71 passed (the original 70 plus incarnation test),
  one subprocess-only fixture directly ignored and exercised by its parent.
  `python scripts/test-service-artifacts.py -v`: three passed, two platform-native
  checks skipped. An isolated rustc harness imported the actual macOS pure policy
  source: three tests passed (classification negatives, result/error/count/ACL
  contracts, rename/replacement/membership revision changes).
- Eight restored source mutations were killed by assertions: skip local-node
  classification, accept mismatched native record type, ignore membership error,
  accept nonempty ACL, ignore array bound, accept duplicate records, skip config
  incarnation comparison. The first six used the isolated actual-policy harness;
  the incarnation mutation ran `cargo test -p arxd --lib service::config::revision_tests`.
  Final Apple source review also corrected filesec presence to accept any nonzero
  bitmask. Its eighth mutation (require exactly one) failed the new assertion.
  The first run of that eighth mutation harness failed to launch its relative
  executable path on Windows; the corrected absolute-path run passed baseline
  and killed the mutation.
  These demonstrate portable guard sensitivity, not native macOS execution.
- Isolated manifest under ignored target/native-check imports actual service
  layout/config/native sources: `cargo check --tests` and `cargo clippy --tests`
  for aarch64-apple-darwin and x86_64-unknown-linux-gnu passed. Isolated Clippy
  uses `-A dead_code -D warnings` because serialized has no owner-plan caller
  there. Workspace Clippy used no suppression. Metadata is not framework link,
  full cross-build or runtime proof. Full cross-builds were not rerun: the known
  Windows-host bundled SQLite cross-C-compiler limitation remains.
- Remaining: independent other-provider security review and hosted macOS native
  link/runtime checks after coordinator publication. Modern local-Mac support
  remains an unreviewed candidate; custom-shell, alias/multivalue, mobile and
  directory profiles refuse. Native APIs can block; bounded traversal does not
  supply a hard deadline. Installed-service identity/layout and M2 acceptance
  remain **unverified on real hardware** and open. No provisioning, elevation,
  owner steps, commands for identity lookup, push/PR/merge/watch, dispatch,
  settings or old Lab Manager access occurred.

## Integrated native service configuration checkpoint

- Codex / gpt-6.1-sol, medium: integrated current main, lifecycle review corrections, fixed configuration and native macOS observation candidate b261457. A strengthened regression first failed because revision lookup ran twice before macOS admin refusal; platform policy now runs before revision resolution and revalidation still refuses before observations or owner plans. Formatting, workspace/all-target Clippy and all 97 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. Native macOS/Linux and installed-service behavior remain unverified on real hardware. No provisioning or owner administration ran.


## Real Claude Code development acceptance

- Codex / gpt-6.1-sol, medium coordinated private acceptance after PR22 merged. Claude Code 2.1.322 was requested to use Opus 5.5 high through its existing login, with unrelated instructions, memory, hooks, connectors and built-in tools disabled; only the explicit arxcovo MCP server and named calls were allowed. Actual MCP events were inspected in memory, not a natural-language success report.
- A non-test arxd binary built from independently reviewed source 72a5e5f listed the host and executed literal cargo --version under the current unelevated Windows user. Production source equivalence to reviewed 59293a8 was checked. SQLite and MCP confirmed the caller's successful row, development identity mode, current-user match, Medium integrity and null raw owner identity/rights. A daemon restart and fresh Claude Code session returned the same row.
- The native executor reported confirmed Job cleanup. The harness stopped its owned processes, an independent native process query found zero remaining acceptance arxd instances, and temporary config/credential files were deleted. Only sanitized booleans, fixed labels, build/source hashes and outcomes are committed in docs/evidence/windows-development-acceptance.json. No raw CLI stream or private owner values were logged or committed.
- This is a passed development acceptance under the owner's explicit alternative, not M2 completion: dedicated-account/service execution, rights enforcement, service Application and native owner adapters remain unfinished. S4U/service and VM paths remain unverified on real hardware. No account, right, task, service, elevation or old Lab Manager action occurred.

## Native hosted validation checkpoint

- Codex / gpt-6.1-sol, medium inspected PR25 run43026547236 at7606c71: Ubuntu job130542582037 and macOS job130542582205 completed successfully; Windows was still running. These are native hosted CI results, not installed-layout or privileged service acceptance. PR24 merged the sanitized real current-user development evidence; dedicated-account and installed-service M2 acceptance remain open.

## Service lifecycle exact-head review integration

- Anthropic / claude-opus-5-5, high independently passed PR20 at1d41f0d: both prior blockers resolved; 75 runnable Windows tests and seven of eight valid scratch mutations passed their intended checks. The surviving owner-default mutation and other advisory findings remain recorded in the review. Codex / gpt-6.1-sol, medium merged docs-only main66c1b17 and corrected stale evidence wording. Production lifecycle source is unchanged. Fresh exact-head recheck and hosted gates are pending; no service installation or M2 acceptance is claimed.

## Service credential uniqueness correction

- Codex / gpt-6.1-sol, medium found service config allowed the same decoded token hash for distinct principals, unlike foreground Config. The new regression first failed because the duplicate was accepted. Service parsing now rejects duplicate hashes regardless of hex casing while retaining a positive distinct-credential control. Formatting, workspace/all-target Clippy with warnings denied and all 88 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. Independent review and hosted checks are pending. This enables no service or owner operation.

## M2 fixed service Application candidate

- Implementation sub-agent: Codex / gpt-6.1-sol, high, working alone on prepared
  branch t3/m2-service-application (initially t3/46c79625) from verified clean pinned base
  `7606c71b35ab906607c00efa96b99a76bca2409f`. Implemented native service
  Application composition, separate static/client validation and dynamic build
  preflight, protected pre-provisioned store with persistent journal, durable
  lifecycle audit, local authenticated inventory/activity endpoint and tracked
  bounded shutdown. Dedicated execution remains unavailable; no development
  executor fallback. Last merged PR and milestone acceptance remain unchanged.
- Windows checks executed: formatting check, workspace/all-target Clippy with
  warnings denied, full workspace tests: 106 passed, one direct-only fixture
  ignored and exercised by its passing parent. Artifact script: three passed,
  two native Unix checks skipped. Actual normal-user pipe/SDK and durable
  fixture-store evidence is in docs/service-foundation.md. No privileged
  service/runtime acceptance or hosted Linux/macOS evidence is claimed.
- Failure evidence: first Application pipe fixture used a dot in its generated
  name and correctly failed endpoint validation; corrected the fixture name.
  The new mutable-handle regression exposed metadata-only Windows handles
  allowing rename despite share flags; adding read-data access made writes
  succeed while rename/deletion refuse. Two test compile corrections addressed
  typed ActivityRow fields. No symptom reached three distinct failed fixes.
- Open integration: coordinator merges its case-insensitive token-hash
  uniqueness guard and parent docs/lifecycle refresh; this slice retains its
  assigned base and deliberately does not duplicate that guard. Independent
  other-provider review and native Unix hosted checks remain required. Owner
  adapter must prepare protected DB/journal/lock files; service refuses unsafe
  or missing prerequisites. Privileged service manager/layout/endpoint/SQLite
  paths are **unverified on real hardware**. Dedicated-account backend, scoped
  tool discovery and full M2 cargo/Claude Code acceptance remain open.
- No admin, provisioning, account/right/task/service/install/dispatch/settings,
  old Lab Manager/state/VM operation, push, PR, merge, watch or branch switch
  occurred. No new architecture deviation was introduced.

## Service Application coordinator integration

- Codex / gpt-6.1-sol, medium integrated candidate16d3788 with current native/config/lifecycle parents. Credential uniqueness is enforced in shared validate_inventory and tested through both parse and parse_inventory (duplicate/case variants refused, distinct credentials accepted). Formatting, workspace/all-target Clippy with warnings denied and all 107 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. The shared intermittent Windows hosted handle-test failure is separately under diagnosis; this local pass is not a claimed resolution. Fresh independent review and native hosted checks remain pending. No installation or privileged service acceptance occurred.

## Service Application Unix entry expectation

- Hosted Ubuntu job130551366884 at585082c reached the service entry regression and observed permission exit4 while its obsolete pre-Application fixture expected integration-unavailable exit7. Codex / gpt-6.1-sol, medium aligned the assertion with the real Application identity preflight and fixed service-permissions category; the fixture explicitly requires an unprivileged Unix runner before launching. Production refusal logic is unchanged. Fresh hosted verification remains pending.

## Windows CI handle-probe investigation

- Opus PR20 exact-head recheck passed d8f034f but identified a Windows failure shared by main66c1b17 and PR25/PR20: actual_job_tree_timeout_cancel_shutdown_overflow_and_handles sees child exit101. Production source is unchanged from earlier green runs. Codex / gpt-6.1-sol, medium ran the focused existing test twice locally; both passed, so no cause is claimed. The parent assertion now includes bounded child stderr as well as stdout for diagnosis. Dedicated high-effort diagnosis is pending; no gate is bypassed and no rerun is claimed as a fix.

- Scoped follow-up, Codex / gpt-6.1-sol / high: base 0de0b725ee976eaa0ac21a531fdd9ea6ab2131e9 (main66c1b173 plus diagnostic). Read the public PR20 addendum and all three exact failed Windows job logs. Locally the original cargo and nextest focused baselines passed once each. A temporary real-executor 64-slot sampler failed 1/64 children in each of two runs, surfacing GetHandleInformation return 1 versus 0 and child exit101. This proves the numeric-validity assumption is invalid locally; hosted logs still do not identify their child assertion.
- Fixed only cfg(test) fixture code: randomly named sentinel, snapshot candidate before opening reference, compare kernel object identity. Child-local bounded native allocation makes reuse deterministic; standard handle type checks, production CreateProcess/three-handle list, Job cleanup and token checks remain unchanged. Restoring numeric-validity checking fails the regression (1/1); temporarily adding the sentinel to the actual production inheritance list also fails (1/1, explicit inherited-object assertion). Both mutations restored. Fixed controlled regression passed directly and under nextest (2/2). Final unforced named-event sampling passed 64/64, with zero spontaneous reuse markers in that run; do not interpret it as proof of a hosted fix.
- Actual validation: cargo fmt --all --check; cargo clippy --workspace --all-targets --locked -- -D warnings; cargo nextest run --workspace --locked: **60/60 passed**, zero skipped, 7.369-second test phase. Original combined test also passed directly. Detailed loop, hypotheses, mutation results and limitations are in tools/windows-identity-probe/DEBUGGING.md. No architectural deviation. Last merged PR remains #24 / main66c1b173. Blockers: fresh hosted Windows result and independent other-provider review pending, coordinator integration required. No push/PR/merge/watch or privileged actions. Dedicated-account/service/VM paths remain **unverified on real hardware** by this fix.

## Windows handle regression merge integration

- Codex / gpt-6.1-sol, medium merged PR28 at reviewed460b617 with all five required gates green. Independent Opus review reproduced the old-test failure3/3, deliberate real inheritance failure3/3 (plus combinedtest2/2), and restored native executor tests passed20/20 repeatedruns. This fixes the reproduced test defect; the exact old hosted assertion was not retained and is not claimed proven. Service lifecycle integrates the correction; formatting, all-target Clippy with warnings denied and all 76 runnable Windows nextest tests passed (one direct-only fixture exercised by its parent). Fresh exact-head review and hosted gates remain pending. Native ancestor-policy blocker in25/27 remains open.

## Native ancestor-policy blocker reproduction

- Independent Opus stack review passed PR21 but failed PR25/27 because trusted ancestor validation rejects stock Windows system parents. Codex / gpt-6.1-sol, medium added stock_system_directory_passes_trusted_ancestor_validation on isolated fix/native-ancestor-policy: the real read-only System32 trusted-open path fails deterministically with Permissions under the existing policy. No permission or owner mutation occurred. The regression is intentionally red until the per-edge namespace-integrity policy is corrected; no fix, service start or M2 acceptance is claimed. Final private-object ownership/DACL checks must remain strict.


## PR25 Windows ancestor-policy implementation

- Task/provider/effort/outcome: Codex / gpt-6.1-sol, high, implementation sub-agent;
  fixed on prepared base `403742ec7136679e8fd3e457b53fa80f636a6efe` without delegation.
  Native ACE masks now distinguish existing-ancestor namespace control from
  sibling creation, directory metadata and inherit-only grants. Final binary
  and private-object policies remain strict. Architectural refinement and
  primary Microsoft references are in [ADR 0013](adr/0013-windows-existing-ancestor-namespace.md).
- Reproduction: the untouched base's exact requested `cargo test -p arxd --lib
  stock_system_directory_passes_trusted_ancestor_validation --locked -- --nocapture`
  failed with Permissions. The final namespace regression passes for System32,
  Program Files and ProgramData using read-only native opens. It now explicitly
  checks shared-directory ancestry rather than treating System32 as a final
  Arxcovo binary directory. Restoring the old ancestor call makes it fail again.
- Native Windows evidence: AccessCheck on in-memory SDDL descriptors confirms
  creation versus DELETE_CHILD/DELETE/WRITE_DAC/WRITE_OWNER and inherit-only
  semantics. Pure countercontrols cover individual rights, unknown ACEs, missing
  DACL, unsafe owner and final private inherit-only grants. Owned-temp junction
  and hardlink fixtures refuse. No OS-path ACL mutation was performed.
- Eleven deliberate guard/policy mutations were individually caught by assertion
  failures and restored: old ancestor call; missing DELETE_CHILD, WRITE_DAC,
  WRITE_OWNER or DELETE; removed ancestor owner check; accepted unknown ACE;
  discarded native masks; removed reparse or hardlink checks; foreign final
  private grants. Details and limitations are in ADR 0013.
- Checks actually run and passed on this Windows host: `cargo fmt --all -- --check`;
  `cargo clippy --workspace --all-targets --locked -- -D warnings`;
  `cargo test --workspace --locked` (102 passed, one direct-only fixture ignored
  and exercised by its subprocess parent); `python scripts/test-service-artifacts.py -v`
  (three passed, two OS-specific checks skipped); `git diff --check`.
  The focused service suite also passed (41 passed, one fixture ignored).
  No shared-handle flake occurred in this workspace run. PR28 files were not copied.
- Last merged PR remains #24 as recorded above. No push, PR, merge, watch,
  provisioning, owner admin operation, dispatch or settings change occurred.
  Independent other-provider review and hosted gates remain coordinator work.
- Limits: metadata-only retained ancestors in this base do not guarantee
  writer/delete exclusion. PR27's separate parent read-data change must survive
  later integration. No macOS stock-parent fix or runtime service/store/endpoint
  attestation is claimed. Installed service, dedicated account, SCM/LSA, owner
  adapters and M2 acceptance remain **unverified on real hardware**. The other
  stack findings remain outside this assignment. No old Lab Manager/VM access.

## Native ancestor correction coordinator integration

- Codex / gpt-6.1-sol, medium integrated candidate12081f2 with reviewed configuration parentaadfa05 and merged main28 handle regression. Renumbered the Windows namespace ADR to0013 to reserve0012 for the separate owner-recovery contract. Combined formatting, workspace/all-target Clippy with warnings denied and all103 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. Builder evidence reports eleven caught/restored mutations and native AccessCheck controls. Fresh exact-head independent review and hosted checks remain pending. PR27 sharing corrections must be preserved when the parent is integrated there; macOS ancestry and installed-service acceptance remain open.

## Service Application ancestor integration

- Codex / gpt-6.1-sol, medium integrated natived045cc4 into PR27, retaining Application FILE_READ_DATA ancestry pins and mutable-state write-sharing/no-delete-sharing behavior while adopting raw ACL masks and namespace authority checks. Formatting, workspace/all-target Clippy with warnings denied and all112 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. Fresh security review and hosted checks remain pending. Privileged workspace metadata and detailed preflight-reason findings remain open, as do macOS ancestry and installed-service acceptance.

## PR27 scoped preflight review corrections (2026-11-11)

- Provider/effort/outcome: Codex / gpt-6.1-sol / high, implementation child,
  worked alone on `t3/service-preflight-review-fixes`, prepared base
  `c7433438e2d618ca1e3fe010c88e68f03033d434`. Fixed the two assigned
  preflight findings only. No architecture deviation; ADR 0009's identity-switch
  boundary remains binding. Last merged PR is unchanged by this task (the latest
  coordinator merge entry above records PR28); no push/PR/merge/watch performed.
- Service inventory/startup/dynamic preflight never call workspace_safe; lexical
  disjointness remains enforced. Only owner-local parse/revalidate/configure-plan
  retains metadata validation. Poison/counting regression observes zero workspace
  calls for repeated valid service preflights and failed identity/rights cases,
  versus one on owner parse and another on configure-plan. Owner metadata refusal
  is also retained. macOS legacy admin preflight refuses before any observation.
- Added fixed response-only ExecutionUnavailable and exhaustive token/storedReason
  matches. list_hosts carries the observed detail; absent-executor run_command
  and discover_tools return execution_reason and finish refusal with the ADR's
  coarse schema-3 Reason. Windows account lookup captures ERROR_NONE_MAPPED as
  confirmed absence; access-denied, insufficient-buffer, no-domain and unknown
  errors remain generic. Group-lookup failures and local SAM policy refusal do
  not claim account absence. Unix/macOS resolver errors remain generic until
  those adapters supply explicit absence evidence. No native error text or
  private account/path values enter response details or stored metadata.
- Evidence: synthetic codes run through the production native lookup classifier
  with explicit absence and ambiguous-error controls; injected account models
  separately exercise missing/changed/rights failures. SDK duplex MCP table covers
  all 14 fixed reasons, both refusal tools, exact list_activity readback and SQL
  completion checks, schema version 3, no detail/os_code columns, database-byte
  absence of response-only tokens and private argument marker. This is not a
  claim that native API failure injection or real account deletion was exercised.
- Three deliberate mutations caught by assertion failures and restored: workspace
  observation in service preflight; ambiguous Windows codes reported as missing;
  account_missing persisted as unavailable instead of preflight_failed. The first
  mapping mutation command hit a CRLF rewrite compile error (not evidence); its
  byte-preserving rerun reached and failed the exact stored-reason assertion.
- Actual checks: cargo fmt --all -- --check; cargo clippy --workspace --all-targets
  --locked -- -D warnings; cargo nextest run --workspace --locked: **116/116 passed**,
  preserving all 112 previous passing tests plus four regressions; one direct-only
  fixture skipped by nextest and exercised by its subprocess parent. Test phase
  7.665 seconds. Focused service tests also passed (53 passed, one fixture ignored).
  Clippy initially caught a nested test if; collapsed it and Clippy passed.
- Dedicated execution and tool discovery remain unavailable, with no foreground
  fallback. Current service reason is a startup observation, not per-run execution
  acceptance. Workspace access after identity switch, native failure paths on
  installed hosts, SCM/LSA and Unix/macOS installed service behavior remain
  **unverified on real hardware**. M2 acceptance is still open. No ignored
  privileged harness, installation, account/right mutation, admin/elevation,
  dispatch, service/settings operation, old Lab Manager or VM access occurred.
  Existing normal-user fixture tests alone ran. Independent other-provider review
  and hosted required checks remain coordinator work. Mac endpoint/ancestry and
  SQLite crash-warning findings remain separate slices. ACL masks, namespace
  policy, ancestry read-data pins and sharing fixes are retained unchanged.

## Service preflight coordinator validation

- Codex / gpt-6.1-sol, medium integrated candidate51a1dee without changing its implementation. Formatting, workspace/all-target Clippy with warnings denied and all116 runnable Windows nextest tests passed; one direct-only fixture is exercised by its parent. Source inspection confirms service preflight omits workspace_safe, owner validation retains it, and only explicit native lookup not-found becomes account_missing. Stored schema3 reason vocabulary is unchanged. Fresh independent review and hosted checks remain pending.
- PR20 Ubuntu job130562335529 (run43032348219) failed browser dependency installation on DNS resolution of Ubuntu mirrors, after earlier checks. A single failed-job retry was requested at the unchanged reviewed4ead112. This is an environment retry, not a source-fix claim; Windows/macOS checks passed. No gate was bypassed.

## PR27 focused hot-journal persistence investigation (2026-11-11)

- Provider/effort/outcome: Codex / gpt-6.1-sol / high, implementation child,
  working alone on t3/m2-hot-journal-recovery from prepared base
  bb077bb45f061c4512d9b2976bed92b7530b2f37. Applied dytschgo-debug; built and ran
  the crash/reopen feedback loop before attempting a production fix. Last merged
  PR remains #28 as recorded above. No architecture contract/fence was relaxed.
- Reproduced reviewer hypothesis using actual child SQLite writes, a flushed
  unfinished transaction, abrupt termination and verified hot-journal magic/page
  records. Native Windows no-delete-sharing pins cover state directory ancestry,
  DB, journal and lock via the same open_impl as production; actual delete
  attempts fail. The exact reopen assertion failed with extended SQLite code
  2570 (SQLITE_IOERR_DELETE). This is native evidence, not a simulated error.
  The explicit cfg(test) service/current-owner policy and private temp ACL helper
  substitute for SYSTEM ownership/trusted installed ancestors only. No production
  runtime flag, fixed service path or privileged operation is involved.
- One failed fix: moving TRUNCATE ahead of page reads still produced 2570.
  Bundled SQLite marks journal_mode as needing schema, so that pragma itself
  triggers recovery. Temporary locking_mode=EXCLUSIVE before TRUNCATE preserves
  the protected journal during that recovery; return to NORMAL follows before
  size/schema/reconciliation work. Activity lock/pins stay live. No ownership,
  DACL, private ancestor, reparse/hardlink, FILE_READ_DATA, mutable write-sharing,
  no-delete-sharing, schema-3 or caller-privacy policy changed.
- Focused command actually run: cargo test -p arxd --lib
  pinned_service_recovers_actual_hot_journal --locked -- --nocapture. Original
  ordering red; pragma-only reorder red; final fix green. Deliberately removing
  only temporary exclusive mode again failed at the exact reopen assertion with
  2570; restored. No RerunUntilGreen or guard exception. Regression also verifies
  identity rollback, child-pending reconciliation, schema/integrity, persistent
  journal/modes, private caller readback, activity-lock exclusion and second reopen.
  Child stdin/stdout/stderr are null, readiness 10 s, lifetime 20 s, kill/wait RAII.
- Checks actually passed: cargo fmt --all -- --check;
  cargo clippy --workspace --all-targets --locked -- -D warnings;
  cargo nextest run --workspace --locked (**117/117 passed**, 7.787 s test phase;
  two subprocess-only helpers skipped directly and exercised by parents);
  python scripts/test-service-artifacts.py -v (three passed, two native checks
  skipped); git diff --check. Full prior 116-test Windows baseline retained.
- Detailed actual evidence and limitations: docs/service-foundation.md, PR27
  hot-journal section. Native Unix/macOS and actual installed SYSTEM/root recovery
  remain **unverified on real hardware**. Other-provider review and hosted gates
  remain coordinator work. Owner-provisioned protected files, installed SCM/LSA,
  systemd/launchd, dedicated-account execution/probe and real Claude Code M2
  acceptance remain open. No push/PR/merge/watch, dispatch/settings, installation,
  admin/elevation, account/right/task/service operation or old Lab Manager/VM
  access occurred. Mac endpoint and other backends are outside this slice.

## Protected SQLite recovery coordinator integration

- Codex / gpt-6.1-sol, medium integrated candidate41bfaa7 by fast-forward. The native normal-user hot-journal regression passed locally with the same Windows pin/sharing path; formatting passed. Builder full117 nextest/fmt/Clippy evidence is recorded above; coordinator did not repeat that entire unchanged suite. A verified hot journal previously caused SQLITE_IOERR_DELETE2570; temporary exclusive SQLite locking during recovery avoids deletion while retaining protected pins, and the mutation without it reproduces failure. This is process-crash evidence, not installed SYSTEM/root or power-loss acceptance. Fresh independent review and hosted checks remain pending.

## Hosted hot-journal fixture owner correction

- Codex / gpt-6.1-sol, medium inspected completed Windows job 130585845956
  from PR30 run 43039115633. It failed at the initial fixture owner check,
  before the crash/recovery path: 91 tests passed, one failed and 37 were not
  run after fail-fast. This is not evidence of a SQLite recovery failure.
- The parent Application fixture created private-DACL files without setting
  their owner. It now assigns the current test user's owner SID explicitly on
  its three newly created temporary files. The strict owner comparison and all
  production code remain unchanged. No token default-owner change is made.
- Formatting, workspace/all-target Clippy with warnings denied, and the focused
  native hot-journal regression passed locally. Hosted full-admin-token evidence
  is still pending; this correction does not establish installed SYSTEM/root
  or physical-power-loss recovery. Those paths remain **unverified on real hardware**.
## Service lifecycle refresh after merged CI unblock

- Codex / gpt-6.1-sol, medium refreshed #20 from reviewed head `4ead112` against actual main `3b6170b` (#32). The squash tree matches reviewed CI head `d6d3b31`. Only PROGRESS conflicted; both parents' provider tables and evidence were retained, Current state stays first, and last merged PR is #32. #31 is recorded closed as folded into #29.
- All service/crate, packaging, Cargo, toolchain and script bytes are unchanged from reviewed `4ead112`; CI workflow/helpers match merged main. Local formatting, locked workspace/all-target Clippy, **76/76 nextest tests** and **26/26 CI-helper tests** passed. The one direct-only subprocess fixture was skipped and exercised by its passing parent. No privileged service or owner operation ran. Fresh exact-head review and required hosted checks remain mandatory. Installed-service and dedicated-account M2 acceptance remain open.

## Configuration queue preparation

- Codex / gpt-6.1-sol, medium prepared #21 locally on refreshed lifecycle head `3195f5a` while #20 awaits normal gates. No new feature PR was opened and this preparation has not been pushed. Crate/configuration, packaging, Cargo/toolchain and script bytes match reviewed `aadfa05`; CI workflow/helpers match the refreshed parent. Both provider/evidence tables remain intact and Current state stays first.
- Local formatting, locked workspace/all-target Clippy, **89/89 nextest tests**, **26/26 CI-helper tests** and three portable artifact checks passed. One direct-only subprocess fixture was exercised by its parent; two native Unix artifact checks skipped. No owner/admin/service operation ran. Publication waits for the actual #20 merge commit and a final metadata refresh; current-head hosted checks and independent review remain required. No installed-service/M2 acceptance is claimed.

## Main CI recovery confirmation

- Codex / gpt-6.1-sol, medium verified the Actions API for main run 43066365961: source `3b6170bbd07e38873deecbd7750432b74fa4cb87`, attempt 1, completed success. Using the repository policy's `updated_at - run_started_at` measure gives **548 seconds (9m08s)**. Main CI is green and under 15 minutes after #32. No rerun, timeout increase or gate exception was used. This record is prepared locally for the next dependency checkpoint; #20 remains the first service-stack merge target.

## Configuration refresh after lifecycle merge

- OpenAI / gpt-6.1-sol / medium integrated the actual #20 squash `64b50e9` into #21 without rewriting published history. The squash tree is identical to reviewed head `3195f5a`; duplicate-history merge conflicts retained the already validated configuration tree. Configuration source, packaging and Cargo/toolchain files remain identical to reviewed `aadfa05`; CI helpers/workflows match merged main. Prior local verification therefore applies: formatting, locked all-target Clippy, 89/89 nextest and 26/26 CI-helper tests passed; artifact checks had three passes and two native Unix skips. No implementation changed in this refresh. Fresh exact-head independent review and required hosted checks remain necessary.
- #20 merged at 17:31:46 UTC after all five gates passed; its Windows job completed successfully at 17:30:23. Main push CI for the earlier #32 unblock passed in 548 seconds. The new #20 main push is not yet claimed green. No new feature PR was opened, and Current state remains the first section.

## Native validation queue preparation

- OpenAI / gpt-6.1-sol / medium prepared #25 locally on configuration head `9f74919` while #21 runs its fresh review and hosted gates. Published history is preserved. Only PROGRESS conflicted; the current state from the refreshed parent remains first, and both parent provider tables and task evidence are retained. Native source, packaging, Cargo/toolchain and scripts remain byte-identical to independently reviewed `d045cc4`; CI helpers/workflows match the configuration parent.
- This refresh ran formatting, 26/26 CI-helper tests and artifact checks (three passed, two native Unix skips). Clippy/nextest were not repeated because the Rust trees are identical to the reviewed head. No native service, owner, admin or account operation ran. No push or new feature PR; publication waits for #21 to merge, then a final refresh and exact-head independent review. Installed-service/dedicated-account M2 acceptance remains open and **unverified on real hardware**.
