# Arxcovo progress

## Current state

- Milestone 1 complete: setup acceptance and its evidence PR #8 passed all required gates and merged. Milestone 2 is active in isolated unpublished worktrees. Milestones 3–6 remain unimplemented.
- Current task: authenticated MCP inventory/activity and the Windows execution feasibility probe. Electron and policy-test follow-ups remain open.
- Last merged PR: [#8](https://github.com/Dytschgo/arxcovo/pull/8), merge `3148241287c48178fefc5e13f1396cbc7ae1a757`, 2026-11-11 08:22:47 UTC. All five required gates passed; GitHub completed auto-merge before a redundant coordinator command could act.
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
- Live slow-main rejection, measured ci-speed recovery, fast main baseline and resumed normal auto-merging all passed. The slow and recovered main durations are retained in this PR's history. Setup acceptance is proven; its final evidence PR still needs independent review and merge. The second main baseline, run 42949562343 at 861db0b, succeeded in 241 seconds and is also recorded in history.

## Slow-main acceptance experiment

- PR #4 added a Linux-only 960-second delay after all real checks and temporarily raised the timeout to 18 minutes. It merged normally after independent review and green CI. No gate was disabled or bypassed.
- Main [run 42952698370](https://github.com/Dytschgo/arxcovo/actions/runs/42952698370), attempt 1 at 5c82988, succeeded from 03:25:47 to 03:42:43 UTC: **1016 seconds**. The Actions evaluator failed the probe's budget status at 03:43:02, naming this run and duration.
- Probe [#5](https://github.com/Dytschgo/arxcovo/pull/5), head 690bf5d, has no ci-speed label. Its three platform checks and independent Opus review passed; Ubuntu finished at 03:52:59 UTC. At 08:05:19 UTC the guarded command enabled auto-merge. GitHub still reported OPEN/BLOCKED with ci-time-budget as the sole failing required gate. This proves live rejection of an otherwise green PR.
- Recovery [#6](https://github.com/Dytschgo/arxcovo/pull/6), reviewed head 1da52c2, removed only the delay and restored the 12-minute timeout. Its attempt-1 [run 42953952528](https://github.com/Dytschgo/arxcovo/actions/runs/42953952528) succeeded from 03:37:12 to 03:41:47 UTC: **275 seconds**. The ci-speed budget exception passed against the slow main baseline. After recording the blocked probe, the coordinator invoked the guarded merge command; GitHub merged at 08:05:25 UTC.
- Recovery main [run 42984281226](https://github.com/Dytschgo/arxcovo/actions/runs/42984281226), attempt 1 at e692b20, succeeded from 08:05:27 to 08:09:38 UTC: **251 seconds**. The evaluator restored probe #5's budget success at 08:10:01. Probe head aaaf52c passed fresh independent review, final Windows CI passed at 08:11:52, and GitHub auto-merged at 08:12:14. No subsequent coordinator merge command was issued for the probe; normal merging resumed through all five gates.
- Electron tests are committed in [#7](https://github.com/Dytschgo/arxcovo/pull/7), reviewed head b8c2187. Local lint and the real Windows smoke test passed. Hosted run 42984639552 passed all three platforms, each running Electron once; independent Opus review passed. The branch needs updating after the newer main merges and re-review before merge. Auto-merge is off.

## Milestones

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
- Electron follow-up #7 verifies rendering, isolation preferences, blocked navigation and popup denial. Clearing ELECTRON_RUN_AS_NODE only in the launched child fixes the harness launch failure. Hosted Ubuntu/macOS/Windows tests passed on b8c2187; no sandbox bypass was added. VM and hypervisor paths remain unverified.
- Blanket minimatch override and duplicated tool pins remain lower-priority scaffold follow-ups. No new reviewer App or model API credential is required for the current flow.

## Oversight

- [Arxcovo babysitter](t3-thread://v2/950f5165-f55d-4ffd-9b15-4b87e10b4d15/mcp:launch:6ddd1ab43a55d1439cb40e2639d67190751dd74482d63b63ba999264bd36f7b0).
- Schedule `scheduled-task:91fbb832-3283-440c-8a0d-e4a89b2d19d0`, every three hours. Initial check-in passed; first scheduled run 2026-11-11 05:16:43 UTC. Agents have not cancelled or rescheduled it.
- Babysitter flagged the 03:42–08:03 stall after tool/provider failures and requested live recovery plus committed checkpoints. Resumed, verified the existing Electron commit before pushing/opening #7, proved #5 rejection, and merged #6 through all normal gates. All local WIP is committed on separate branches: docs/setup-acceptance-evidence, docs/early-hardware-and-raft-spikes (4481301), docs/security-reporting (f977b29), test/await-policy-failure (0c16a9e), plus published Electron/probe branches. The working tree is clean; these local documentation/test checkpoints still require review and merging. No protection was weakened.

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
| MCP inventory/activity tracer | Codex / gpt-6.1-sol | high | Isolated builder running; authenticated official SDK transport, inventory and persisted activity only; no command execution or acceptance claim |
| Windows execution feasibility probe | Codex / gpt-6.1-sol | high | Second isolated builder preparing disposable-CI-only identity/Job probe; no owner-PC privileged execution or account changes permitted |
| Awaited policy-failure regression | Codex / gpt-6.1-sol | medium | PR #9, originating commit 0c16a9e; 10 evaluator tests pass; independent review/merge pending |
| Single-host security contract | Claude / claude-opus-5-5 | high | Advisory design running while setup acceptance waits; no implementation or acceptance claimed |
| Electron verification follow-up | Codex / gpt-6.1-sol | medium | Local Windows test passed; not yet merged |
| PR #7 independent review | Claude / claude-opus-5-5 | high | Success on b8c2187; hosted Electron passed on three OSes, all five protection-removal mutations failed locally |
| Oversight | Claude / claude-opus-5-5 | medium | Initial check-in on track; recurring schedule active |

Exact provider allowances and per-task cost are not exposed; no percentages or costs are invented.
