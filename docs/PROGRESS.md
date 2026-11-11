# Arxcovo progress

## Current state

- Milestone: 1 — Setup (in progress).
- Current task: merge-policy follow-up, first main CI baseline and complete gate activation; then end-to-end setup acceptance.
- Coordinator follow-up: guarded auto-merge, reviewed protection payload proposal, trusted workflow_run triggers and tested evaluator added. Twelve policy/auto-merge tests and UI lint pass. No configuration applied or auto-merge action performed.
- Last merged PR: #1, installation merge `708afbd3f7a5a262a0f21ca9cb11bce991c41aff` at 2026-11-11 02:52 UTC. All three platform checks and independent Opus review passed at reviewed head `5b07ea1`. Budget was not yet installed; this is installation, not setup acceptance.
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

- Installation protection: coordinator applied the independently reviewed CI/review requirements as bootstrap controls (all three platform checks pinned to Actions and agent-review, strict/up-to-date, administrators included, zero approving users, force push/deletion blocked). Budget becomes required after its first real main baseline. GitHub rejected `contexts: []` together with `checks` (422, ambiguous schema); removing `contexts` succeeded. Final payload correction is prepared locally. Full gate acceptance remains pending.
- Read-only host preflight confirmed the dedicated local `Arxcovo` account is absent and QEMU is not available on the current PATH. Owner must create the standard account before dedicated-account host acceptance, and complete QEMU/WHPX/data-cap setup before real VM acceptance. Follow docs/dev-host-setup.md. No account, feature, installation or other admin change was made by agents.
- Repository settings readback: default Actions token permission is `read`; Actions PR approval is disabled. Coordinator changed fork workflow approval from first-time contributors to `all_external_contributors` and verified the API readback. This matches the independently advised setup. Main protection and repository auto-merge remain unapplied.
- None confirmed yet. Administrative host configuration will require the owner before real VM acceptance; prepare the checklist first.
- Main has no CI baseline/workflow yet. Coordinator must resolve trusted-policy bootstrap activation; no gate bypass was performed. Duration history remains honestly empty.
- Pending: revised-head Opus review, hosted CI, reviewed installation merge, baseline history, protection/readback, trivial auto-merge and slow-main end-to-end acceptance. No new reviewer App is required.
- Owner selected Dytschgo as the current allowlisted agent author; their own same-repository PRs under this login are eligible too. Recorded in .github/agent-authors.json.
- Review round 1: Opus posted agent-review failure on 835d06c. Blocking findings: history became an unintended gate; untested evaluator paths; stale-success and label races; unnecessary approving-user/App requirements. Revised evaluator separates history, invalidates discovered heads before measurement, handles failures and reads live head/labels under workflow concurrency. Fresh review required.
- Review round 2: Opus posted failure on 0218ba8 for repeated status exhaustion and cross-PR failure propagation. Follow-up publication is idempotent, only old successes with changed main measurements become pending, and capped heads are isolated. A 1,100-evaluation regression test emits only two statuses; another proves a capped head cannot strand a fresh PR. Fourteen policy/auto-merge tests plus one React component test, UI lint/build and Chromium smoke pass locally.
- Review round 3: Opus posted success on 5b07ea1, with Windows/macOS/Ubuntu CI green before merge. Non-blocking event fallback issue corrected in follow-up: only discovery failures publish the triggering-head fallback, and publication is awaited. Regression tests preserve a good PR's success when another PR errors. Additional guard tests cover unready PRs and weakened protection. Electron smoke and owner checklist remain separate local follow-up work.
- Owner setup checklist prepared locally as docs/dev-host-setup.md; no admin changes performed. It is a follow-up document outside the bootstrap review target.
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
