# Arxcovo progress

## Current state

- Milestone: 1 — Setup (in progress).
- Current task: independent Opus review of PR #1 at `835d06cf6bd86a79645108dc0b940cb27e338ac9`; hosted CI and trusted-policy bootstrap activation pending.
- Coordinator follow-up: guarded auto-merge, reviewed protection payload proposal, trusted workflow_run triggers and tested evaluator added. Twelve policy/auto-merge tests and UI lint pass. No configuration applied or auto-merge action performed.
- Last merged PR: none.
- Acceptance: no milestone acceptance checks have run yet.
- Milestone 1 implementation: Rust workspace, React/Vite and Electron scaffolds, locked dependencies, pinned tools, three-OS CI and fail-closed budget policy prepared on feat/milestone-1. Bootstrap main is not acceptance.
- Local evidence (Windows): cargo fmt --all --check, clippy workspace/all-targets with warnings denied, cargo test, cargo nextest (2 integration tests), npm lint, policy tests (3), UI production build and Playwright Chromium smoke (1) passed. Final rerun before PR: fmt, clippy, npm ci, lint, policy tests, build and Chromium smoke all passed.
- Slow-main simulation: policy tests reject 901-second main for normal PRs, missing/unimproved/failed ci-speed measurements; successful measured reduction and 900-second boundary pass. Missing baseline rejects.
- npm audit after pinned minimatch override: zero vulnerabilities. ESLint 9 remains pinned; major upgrade dependency resolution failed twice, so upgrade deferred rather than forced.
- Coordinator WIP observation: nextest 0.9.151 installed and both scaffold tests passed; not independent final-head review.
- Hardware: all hardware paths unverified on real hardware.
- Repository started empty; local Rust 1.95.0 and Node 24.18.0 available.

## Oversight

- Babysitter: [Arxcovo babysitter](t3-thread://v2/950f5165-f55d-4ffd-9b15-4b87e10b4d15/mcp:launch:6ddd1ab43a55d1439cb40e2639d67190751dd74482d63b63ba999264bd36f7b0).
- Schedule ID: `scheduled-task:91fbb832-3283-440c-8a0d-e4a89b2d19d0`. Every three hours; next run `2026-11-11T05:16:43.715Z` (06:16 Europe/Zurich). Initial check-in completed on track.

## Milestone checklist

- [ ] 1. Setup — public AGPL repository, workspace, CI, budget and independent review gates; trivial PR auto-merges; simulated slow CI blocks.
- [ ] 2. Single host — per-OS service, host inventory MCP, dedicated-account commands, persistent activity feed; Claude MCP acceptance.
- [ ] 3. Multiple hosts — join codes, CA/mTLS, Raft, routed commands; three-node failure and invalid-code acceptance.
- [ ] 4. Test VMs — WHPX/QEMU, unattended templates, guest agent, snapshots, expiry/caps/reserves; Windows and Ubuntu lifecycle acceptance.
- [ ] 5. Script vault — versioning, inventory, throwaway validation; broken-version rejection acceptance.
- [ ] 6. Dashboard — login, browser/Electron, hosts/VMs/activity/terminal/console; Playwright and PR screenshots.

## Blockers and owner actions

- None confirmed yet. Administrative host configuration will require the owner before real VM acceptance; prepare the checklist first.
- Main has no CI baseline/workflow yet. Coordinator must resolve trusted-policy bootstrap activation; no gate bypass was performed. Duration history remains honestly empty.
- Pending: revised-head Opus review, hosted CI, reviewed installation merge, baseline history, protection/readback, trivial auto-merge and slow-main end-to-end acceptance. No new reviewer App is required.
- Owner selected Dytschgo as the current allowlisted agent author; their own same-repository PRs under this login are eligible too. Recorded in .github/agent-authors.json.
- Review round 1: Opus posted agent-review failure on 835d06c. Blocking findings: history became an unintended gate; untested evaluator paths; stale-success and label races; unnecessary approving-user/App requirements. Revised evaluator separates history, invalidates discovered heads before measurement, handles failures and reads live head/labels under workflow concurrency. Fresh review required.
- Coordinator steering applied: budget policy uses trusted default-branch checkout on pull_request_target/workflow_run, with API-only candidate measurements and completion re-evaluation. Gate architecture remains proposed pending independent review.

## Provider usage

| Task | Provider/model | Effort | Outcome |
| --- | --- | --- | --- |
| Bootstrap coordination | Codex / gpt-6.1-sol | medium | Empty workspace and local prerequisites inspected |
| Milestone 1 implementation | Codex / gpt-6.1-sol | medium | Scaffold and local checks prepared; hosted gates and independent review pending |
| Oversight | Claude / claude-opus-5-5 | medium | Initial check-in completed on track; three-hour schedule active |
| CI gate architecture | Claude / claude-opus-5-5 | high | Completed; app-free trusted workflow_run design and reviewed installation sequence proposed |
| PR #1 independent review | Claude / claude-opus-5-5 | high | Completed on `835d06c`; changes requested, failure status posted |
| Review corrections and merge command | Codex / gpt-6.1-sol | medium | Twelve local tests pass; revised-head review pending |

Exact allowances and per-task cost are not exposed; do not invent usage percentages.


## Implementation handoff

- Public repository created; bootstrap base `176af7a` intentionally includes coordinator-authored AGENTS.md, KICKOFF.md and PROGRESS.md.
- PR: https://github.com/Dytschgo/arxcovo/pull/1 (feat/milestone-1 to main). Nothing merged; last merged PR remains none.
- Required next actions: independent Opus review, coordinator reconciliation of trusted-policy activation/baseline, branch protection and separately authenticated agent-review integration, then trivial PR through all gates.
- Hosted CI pending at handoff. Electron runtime and VM/hypervisor paths **unverified on real hardware**. No Lab Manager, host admin, babysitter or schedule changes performed.
