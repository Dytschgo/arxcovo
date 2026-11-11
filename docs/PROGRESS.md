# Arxcovo progress

## Current state

- Milestone 1: setup acceptance in progress. Milestones 2–6 have not started.
- Current task: temporary slow-main budget acceptance experiment, followed by blocked-probe evidence and measured ci-speed recovery.
- Last merged PR: [#3](https://github.com/Dytschgo/arxcovo/pull/3), merge `e1f09bf1b7d47b09e55db6bbf98f882fd2ec9fb2`, 2026-11-11 03:06:17 UTC.
- [#1](https://github.com/Dytschgo/arxcovo/pull/1) installed the workspace/CI at `708afbd` after all three platform checks and an independent Opus review passed. Budget enforcement followed its first real main CI baseline.
- No milestone is declared complete. All VM/hypervisor paths remain **unverified on real hardware**.

## Setup evidence and remaining work

- Public AGPL-3.0 repository, Rust/npm workspaces, pinned tools/lockfiles, README/CONTRIBUTING/SECURITY and roadmap are merged.
- Hosted Windows, macOS and Ubuntu CI passed on both merged PRs. Local Rust formatting, Clippy with warnings denied and two nextest integration tests passed.
- First main CI: [run 42948900419](https://github.com/Dytschgo/arxcovo/actions/runs/42948900419), commit `708afbd`, attempt 1, success. Started 02:52:03 UTC and completed 02:55:12 UTC: **189 seconds**. The API-derived record is in docs/ci-duration-history.json.
- Eighteen policy/guard tests and one React component test pass locally. UI lint/build and Chromium smoke pass. Hosted CI repeats the relevant checks for each PR.
- Main protection readback requires `verify (ubuntu-latest)`, `verify (windows-latest)`, `verify (macos-latest)`, `ci-time-budget` and `agent-review`. CI/budget are pinned to Actions App 15368; reviewer status permits the authenticated external publisher. Strict mode and administrator enforcement are on, zero approving GitHub users are required, and force pushes/deletion are blocked.
- GitHub rejected `contexts: []` plus `checks` with HTTP422. The checks-only payload succeeded and is committed in #2. PR #2 subsequently merged with the pinned budget requirement active, proving the Actions-published status satisfies it.
- The owner selected `Dytschgo` for the trusted agent-author allowlist. The guarded command rejects outsiders, forks, stale heads, unready PRs and weakened protection. Repository auto-merge is enabled. PR #3 proves the trivial-PR acceptance: auto-merge enabled at 03:00:12 UTC before review/CI completion, independent Opus review passed at 03:02:08, final Windows CI passed at 03:04:03, and GitHub merged at 03:06:17 without a subsequent coordinator merge command. Reviewed head was f432908; all five required gates passed.
- Default Actions tokens are read-only, Actions PR approvals are disabled, and fork workflows require approval for all external contributors. Only the owner approves fork workflows and merges outsider PRs.
- Policy uses trusted default-branch workflow_run code, a label-change signal and periodic retry. It reads candidate measurements through the API, measures attempt 1, deduplicates statuses and isolates failing heads. History is a record, not an additional gate. API unavailability and the 100-open-PR bound remain documented limitations.
- Remaining: prove an over-15-minute main baseline blocks an otherwise green PR; prove measured ci-speed recovery; retain the resulting CI history. Do not claim these from unit tests. The second main baseline, run 42949562343 at 861db0b, succeeded in 241 seconds and is also recorded in history.

## Slow-main acceptance experiment

- The temporary fixture raises the verify-job timeout from 12 to 18 minutes and adds an unconditional 960-second delay on Ubuntu after the real checks. It remains part of the existing required Ubuntu check, so this PR cannot merge before that delay completes. Windows/macOS and all existing checks remain enabled.
- This fixture must pass actual CI and independent review under the normal gates. It intentionally makes one completed main run exceed 15 minutes to exercise the budget fence; it is temporary test work, not the target CI performance. No gate is disabled or bypassed.
- After the slow main run completes, open a harmless probe PR from that main revision, obtain green platform checks and independent review, and verify the budget failure prevents auto-merge.
- Remove the fixture and restore the 12-minute timeout in a ci-speed PR. Require its own successful measured CI reduction and independent review. After recovery and a fast main run, update the probe against current main, re-review its new SHA and verify normal merging resumes.
- Record actual run IDs, timestamps, statuses and merge outcomes. No slow-main or recovery acceptance has run yet. The separate local Electron CI patch is preserved for restoration after the experiment.

## Milestones

- [ ] 1. Setup: remaining end-to-end acceptance above.
- [ ] 2. Single host: per-OS service, inventory MCP, dedicated-account commands and persisted activity; Claude Code MCP acceptance.
- [ ] 3. Multiple hosts: join codes, CA/mTLS, Raft and remote commands; three-node failure and invalid-code acceptance.
- [ ] 4. Test VMs: QEMU/WHPX, unattended Windows/Linux templates, guest agent, snapshot/expiry/caps/reserves; real lifecycle acceptance.
- [ ] 5. Script vault: versioning, tools inventory and throwaway validation; broken-version rejection.
- [ ] 6. Dashboard: authenticated browser/Electron, hosts/VMs/activity/terminal/console; Playwright and PR screenshots.

## Owner actions and local follow-up

- Blockers: only the owner setup actions below are known; setup acceptance is still running.
- Read-only preflight: the dedicated local `Arxcovo` account is absent; QEMU is not on the current PATH. Owner must create the standard account before dedicated-account host acceptance and complete WHPX/QEMU/data caps before real VM acceptance. No agent performed administrative changes.
- docs/dev-host-setup.md is prepared locally for a separate documentation PR. It includes resource reserves, disk limits, official installation media and verification steps. Never touch the old Lab Manager installation, state or `agent-windows-test` VM.
- Local Electron follow-up (outside this documentation PR): a real Windows smoke test verifies rendering, isolation preferences, blocked navigation and popup denial. Clearing ELECTRON_RUN_AS_NODE only in the launched child fixes the harness launch failure. Hosted/Linux/macOS test results remain pending; no sandbox bypass was added.
- Blanket minimatch override and duplicated tool pins remain lower-priority scaffold follow-ups. No new reviewer App or model API credential is required for the current flow.

## Oversight

- [Arxcovo babysitter](t3-thread://v2/950f5165-f55d-4ffd-9b15-4b87e10b4d15/mcp:launch:6ddd1ab43a55d1439cb40e2639d67190751dd74482d63b63ba999264bd36f7b0).
- Schedule `scheduled-task:91fbb832-3283-440c-8a0d-e4a89b2d19d0`, every three hours. Initial check-in passed; first scheduled run 2026-11-11 05:16:43 UTC. Agents have not cancelled or rescheduled it.

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
| Electron verification follow-up | Codex / gpt-6.1-sol | medium | Local Windows test passed; not yet merged |
| Oversight | Claude / claude-opus-5-5 | medium | Initial check-in on track; recurring schedule active |

Exact provider allowances and per-task cost are not exposed; no percentages or costs are invented.
