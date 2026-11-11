# Arxcovo progress

## Current state

- Milestone 1 complete: setup acceptance and evidence PR #8 passed all required gates and merged. Milestone 2 is active in published PRs and isolated correction worktrees. Milestones 3-6 remain unimplemented.
- Current task: correct Unix validation PR #14 review blockers, diagnose the first Windows experiment failure, and independently review local MCP/stdio PR #15 including its hosted initialization failures. Dedicated-account command acceptance remains unverified.
- Last merged PR: [#12](https://github.com/Dytschgo/arxcovo/pull/12), merge `03adb434c5f8ff5d5fb50303174ad95367fd8761`, 2026-11-11 09:26:11 UTC. All required checks and exact-head independent review passed. The subsequent manual Windows experiment failed; its dispatch gate is cleared.
- [#1](https://github.com/Dytschgo/arxcovo/pull/1) installed the workspace/CI at `708afbd` after all three platform checks and an independent Opus review passed. Budget enforcement followed its first real main CI baseline.
- Only milestone 1 is complete. All VM/hypervisor paths remain **unverified on real hardware**.

## Setup evidence and remaining work

- Public AGPL-3.0 repository, Rust/npm workspaces, pinned tools/lockfiles, README/CONTRIBUTING/SECURITY and roadmap are merged.
- Hosted Windows, macOS and Ubuntu CI passed on every merged PR. Local Rust formatting, Clippy with warnings denied and two nextest integration tests passed.
- First main CI: [run 42948900419](https://github.com/Dytschgo/arxcovo/actions/runs/42948900419), commit `708afbd`, attempt 1, success. Started 02:52:03 UTC and completed 02:55:12 UTC: **189 seconds**. The API-derived record is in docs/ci-duration-history.json.
- Eighteen policy/guard tests and one React component test pass locally. UI lint/build and Chromium smoke pass. Hosted CI repeats the relevant checks for each PR.
- Main protection readback requires `verify (ubuntu-latest)`, `verify (windows-latest)`, `verify (macos-latest)`, `ci-time-budget` and `agent-review`. CI/budget are pinned to Actions App 15368; reviewer status permits the authenticated external publisher. Strict mode and administrator enforcement are on, zero approving GitHub users are required, and force pushes/deletion are blocked.
- GitHub rejected `contexts: []` plus `checks` with HTTP422. The checks-only payload succeeded and is committed in #2. PR #2 subsequently merged with the pinned budget requirement active, proving the Actions-published status satisfies it.
- The owner selected `Dytschgo` for the trusted agent-author allowlist. The guarded command rejects outsiders, forks, stale heads, unready PRs and weakened protection. Repository auto-merge is enabled. PR #3 proves the trivial-PR acceptance: auto-merge enabled at 03:00:12 UTC before review/CI completion, independent Opus review passed at 03:02:08, final Windows CI passed at 03:04:03, and GitHub merged at 03:06:17 without a subsequent coordinator merge command. Reviewed head was f432908; all five required gates passed.
- Default Actions tokens are read-only, Actions PR approvals are disabled, and fork workflows require approval for all external contributors. Only the owner approves fork workflows and merges outsider PRs.
- Policy uses trusted default-branch workflow_run code, a label-change signal and periodic retry. It reads candidate measurements through the API, measures attempt 1, deduplicates statuses and isolates failing heads. History is a record, not an additional gate. API unavailability and the 100-open-PR bound remain documented limitations.
- Live slow-main rejection, measured ci-speed recovery, fast main baseline and resumed normal auto-merging all passed. Final evidence PR #8 merged at 3148241; milestone 1 is complete. API-derived attempt-1 durations are retained in docs/ci-duration-history.json.

## Slow-main acceptance experiment

- PR #4 added a Linux-only 960-second delay after all real checks and temporarily raised the timeout to 18 minutes. It merged normally after independent review and green CI. No gate was disabled or bypassed.
- Main [run 42952698370](https://github.com/Dytschgo/arxcovo/actions/runs/42952698370), attempt 1 at 5c82988, succeeded from 03:25:47 to 03:42:43 UTC: **1016 seconds**. The Actions evaluator failed the probe's budget status at 03:43:02, naming this run and duration.
- Probe [#5](https://github.com/Dytschgo/arxcovo/pull/5), head 690bf5d, has no ci-speed label. Its three platform checks and independent Opus review passed; Ubuntu finished at 03:52:59 UTC. At 08:05:19 UTC the guarded command enabled auto-merge. GitHub still reported OPEN/BLOCKED with ci-time-budget as the sole failing required gate. This proves live rejection of an otherwise green PR.
- Recovery [#6](https://github.com/Dytschgo/arxcovo/pull/6), reviewed head 1da52c2, removed only the delay and restored the 12-minute timeout. Its attempt-1 [run 42953952528](https://github.com/Dytschgo/arxcovo/actions/runs/42953952528) succeeded from 03:37:12 to 03:41:47 UTC: **275 seconds**. The ci-speed budget exception passed against the slow main baseline. After recording the blocked probe, the coordinator invoked the guarded merge command; GitHub merged at 08:05:25 UTC.
- Recovery main [run 42984281226](https://github.com/Dytschgo/arxcovo/actions/runs/42984281226), attempt 1 at e692b20, succeeded from 08:05:27 to 08:09:38 UTC: **251 seconds**. The evaluator restored probe #5's budget success at 08:10:01. Probe head aaaf52c passed fresh independent review, final Windows CI passed at 08:11:52, and GitHub auto-merged at 08:12:14. No subsequent coordinator merge command was issued for the probe; normal merging resumed through all five gates.
- Electron tests merged in [#7](https://github.com/Dytschgo/arxcovo/pull/7) as 9c62344 after review of final head 8b899ba. Hosted run 42986780669 passed all three platforms, each running Electron once. Independent mutation tests removed each of five protections and observed failure. This is software verification, with VM/hypervisor paths still unverified.

## Milestones

### Milestone 2 delivery status

- Inventory/activity: PR #11 merged the authenticated official-SDK HTTP tracer after all three hosted platforms and security re-review passed. It deliberately has no execution authority. Local and independent workspace validation passed 13 tests.
- The macOS temporary-root fixture issue and blocking Windows inherit-only ACL exposure were fixed before merge. The permission regression failed old code and passed the correction. Windows path-prefix handling, owner checks, header/connection bounds and other non-blocking review observations remain tracked follow-ups before privileged service use.
- Windows execution: PR #12 merged after four blocker corrections, the deny-only regression and final documentation review. Unprivileged self-test: 34 assertions; helper validation: 114 assertions; five guard mutations detected. First manual experiment [run 42995659949](https://github.com/Dytschgo/arxcovo/actions/runs/42995659949) at reviewed main 03adb434 failed on 2026-11-11: setup/build/self-tests passed, experiment emitted ci-experiment ok:false and cleanup ok:true, without identifying the failed setup phase. Cause remains unknown. WINDOWS_IDENTITY_PROBE_REVIEWED_SHA was deleted after completion, and a later variable-list readback was empty. Bounded diagnostics are being prepared before any new reviewed dispatch. No S4U or dedicated-account acceptance is claimed.
- Unix execution: [PR #14](https://github.com/Dytschgo/arxcovo/pull/14), head c508de0, contains the library and manual disposable validation tooling. All three hosted platform checks passed, with the privileged runtime test skipped. Independent review failed with three tooling blockers: overbroad global cgroup ownership checks, an inconclusive symlink-cwd negative, and missing cleanup/evidence guard tests. Corrections are assigned in an isolated worktree; no Linux privileged experiment has run. Restart cleanup and audited caller integration remain unfinished.
- Local transport: [PR #15](https://github.com/Dytschgo/arxcovo/pull/15), head 6f55eb2, adds opt-in local IPC and an authenticated bounded stdio bridge. Hosted run 42996013708 passed Ubuntu but failed daemon/stdio initialization on Windows and macOS. The coordinator reran the focused test and all 24 workspace tests locally on Windows successfully; that does not resolve the hosted failures. Independent security/failure review is active. Services, cross-user provisioning and execution are not implemented by this slice.
- Still to implement and prove: per-OS service integration; protected service/build-account separation; actual dedicated-account command execution and rights enforcement; account-scoped tool discovery; restart cleanup; bounded command-tree cancellation/output; persisted execution identity/outcome and activity access.
- Final acceptance remains Claude Code through real MCP listing the host and running cargo --version as the dedicated account, with the action visible in persisted activity. The missing owner-created account is a real acceptance prerequisite, not permission to fall back to the owner or daemon identity.

- [x] 1. Setup: end-to-end acceptance passed; evidence reviewed and merged in #8.
- [ ] 2. Single host: per-OS service, inventory MCP, dedicated-account commands and persisted activity; Claude Code MCP acceptance.
- [ ] 3. Multiple hosts: join codes, CA/mTLS, Raft and remote commands; three-node failure and invalid-code acceptance.
- [ ] 4. Test VMs: QEMU/WHPX, unattended Windows/Linux templates, guest agent, snapshot/expiry/caps/reserves; real lifecycle acceptance.
- [ ] 5. Script vault: versioning, tools inventory and throwaway validation; broken-version rejection.
- [ ] 6. Dashboard: authenticated browser/Electron, hosts/VMs/activity/terminal/console; Playwright and PR screenshots.

## Owner actions and local follow-up

- Blockers: dedicated-account and hardware acceptance require the owner setup below. No remaining setup acceptance blocker.
- Read-only preflight: the dedicated local `Arxcovo` account is absent; QEMU is not on the current PATH. Owner must create the standard account before dedicated-account host acceptance and complete WHPX/QEMU/data caps before real VM acceptance. No agent performed administrative changes.
- docs/dev-host-setup.md is prepared locally for a separate documentation PR. It includes resource reserves, disk limits, official installation media and verification steps. Never touch the old Lab Manager installation, state or `agent-windows-test` VM.
- Electron follow-up #7 is merged. Rendering, isolation preferences, blocked navigation and popup denial passed hosted Ubuntu/macOS/Windows checks. ELECTRON_RUN_AS_NODE is cleared only in the launched child; no sandbox bypass was added.
- Blanket minimatch override and duplicated tool pins remain lower-priority scaffold follow-ups. No new reviewer App or model API credential is required for the current flow.

## Oversight

- [Arxcovo babysitter](t3-thread://v2/950f5165-f55d-4ffd-9b15-4b87e10b4d15/mcp:launch:6ddd1ab43a55d1439cb40e2639d67190751dd74482d63b63ba999264bd36f7b0).
- Schedule `scheduled-task:91fbb832-3283-440c-8a0d-e4a89b2d19d0`, every three hours. Initial check-in passed; first scheduled run 2026-11-11 05:16:43 UTC. Agents have not cancelled or rescheduled it.
- Babysitter flagged the 03:42–08:03 stall after tool/provider failures and requested live recovery plus committed checkpoints. Resumed, verified the existing Electron commit before pushing/opening #7, proved #5 rejection, and merged #6 through all normal gates. All local WIP is committed on separate branches: docs/setup-acceptance-evidence, docs/early-hardware-and-raft-spikes (4481301), docs/security-reporting (f977b29), test/await-policy-failure (0c16a9e), plus published Electron/probe branches. Those checkpoints remain preserved; outstanding docs/test PRs #9 and #10 still require review and merging. No protection was weakened.

## Provider usage

| Task | Provider/model | Effort | Outcome |
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
| Awaited policy-failure regression | Codex / gpt-6.1-sol | medium | PR #9, originating commit 0c16a9e; 10 evaluator tests pass; independent review/merge pending |
| Single-host security contract | Claude / claude-opus-5-5 | high | Advisory design prepared; implementation and M2 acceptance remain incomplete |
| Electron verification follow-up | Codex / gpt-6.1-sol | medium | Local and hosted three-OS verification passed; merged in #7 |
| PR #7 independent review | Claude / claude-opus-5-5 | high | Success on b8c2187; hosted Electron passed on three OSes, all five protection-removal mutations failed locally |
| Oversight | Claude / claude-opus-5-5 | medium | Initial check-in on track; recurring schedule active |
| PR #12 full corrected security review | Claude / claude-opus-5-5 | high | Success on 9fbbb76; N1 requested before dispatch, subsequently corrected |
| PR #12 final evidence review | Claude / claude-opus-5-5 | high | Success on 42d1165; all required checks passed, merged as 03adb434 |
| First Windows disposable experiment | Codex / gpt-6.1-sol | medium | Run 42995659949 failed with generic setup error; cleanup reported true; dispatch variable cleared |
| Windows experiment diagnosis | Codex / gpt-6.1-sol | high | Isolated implementation active; bounded diagnostics required before another reviewed run |
| PR #14 security review | Claude / claude-opus-5-5 | high | Failure on c508de0: three disposable tooling/test blockers; Rust library had no blocking finding |
| PR #14 corrections | Codex / gpt-6.1-sol | high | Isolated implementation active; no privileged run or acceptance claimed |
| Local MCP/stdio transport | Codex / gpt-6.1-sol | high | Candidate integrated in #15; 24 local Windows tests pass, hosted Windows/macOS initialization fails |
| PR #15 independent security/failure review | Claude / claude-opus-5-5 | high | Active on pinned 6f55eb2; verdict pending |

Exact provider allowances and per-task cost are not exposed; no percentages or costs are invented.

## Windows setup diagnostic correction

- PR #17 adds fixed setup-phase/numeric HRESULT/category diagnostics, privacy tests against the production catch, and case-insensitive matching at the exact trusted PowerShell executable location. Existing absolute-path/reparse checks remain. The casing defect was reproduced unprivileged; the first hosted run's actual cause remains unknown.
- Builder evidence: 115 helper assertions, 73 setup diagnostic assertions, 11 killed mutations, Release build and 34 native self-test assertions passed. Provider: Codex / gpt-6.1-sol, high effort. Independent review and hosted checks remain required before another dispatch. The reviewed-SHA variable remains unset; no new privileged run was performed.

## PR #17 preflight review correction

- Review on ed20774 failed because the new preflight catch lacked a fail-closed regression. The test now extracts only that production preflight block into a child PowerShell with a deliberately invalid context, requiring exit 1, two fixed rows and no post-preflight marker. No provisioning statements are passed to that child.
- Diagnostics unwrap one MethodInvocationException inner type/code, tested through a real failing static file read. Exact phase ordering and the task-path marker placement are checked. Separate path-layer tests require the original exact spelling and the reparse-check call. Current local helper suite: 115 assertions; setup suite: 90 assertions. Mutation and hosted results are recorded after execution; no privileged rerun has occurred. Provider: Codex / gpt-6.1-sol, medium effort; fresh independent review is required.

- Coordinator verification of this correction: all 115 helper assertions and 90 setup assertions passed; all 17 guard mutations were killed, including preflight exit/diagnostic removal, phase ordering, wrapped errors, reparse-call removal and unsafe normalization. No native code changed or privileged operation ran.
