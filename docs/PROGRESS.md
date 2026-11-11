# Arxcovo progress

## Current state

- Milestone 1 complete: setup acceptance and its evidence PR #8 passed all required gates and merged. Milestone 2 is active; local transport PR #15 is under review correction. Milestones 3–6 remain unimplemented.
- Current local transport task: correct PR #15 after its first independent Opus review; local branch `t3/fix-pr15-client-isolation` starts at `6f55eb23c33c83477b9ffa534c1a763c856a5f8c`. Fresh review and hosted checks are pending. Dedicated-account command acceptance remains unverified. Other execution/service tasks remain separately owned.
- Last merged PR: [#12](https://github.com/Dytschgo/arxcovo/pull/12), merge `03adb434c5f8ff5d5fb50303174ad95367fd8761`, 2026-11-11 09:26:11 UTC. All required checks and exact-head independent reviews passed. Its manual experiment is running at that reviewed SHA; the repository dispatch gate must be cleared after completion.
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

### Milestone 2 delivery status

- Inventory/activity: PR #11 merged the authenticated official-SDK HTTP tracer after all three hosted platforms and security re-review passed. It deliberately has no execution authority. Local and independent workspace validation passed 13 tests.
- The macOS temporary-root fixture issue and blocking Windows inherit-only ACL exposure were fixed before merge. The permission regression failed old code and passed the correction. Windows path-prefix handling, owner checks, header/connection bounds and other non-blocking review observations remain tracked follow-ups before privileged service use.
- Windows execution: PR #12 includes the four blocker corrections and the pre-dispatch N1 regression. Ordinary child reproduction showed handle mask 7 before and 0 after detached creation; raw TokenGroups rejected the existing filtered admin's deny-only membership. Current local self-test passes 34 assertions; filtering deny-only groups now fails it. The earlier 114 helper assertions and five guard mutations passed independently. Current-head review and hosted checks remain required. No privileged experiment has run and no S4U/cross-account/task/cleanup success is claimed.
- Unix execution candidate 46cba1f is integrated for review as a library only, without an MCP or CLI execution entry point. Builder formatting/Clippy/19 Windows tests and standalone Unix metadata checks passed; coordinator formatting and 19 tests passed before updating against merged host identity. Linux/macOS runtime, privileged harness, restart cleanup and audit integration remain unverified. Persistent identity merged in PR #13 after its atomicity test and legacy rollback were independently checked.
- Still to implement and prove: stdio bridge and trusted local endpoint identity; per-OS service integration; protected service/build-account separation; actual dedicated-account command execution and rights enforcement; account-scoped tool discovery; bounded command-tree cancellation/output; persisted execution identity/outcome and activity access.
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

Exact provider allowances and per-task cost are not exposed; no percentages or costs are invented.

### Local transport PR #15 correction (2026-11-11)

- Provider/effort/outcome: Codex / GPT-6.1 Sol / high, implementation sub-agent,
  no delegation. Base `6f55eb23c33c83477b9ffa534c1a763c856a5f8c`, local branch
  `t3/fix-pr15-client-isolation`. PR [#15](https://github.com/Dytschgo/arxcovo/pull/15)
  remains blocked by its [first independent Opus review](https://github.com/Dytschgo/arxcovo/pull/15#issuecomment-6792699854)
  and hosted Windows/macOS initialization failures on run 42996013708. This local
  correction code commit is `c744d73944c3b260ddc93cc6569b99ca7b5d1b00`;
  it was not pushed and no remote state was changed. Last merged PR is
  unchanged in Current state above; historical 24/24 results were local Windows
  only, not evidence of hosted acceptance.
- Corrected per-peer disconnect/credential-query rejection versus unrecoverable
  listener errors, paced rejected/busy admission, bounded Windows instance retries
  and busy connects. Malformed clients cannot terminate the listener; failed
  credential queries never admit a peer. Listener reservations remain exclusive.
- Strict production TokenUser ownership retained. Fixtures explicitly assign
  TokenUser ownership to private parents/client files and derive identities from
  TokenUser independently. Positive config-load control precedes negative cases.
  ADR0008 documents standard/filtered, full/elevated and SYSTEM applicability and
  the remaining independent security decision; group owners are not broadly trusted.
- Safe fixed diagnostics distinguish client config, connect/identity, auth and
  daemon config/state/bind/listener stages. Test subprocess stderr reads cap at
  512 bytes/2 seconds, report read failures/timeouts and only expose known categories.
  Binary readiness/SDK failure paths retain daemon and bridge status/context.
- Restored graceful HTTP drain with a real in-flight response regression; main
  bounds drain to five seconds. Buffered frame forwarding retains the 8192-byte
  cap in both directions, withholding oversized/partial frames. Eight slots,
  five-second auth/initialize/write limits and 300-second partial-frame limits
  replace the arbitrary total lease: initialized clients may idle indefinitely.
  Same-user slot occupancy is an explicit accepted limitation; oversized inventory
  responses terminate only that connection and require pagination before growth.
- Local Windows validation: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --locked -- -D warnings`,
  `cargo nextest run --workspace --locked --no-fail-fast`: **28/28 passed**
  (27 arxd + 1 guest CLI). Added actual connect/close before accept followed by
  malformed auth and real subprocess SDK initialization; deterministic injected
  credential-query rejection followed by SDK initialization; fatal-listener no-spin;
  virtual-time idle survival/partial-frame expiry; HTTP in-flight shutdown drain.
- Red/green mutations (each restored; all exit 101 with assertion failures):
  treating rejected peers as fatal fails the injected-seam initialization test;
  inverting current TokenUser config ownership fails the fixture positive control;
  bypassing expected server identity fails the zero-byte trap (credential bytes
  observed). Windows trap explicitly uses TokenUser pipe owner and wrong expected
  `S-1-5-32-544`, reproducing the reviewer's BuiltinAdmins mismatch.
- During development the new unit fixture initially lacked a private DACL, and
  the early-disconnect test hit ERROR_PIPE_BUSY before replenishment. The fixture
  now has a private DACL and busy retry is explicitly bounded. These were observed
  Windows failures; the macOS LOCAL_PEEREPID early-close failure remains a
  source-checked hypothesis, **unverified on real hardware** here.
- Native macOS/Linux, full/elevated administrator, SYSTEM and actual cross-user
  service runtime remain **unverified on real hardware**. No native Unix runtime
  or hosted rerun/dispatch was performed. Existing CI will exercise the actual
  early-disconnect path; a coordinator-owned `--no-fail-fast` hosted run is advised
  to preserve all platform failures. Fresh exact-head independent review and all
  hosted required checks must pass before merge; neither is claimed here.
- Root PR #16 path-prefix fix `5bb0969` remains separate and unmerged; it was not
  cherry-picked or duplicated and did not block local tests. No Unix executor,
  Windows experiment, ADR0009, service architecture, privileged/owner operations
  or old Lab Manager changes were made. Services, dedicated-account execution,
  tool discovery and Claude Code cargo acceptance remain outstanding for M2.

## Service implementation contract

- Opus / claude-opus-5-5 high authored docs-only ADR 0009 and the owner service guide, originating at dd0eb7e. The coordinator read both in full against KICKOFF M2 and the current executor/transport boundaries; no service or account action was run. The tool vocabulary includes PR #18�s fixed unknown_tool audit label. Lifecycle and activity implementation slices are next; all privileged mechanisms and full M2 acceptance remain unverified.


## M2 ADR0009 slice 5: activity v2 candidate (Sol high, 2026-11-11)

- Base `2aba6c3154201ca6e9afcd1ab53f984a002de600` includes unmerged PR15
  correction `30f5c00` and Opus ADR0009. Last merged PR remains #12 (`03adb434`)
  as recorded above; no remote status was queried or changed here.
- Provider/effort/outcome: Sol / gpt-6.1-sol high / implemented local candidate.
  Store: transactional version 0 identity initialization and version 1-to-2 migration,
  lossless legacy IDs/data and AUTOINCREMENT high-water preservation, atomic recovery,
  bounded UTC timestamps, typed nullable metadata with SQL CHECKs, daemon/arxd lifecycle
  entrypoints, separate read-200/action+lifecycle-1000 retention, post-work transaction
  page-cap checks. Service Host integration belongs to the other builder.
- MCP: exact list_activity arguments; immutable authenticated principal; agent-only
  visibility; limit+1 SQL pages below min(own committed read ID,before_id); own-row
  exclusion; completion-before-exposure; rejected-argument audit; fixed unknown_tool.
  PR18 exact-one-started transactional completion semantics adopted from `6b24111`
  source without cherry-picking unrelated commits/docs.
- Transport: strict 8 KiB requests and 64 KiB responses through the SDK pump and real
  stdio bridge; no total connection deadline. Serialized 50-row maximum-field pages
  (184-byte SID, large IDs/timestamps/numerics, full metadata), including a conservative
  8 KiB request-ID envelope, fit 64 KiB and exceed 8 KiB over HTTP and actual subprocess
  stdio. Exact 65536/65537-byte frame tests verify newline and partial withholding.
- Coordinator clarification: cancellation/timeout/shutdown finish as Outcome::Failed
  with typed StopReason::Cancelled/Timeout/Shutdown, persisted in the same update.
  Those strings are rejected as outcomes. Clean lifecycle completion can succeed.
  README and API docs state the mapping; no inconsistent outcome is introduced.
- Local Windows evidence: fmt check; workspace/all-target Clippy with -D warnings;
  locked workspace nextest **38/38 passed** (baseline 28). Coverage: atomic/lossless
  migration; legacy-0 recovery; unsupported version 3; exact pagination boundaries;
  principal/daemon isolation and own row; read/action floods; missing/duplicate/repeated
  completion rollback; bounded corrupt values and extra/missing columns; SQL CHECKs;
  secret-marker args/unknown calls absent from DB bytes; invalid args audited rejected;
  token revocation after restart on SDK HTTP and real stdio; start failures refusing
  dispatch and finish failures withholding results on both. Triggers are fault injection.
- Actual red/green mutations, each restored: targeted tests exited 101 with assertion
  failures when exact-one completion was bypassed, query included own row, read retention
  expanded to 1000, v1 reconciliation committed before migration, or unknown calls used
  list_hosts label. Restored source passed the full suite. Fixture corrections recreated
  actual legacy tables rather than relabeling v2 and kept secret markers out of trigger DDL.
- Scope: store/MCP, inventory tests, narrowly necessary frame bounds, crate README and
  this progress entry. No main/service/probe/executor/CI workflow changes; no agents,
  push, PR, GitHub writes, dispatch, account/admin or old Lab Manager work.
- Remaining: independent different-provider exact-head review, dependency integration
  and hosted gates are unaccepted. Linux/macOS native runtime and all privileged or
  dedicated-account paths remain **unverified on real hardware**. Future execution
  metadata can remain NULL; no executor/tool stub route, command run, hardware evidence,
  service acceptance or full M2 acceptance is claimed.
