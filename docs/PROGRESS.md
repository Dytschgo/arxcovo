# Arxcovo progress

## Current state

- Milestone: 1 — Setup (in progress).
- Current task: bootstrap public repository, workspace and CI/review gates.
- Last merged PR: none.
- Acceptance: no milestone acceptance checks have run yet.
- Milestone 1 implementation: Rust workspace, React/Vite and Electron scaffolds, locked dependencies, pinned tools, three-OS CI and fail-closed budget policy prepared on feat/milestone-1. Bootstrap main is not acceptance.
- Local evidence (Windows): cargo fmt --all --check, clippy workspace/all-targets with warnings denied, cargo test, cargo nextest (2 integration tests), npm lint, policy tests (3), UI production build and Playwright Chromium smoke (1) passed. Final verification rerun is recorded below before handoff.
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
- Pending: independent Opus review, three-OS hosted CI, protected branch/reviewer App setup, owner-agent allowlist and coordinator-only auto-merge automation, trivial PR acceptance. Proposed settings: docs/ci-administration.md.
- Coordinator steering applied: budget policy uses trusted default-branch checkout on pull_request_target/workflow_run, with API-only candidate measurements and completion re-evaluation. Gate architecture remains proposed pending independent review.

## Provider usage

| Task | Provider/model | Effort | Outcome |
| --- | --- | --- | --- |
| Bootstrap coordination | Codex / gpt-6.1-sol | medium | Empty workspace and local prerequisites inspected |
| Milestone 1 implementation | Codex / gpt-6.1-sol | medium | Scaffold and local checks prepared; hosted gates and independent review pending |
| Oversight | Claude / claude-opus-5-5 | medium | Thread launched; schedule result pending |

Exact allowances and per-task cost are not exposed; do not invent usage percentages.

