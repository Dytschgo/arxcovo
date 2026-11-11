# Arxcovo progress

## Current state — 2026-11-11

- Milestone 1 is accepted. Milestone 2 is in progress; milestones 3–6 remain unimplemented apart from research and scaffolding.
- Last merged PR: [#10](https://github.com/Dytschgo/arxcovo/pull/10), `b5e65561b001a3616fa8a6623512e7ff8aa0e346`, 20:23:38 UTC, through all five gates. Its feasibility notes remain proposed/historical; the owner-only development-host checklist is prepared before M4 and proves no hardware acceptance.
- Main includes service, owner and Unix execution libraries plus accepted opt-in Windows development execution. Native backends, owner provisioning/CLI and installed/dedicated M2 acceptance remain incomplete. macOS owner parsing still refuses Unknown; no Unix execution route is activated.
- Queue priority: obtain a fresh independent recheck of corrected #36 and finish existing #9/#26/#34. No new feature PRs while the four-PR queue drains. Windows owner API narrowing E1 and fixed Linux protection-site diagnostics remain local and unreviewed.
- Main push CI after #14 passed on its first attempt, run 43091130349: **549 seconds**, below 15 minutes. New #10 main CI is not yet claimed green. The first reviewed disposable Linux experiment failed `E_PROTECTION`; its SHA variable is cleared and no retry or acceptance is claimed. No third S4U probe or owner-PC administration is authorized.

## PR29 compatibility integration — Sol high, 2026-11-11

- Integrated implementation commit: `79959a33a0422cf9bd594cb99d1323d85051740d`, parents exactly `06c7f6b646b64adf615c98353b9a5a9111636b5d` and `4f2b0c7177c8b7370d34484e4739082364859f4a`. Following documentation-only evidence commits do not alter tested code.
- Files changed relative to prepared base: `crates/arxd/src/service/config.rs`, `mod.rs`, `owner_plan.rs`, `owner_transactions.rs`, `owner_transactions_tests.rs`; `docs/PROGRESS.md`, `docs/adr/0009-service-lifecycle-and-dedicated-execution.md`, `docs/adr/0012-windows-owner-transaction-and-right-provenance.md`, `docs/reviews/0012-owner-contract-review.md`, `docs/service-foundation.md`, `docs/service-install.md`. The provenance module/contract/review come from actual main; the shared plan resolves compatibility, Unix call sites use None, and the cross-platform test is strengthened. No new ADR was needed.
- Provider/effort: OpenAI / gpt-6.1-sol / high, implementation sub-agent. Prepared base `06c7f6b646b64adf615c98353b9a5a9111636b5d`; merged actual main `4f2b0c7177c8b7370d34484e4739082364859f4a` locally, preserving both histories. No force, push, PR mutation, GitHub access, watch, merge-to-main, subagent or CLI activation. This is compatibility only; no new runtime capability or acceptance.
- Windows keeps `Option<BatchObservation>`, the opaque matching observation contract, transaction-only configure/uninstall activation refusal, record-only revoke authority, no compensation, and uninstall independent of config/account/workspace with no emitted BatchRight removal. Unix passes None and retains typed lifecycle plans, gates/status, exact manager allowlist, optional endpoint preflight, retained-state provisioning, macOS namespace/creation-only chown and fail-closed Unknown parsing. Application/sharing, native adapters, Unix query, layout and packaging bytes match the prepared base. Existing stale-plan tests cover both directions and direct unresolved Intent uninstall still refuses before observation.
- Compatibility coverage strengthens the existing Linux/macOS shared-API test: configure and uninstall reach an injected recording manager (all operations counted), carry no Windows SCM/right operations, and uninstall retains the Unloaded proof. Existing Windows tests prove no manager call occurs on direct activation and no resolver is consulted on uninstall. No production constructor or native owner path was invoked.
- Executed: `cargo fmt --all -- --check`; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed. `cargo nextest run --workspace --locked --no-fail-fast`, run `ea9ab473-0c0b-4bc8-ac6b-f253a0ee0edd`, finished in 10.302 s with **149 passed, 1 failed, 2 skipped**. Failure: `development::windows::tests::actual_job_tree_timeout_cancel_shutdown_overflow_and_handles`, `crates/arxd/src/development/windows.rs:818:69`, `overflow: ExecutionFailed` (3.512 s). The two direct-only fixtures were exercised by their existing normal parents. No ignored fixture was run directly.
- This is a recurrence of the unresolved ExecutionFailed error family from the prior 133/136 run (cancel/junction/SDK positive control), now at overflow. The junction and SDK controls passed this time. It is distinct from the old ETW/handle-marker issue; root cause remains unknown. No retry, attempted fix, timeout/limit change, serialization, weakened test/policy or unrelated patch was applied. The full workspace check is **not green** and blocks publication readiness.
- Focused command: `cargo nextest run -p arxd --lib --locked -E 'test(service::owner_) | test(service::layout::tests::unix_) | test(service::layout::tests::mac_) | test(service::config::tests::macos_admin)' --no-fail-fast`: **37/37 passed**, run `7ec9660b-26c9-4f2c-bc60-a5915f3f66ac`, 2.313 s. Includes depth-6 provenance/mutation models and typed Unix gates. Portable checks: `node --test scripts/*.test.mjs` **18 passed**; `python -m unittest discover -s tools/ci -p 'test_*.py'` **26 passed**; `python scripts/test-service-artifacts.py -v` **3 passed, 2 native Unix skips**. Whitespace check passed. Initial fmt check requested ordinary formatting changes; formatting was applied before the passing check.
- Remaining: coordinator dependency integration, fresh independent Opus high exact-head review and required hosted gates. Existing advisories remain open: case-sensitive Windows pipe comparison, SYSTEM-guard test coverage, state-protection exit mapping and unrelated historical docs; macOS endpoint creation is preserved from PR29 but native evidence remains absent. macOS launchd absence/state fixture constants remain unpinned and Unknown stays fail-closed. Native Unix compilation/link/runtime and native systemd/plutil were not exercised here; no metadata-only or native-runtime pass is claimed. Installed/dedicated/native M2 acceptance remains **unverified on real hardware**. No actual owner/root/manager/SCM/LSA/privilege/account/service/provisioning/VM/LabManager operation ran.
- Main push CI after #33 passed on its first attempt, run 43079116241 at `7c150f0`: **532 seconds**, below 15 minutes. The new #37 main push is not yet claimed green. No third S4U probe, owner-PC administration, old Lab Manager or protected VM work is authorized.

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
| Owner-store fixture interference diagnosis | Anthropic / claude-opus-5-5 | high | Cause proven: child-launching development tests pin `%TEMP%` with read-only sharing, which blocks fixture renames. Test-only patch artifact: full suite 144/144 in 5 nextest and 3 cargo test runs. Sol applies the patch; independent review pending |

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


## Unix review correction checkpoint

- PR #14 corrections integrated from 9417e62: bounded cgroup ownership checks, accessible symlink test controls, descriptor copying, cleanup failure reporting and runtime controller assertions. Builder evidence: 28 pure Python tests, nine killed mutations, 22 Windows workspace tests, formatting and Clippy passed. Unix metadata checks passed; full local Linux workspace build lacked a C cross-toolchain. All privileged runtime assertions remain unrun and **unverified on real hardware**. Fresh independent review and hosted checks are required before merge or a Linux experiment.

## PR #14 positive-control correction

- Second independent review failed at a5777bb: the plain directory inherited mode 0700 under service UMask=0077. Positive-control creation now starts at 0700 and explicitly sets 0755 through a shared helper for outside/plain, with a non-privileged Linux regression requiring traversal and refusing reuse. Its runtime cross-account assertion remains unrun.
- Added pure fixture-check coverage proving the captured UID/GID are forwarded to the post-account scan. All 29 Python tests pass and ten driver mutations are killed by assertion failures. Fixed invalid UTF-8 and provider-table formatting. Provider: Codex / gpt-6.1-sol, medium effort. Fresh independent review and hosted checks are required; no privileged experiment has run.

## Unix executor integration with current main

- Codex / gpt-6.1-sol, medium: integrated main d7f3ebd with the corrected Unix backend. The accessible positive controls and fixture-check forwarding remain present. Formatting, Clippy with warnings denied and all 44 Windows workspace nextest tests passed. The 29 pure Python tests passed and all ten driver guard mutations were killed with assertion failures and zero errors. Fresh independent review and hosted native checks are pending; no root experiment or owner administration has run.

## Unix path-main update and review checkpoint

- Opus 5.5 high independently passed b3b808c: 44 Windows nextest tests, 29 Python tests and all ten assertion-failing mutations. Comment: https://github.com/Dytschgo/arxcovo/pull/14#issuecomment-6794916805. Main then contained #16, so the prior head conflicted only in progress and hosted checks did not run.
- Codex / gpt-6.1-sol, medium: resolved progress against merged path main b29f531, retained all provider rows, removed the duplicate table header, and updated historical validation notes. Executor/tooling/workflow source is unchanged from the reviewed head; main adds its path helper/regression and README encoding fix. New-head formatting, Clippy with warnings denied and all 45 nextest tests passed on Windows. Executor, tooling and workflow source is byte-identical to the reviewed b3b808c. Fresh hosted checks and independent recheck are pending.
- Advisory A3 remains open: the positive-control unit test checks explicit final mode but does not set a restrictive process umask. The actual privileged harness uses UMask0077, remains unrun, and no elevated/runtime containment acceptance is claimed.

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

## Main CI duration history refresh

- Codex / gpt-6.1-sol, medium: read all 17 completed main push CI runs through the GitHub API, fetched attempt 1 for each and cross-checked workflow completion against the latest job completion (within 30 seconds). The committed history now includes every completed run through PR22 main run43023260965. Existing five records were verified unchanged. JSON and whitespace validation passed; the history is evidence only and does not control the trusted budget gate. No workflow, policy or timeout changed. Independent review remains pending.
## Unix executor integration with current main

- Codex / gpt-6.1-sol, medium: integrated main 8fafbe7 into PR14, preserving current-user Windows execution and the reviewed retention test correction. Formatting, workspace/all-target Clippy with warnings denied, all 65 Windows nextest tests, 29 pure Python guards and all ten executable guard mutations passed. The Unix executor and disposable driver production bytes are unchanged from the prior independently reviewed candidate; native/privileged Unix runtime remains unverified on real hardware. No experiment was dispatched and no owner administration ran. Fresh exact-head review and hosted checks remain pending.

## Unix restrictive-umask test correction

- Codex / gpt-6.1-sol, medium addressed PR14 advisoryA3: the ordinary Linux positive-control test now launches an exact subprocess-only fixture with umask0077 set before exec. The child verifies that mask, requires outside/plain end at0755 and rejects directory reuse. The parent bounds runtime/output and proves exactly one named test ran. This leaves the parallel runner umask unchanged and invokes no account/service operations. Formatting and Windows workspace/all-target Clippy passed; native Linux execution and the creation-mode-only mutation remain pending hosted verification. Production executor/driver and privileged harness bytes are unchanged.
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

## Unix validation conflict resolution

- Codex / gpt-6.1-sol, medium resolved the docs-only conflict with main66c1b17 so hosted CI can exercise the restrictive-umask fixture at the integrated head. Executor, driver and test source are unchanged from18d4f1f; native test evidence remains pending. No privileged experiment ran.
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

## Main CI history through merged handle correction

- Codex / gpt-6.1-sol, medium reconciled this docs-only branch with main and
  fetched the first-attempt API records for the two newer completed main runs.
  Run 43027101934 at 66c1b17 failed after 219 seconds; run 43032105696 at
  0ceb174 succeeded after 525 seconds. Both conclusions remain in the history,
  so the short failed run cannot be mistaken for a speed improvement.
- The history now contains all 19 completed main push CI runs visible in the
  API through PR28. No policy, workflow, required check or timeout changed.
  This evidence refresh still requires independent review and hosted checks.
## Unix execution integration after Windows test correction

- Codex / gpt-6.1-sol, medium integrated merged PR28 into PR14, retaining both
  progress histories. The inherited changes are confined to the Windows handle
  regression fixture and its debugging documentation; Unix execution and guarded
  experiment behavior are unchanged. PR14 head d018a90 passed hosted Ubuntu,
  macOS and Windows CI before this integration. No privileged experiment ran.
- The integrated revision still requires fresh hosted checks and independent
  exact-head review. Root/systemd/cgroup runtime remains **unverified on real hardware**.

- Coordinator combined-head verification: formatting and workspace/all-target
  Clippy passed, followed by 66/66 Windows nextest tests. The actual Ubuntu log
  for run 43030809255 at d018a90 confirms the positive-control parent and nofollow
  tests passed, along with 29 pure driver tests. This supplies native ordinary-user
  fixture evidence, not root execution or cgroup containment acceptance.
- PR21 Ubuntu job 130562541853 also failed during Playwright dependencies with
  repeated Ubuntu-mirror DNS failures; its Windows and macOS jobs passed. The
  failure is not waived, and no repeated automatic retry was requested.
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

## M2 independent Unix owner operations candidate

- Provider/effort/outcome: Codex / gpt-6.1-sol, high; implementation sub-agent
  working alone on `t3/unix-owner-operations`, prepared base
  `4e66c555686d8ee9084d700ebf4dc60ce926ea74` (Application PR27 stack).
  Implemented callable fixed-layout Unix owner executor and separate protected
  empty state provisioning. No main CLI, Application, transport or store change.
  Code commit: `90948f6` (feat(service): execute protected Unix owner plans).
  Last merged PR and M2 dedicated-account acceptance remain unchanged.
- Done: fresh native root/current-binary gates, descriptor-relative no-follow
  operations, ACL/owner/mode/link/type/ancestor refusal, exclusive creation,
  identical-artifact/no-takeover handling, parent-locked atomic config publish
  with file/parent fsync, bounded fixed manager commands with explicit environment,
  conservative stop/unregister, disabled-by-default macOS installation and
  explicit start enable/bootstrap. Provisioning initializes only empty protected
  DB/journal/lock prerequisites; existing Activity performs schema initialization.
- Checks actually run: Windows fmt check and workspace/all-target Clippy with
  warnings denied; service library suite 47 passed/one ignored direct fixture
  exercised by its existing ordinary-user parent; three new portable adapter
  tests passed; artifact script three passed/two native skips. Three restored
  allowlist mutations each killed by assertion; baseline rerun passed. Isolated
  actual-source Linux/macOS test metadata Clippy passed with warnings denied.
  Six ordinary-user Unix fixture tests compiled, not executed. Full Unix link
  and runtime were not attempted on Windows. Details/API/limitations in
  docs/service-foundation.md.
- Corrections during validation: Windows-only unused imports/dead helpers were
  cfg-gated; cross-check exposed helper visibility and Darwin mode_t width;
  isolated Clippy found test placement/collapsible branch. Each was corrected;
  no symptom reached three failed fixes. No fence was weakened.
- Open: independent other-provider review; native ordinary-user fixture runs;
  disposable root installation/config CAS/protected SQLite first-start/restart;
  native ACL/default ACL and manager timeout/stop/unregister semantics; macOS
  first reboot stays disabled and explicit start enables/bootstrap; dedicated
  executor and full Claude Code/cargo/activity acceptance. Manager nonzero
  statuses stop uninstall (including possibly already absent launchd jobs);
  no successful cleanup is claimed then. All privileged paths remain
  **unverified on real hardware**. No new architectural deviation is claimed.
- No privileged operation, OS-path owner-adapter invocation, elevation,
  account/right/task/service/dispatch/settings mutation, old Lab Manager/VM
  access, push, PR, merge, watch, GitHub write or subagent occurred. Actual local
  mutations were repository/ignored build output and ordinary-user test fixtures.

## Unix owner adapter coordinator integration

- Codex / gpt-6.1-sol, medium integrated candidate7e12abf above the service Application ancestor/handle corrections. Formatting, workspace/all-target Clippy with warnings denied, three portable owner-adapter tests and artifact checks (three passed, two native platform checks skipped) passed. Linux/macOS native fixture/link/runtime and independent review remain pending. No privileged installation, manager command, account or owner operation ran; installed-service M2 acceptance remains open.

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

## Unix owner adapter preflight-parent refresh

- Codex / gpt-6.1-sol, medium aligned PR29 with Applicationbb077bb, retaining both owner_unix and preflight module exports. Formatting and all-target Clippy with warnings denied passed; three portable owner-adapter tests and nine configuration/boundary tests passed. Native Unix fixtures, root-owned paths and installed managers remain unverified here. Hosted CI and independent review remain pending.

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

## Superseded owner transaction coordinator integration

- Codex / gpt-6.1-sol, medium preserved candidatea8628c6 on feat/windows-owner-transactions above the integrated service Application. Formatting and all-target Clippy with warnings denied passed; all nine focused owner-transaction tests passed. This delivers only a pure transaction prerequisite, not the requested native Windows installer adapter. Native ConfigurationIo, SCM/LSA operations and durable uninstall provenance remain absent. A crash between right grant and durable receipt remains RecoveryRequired; presence never proves ownership. Cross-provider architecture guidance is required before native activation. No owner or privileged operation occurred.

## Windows owner recovery architecture review

- Anthropic / claude-opus-5-5, high delivered docs-only ADR0012 at784fb16, superseding the old pure Journal/ConfigurationIo design. Codex / gpt-6.1-sol, medium independently read all four changed docs and found changes required: P1 arbitrary record-rollback safety claim has a pure-model counterexample, and P2 missing-account reason contradicts ADR0009. Strict UTF-8, balanced fences and scoped whitespace checks passed; no runtime/privileged check ran. See docs/reviews/0012-owner-contract-review.md. Native owner implementation remains blocked on contract correction and the ancestor-policy fix.

## Corrected Windows owner contract recheck

- Anthropic / claude-opus-5-5, high corrected ADR0012 atb522125. Codex / gpt-6.1-sol, medium independently rechecked all three changed docs and accepts the bounded documentation contract: P1 unconditional power-loss claim removed, record rollback explicitly outside guarantee, P2 reason split preserved. UTF-8/fence/whitespace checks and depth6 bounded scratch model passed the stated characterization. No native behavior is proven; the old Journal implementation must be replaced before activation. See docs/reviews/0012-owner-contract-review.md.


## ADR 0012 PR-A pure owner provenance candidate

- Provider/effort/outcome: Codex / gpt-6.1-sol, high, implementation sub-agent
  working alone from pinned prepared base
  `7e7d3bfd49b558f2e5057260694a099540534906` on
  `t3/implement-owner-provenance-engine`. Replaced obsolete Journal,
  ConfigurationIo and compensating configure/recovery with strict 512-byte
  OwnerRecord, OwnerStore/BatchRight, opaque observe_batch, recover, configure
  and uninstall_right. Intent resolves only to None/Unproven; configure/recover
  never revoke; only Granted authorizes uninstall using the record's explicit
  SID. Last merged PR and M2 acceptance are unchanged.
- Completed tasks: P1-P17 plus P6b/P6c; config mirror/old canonical expected-byte
  paths, stale presence and record-only observations, cross-SID states, record
  decoding, CAS, receipt/publication and other store failures with/without
  effects, fixed errors/reports, explicit old-SID uninstall, unavailable
  account/workspace and no-config uninstall path. Windows Plan::execute fails
  closed for configure/uninstall and uninstall plans contain no BatchRight.
  Former reserved-SID/group coverage is retained alongside new provenance tests.
- API integration: configure_plan takes Option<BatchObservation>: Windows
  requires Some from the locked private store; Linux/macOS require None and
  retain their owner planning/validation behavior without LSA/provenance.
  Configure::from_plan takes the same opaque observation after resolver.
  Unix adapter candidate 7e12abf / PR29 needs the coordinator's argument update;
  no candidate files or unrelated Windows handle-flake fixes were copied.
  See docs/service-foundation.md for precise session ordering, API and limits.
- Actual Windows checks: cargo fmt --all -- --check; cargo clippy --workspace
  --all-targets -- -D warnings; cargo check --workspace; cargo build --workspace;
  cargo test -p arxd service::owner --lib -- --nocapture (19 passed);
  cargo test -p arxd service::config --lib (7 passed);
  cargo test -p arxd service::layout --lib (2 passed); git diff --check.
  Final focused owner suite took 1.93 seconds after compilation. No complete
  workspace runtime suite or hosted CI was run; the inherited intermittent
  Windows handle test remains unresolved and unclaimed by these local passes.
- Model bounds/results: depth 6, every adapter call prefix (0..15 plus complete),
  both commands with recovery, both external toggles, three grant/revoke outcomes,
  and equivalent-state merging. Completed record writes persist in P6/P6b.
  P6: 11,544 transitions, zero unsafe revokes. P6b LSA-only earlier-value loss:
  15,365 transitions, zero unsafe revokes. P6c record rollback to any previously
  seen value: 47,281 transitions, 312 unsafe revokes, all characterized by stale
  Granted restored over a later non-Granted write; exact decision 6 trace pinned.
  Independent ghost attribution detects both seeded receipt mutations: taking a
  pre-existing right (52 unsafe witnesses / 12,064 transitions) and recovery
  promoting to Granted (376 / 12,168). These mutations fail the P6 safety
  predicate; the harness requires witnesses. No physical NTFS/LSA proof is claimed.
- Corrections during verification: a fixture initially asserted only the primary
  SID on a valid cross-SID None replacement; another injected read-error fixture
  failed to return Err when its effect flag was set. Both fixture issues were
  corrected and the focused suite rerun. One Clippy let-and-return diagnostic
  was corrected. No symptom reached three distinct failed fixes.
- Remaining blockers/tasks: fresh independent Opus implementation review, native
  store/locking/publication and LSA (PR-B), SCM (PR-C), checked CLI session wiring
  (PR-D), corrected native ancestor authority, service workspace boundary,
  protected state provisioning, dedicated-account execution and installed M2
  cargo/Claude Code/activity acceptance. Native and installed paths remain
  **unverified on real hardware**. The accepted architecture does not accept
  this implementation automatically.
- No subagents, push, PR creation, merge, watch, admin/account/right/task/service/
  install/provision/dispatch/settings operation or old Lab Manager/VM access.
  Record rollback remains explicitly outside the safety guarantee. No new
  infrastructure, approval flow, acknowledgement flag or architectural deviation.

## Windows owner provenance integration checkpoint

- Codex / gpt-6.1-sol, medium integrated candidate 120752c onto the current
  service Application, preserving the Windows namespace policy, typed preflight
  reasons and protected SQLite hot-journal recovery. Both modules remain exported.
  The service-boundary owner-plan fixture now passes the Unix `None` observation.
- Formatting, workspace/all-target Clippy with warnings denied, and all 129
  runnable Windows nextest tests passed. Two subprocess-only fixtures were skipped
  directly and exercised by their parents. The bounded owner model and actual
  normal-user hot-journal regression both passed in this combined run.
- This is pure owner-provenance logic; no native OwnerStore/LSA/SCM implementation
  or CLI activation is delivered. Independent Opus review and hosted checks remain
  required. Installed services and dedicated execution remain **unverified on real hardware**.
- PR20 attempt 2 Ubuntu job 130573258883 failed at Playwright dependency installation
  with DNS resolution failures for Ubuntu mirrors, as attempt 1 did. Windows,
  macOS and independent review passed; the Ubuntu gate remains required. No second
  automatic retry or check bypass was performed.

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

## Unix lifecycle contract accepted for implementation

- Anthropic / claude-opus-5-5, high authored ADR0014 and corrected the independent
  Sol finding about reinstall boot intent. Codex / gpt-6.1-sol, medium rechecked
  all four corrected documents at d1ef91b and accepted the contract, with the
  upgrade guide clarified to use the old installed binary for uninstall.
- The contract now puts Installable before any mutation, refuses active or
  foreign/unknown registrations, preserves retained activity history, and
  explicitly disables boot intent on a successful inactive install. The known
  stale-enable reboot window and macOS unpinned query results remain explicit.
- The PR29 branch now includes the current parent preflight, protected SQLite
  recovery and explicit fixture-owner correction, plus the accepted docs. No
  lifecycle code correction has been implemented by this integration. No manager,
  owner or privileged operation ran; installed paths remain **unverified on real hardware**.

- Coordinator preparation checks passed: formatting, workspace/all-target
  Clippy with warnings denied, and all three portable owner_unix tests. Native
  Unix manager behavior was not exercised by these Windows checks.

## ADR0014 implementation foundation — incomplete

- Provider/effort: OpenAI / gpt-6.1-sol / medium. Prepared base verified as
  `d1e2425e8b37c038b65076b799bcbddc332dc597`; initial worktree clean.
- Outcome: added portable `service::owner_query` types, bounded systemd property
  parser, fixed status tokens, expectation/gate predicates and the three fixed
  lifecycle errors. These APIs are not wired into owner plans or the executor.
  `launchd` deliberately returns Unknown for all inputs; even the positive-output
  parser remains unfinished. No absence exit code or state vocabulary was invented.
- Checks run: `cargo fmt --all -- --check`, workspace/all-target Clippy with
  warnings denied, `cargo test --workspace --no-run`, and
  `cargo test -p arxd service::owner_` (14 passed, including four new query tests).
  Workspace test compilation preceded the fourth test; that test subsequently
  compiled and passed in the focused run. `git diff --check` passed.
- Native cross-target attempts: Linux and macOS `cargo check -p arxd --tests`
  stopped in bundled SQLite's C build before checking arxd. Linux lacks
  `x86_64-linux-gnu-gcc`; macOS lacks `cc`. No native metadata or runtime pass
  is claimed. All systemd/launchd and installed paths remain
  **unverified on real hardware**.
- Remaining implementation: exact lifecycle plans and command/gate allowlist,
  runner and fixture-Fs seam, bounded spawn/query/drain, explicit optional-parent
  StartPreflight, stop postcheck, boot intent ordering, provision integration and
  retained-state handling, UnitAbsent publication, macOS endpoint creation and
  all ADR matrix/retry/failure/state-preservation/model tests. No guard mutations
  were executed (0/7 required); mutation acceptance is outstanding.
- Coordination: `configure_plan` retains this base's bool batch observation.
  PR30's Option<BatchObservation> integration must adapt Unix callers when that
  parent lands; no engine or Windows adapter changes were copied here.
- Independent Opus review is still required. PR29's unloaded-job blocker is not
  closed; CLI activation is not ready. Native launchd fixtures, stock ancestor
  policy, installed-service acceptance and the full M2 dedicated-account goal
  remain open. Last merged PR remains #28 as recorded above.
- No manager, privileged harness, owner administration, dispatch, settings,
  old Lab Manager or VM operation ran. No push, PR write, merge or watch ran.

## Unix typed-query continuation checkpoint

- Codex / gpt-6.1-sol, medium integrated incomplete candidatece7d044. It supplies
  bounded systemd parsing, fail-closed launchd queries, typed gates/expectations
  and status mapping; lifecycle plan and native runner integration are unfinished.
- Coordinator found contradictory not-found/active observations were classified
  Absent. A new regression failed before correction. Absent now additionally
  requires inactive and no fragment/drop-in path; active, transitioning and failed
  not-found observations remain Unknown and cannot prove stop. ADR0014 documents
  that fail-closed consistency requirement.
- Formatting and workspace/all-target Clippy passed; all five portable query
  tests passed. No manager, owner or privileged operation ran. The next bounded
  slice is typed lifecycle plans and gate tests, followed by native runner and
  retained-state/runtime-directory work. PR29's functional blockers remain open.

## ADR0014 phase 2: typed portable plans (2026-11-11)

- Provider/effort/outcome: OpenAI gpt-6.1-sol / medium / typed-plan slice completed
  locally from verified base 074530b5dc1ba51862c3927af6d9e1cbb2aebe97.
  Work alone; no delegate, push, PR write, merge or watch. Last merged PR unchanged.
- Explicit Expect, StartPreflight, ProvisionState, command gates and constant
  gates encode the eight ADR0014 Unix lifecycle plans. Installable precedes all
  mutation, drop-in precedes unit with UnitAbsent, install disables boot intent,
  and stop proof precedes removal. Endpoint EnsureDirectory is disallowed.
  Windows plans/source behavior and bool configure/uninstall APIs are retained.
- Safe callable contracts: pure owner_plan/configure_plan/uninstall_plan remain
  available; Unix Status returns fixed Manager rather than a mutating plan.
  owner_query remains the read-only parsing/predicate/status API for a future
  adapter. Query vectors are deliberately rejected by the mutating command gate.
- Native Unix execute validates the entire plan and rejects new unsupported
  expectations, preflight, provisioning and conditional gates with Manager before
  applying any operation. apply also checks that boundary. Hidden launchd
  enable/disable and hidden command preflight were removed. Configure's existing
  Always drop-in/reload semantics remain. Existing standalone provision_state is
  unchanged and does not implement retained-state provisioning.
- Acceptance: cargo fmt --all -- --check; cargo clippy --workspace --all-targets
  -- -D warnings; cargo test -p arxd --lib service::owner_ (18 passed); cargo test
  -p arxd --all-targets --no-run; git diff --check all passed on restored sources.
  Initial service suite exposed the obsolete Unix status-plan expectation; fixed.
  Model lifecycle/reinstall/boot-intent tests are MODEL evidence, not runtime.
- Actual source mutations run and killed by unix_exact_typed_plans: remove start
  enable; remove install disable; swap stop disable/stop order; ungate install
  drop-in; omit Installable; omit Unloaded. Sources restored and owner suite green.
  No retained-state native mutation test is claimed.
- Cross-target cargo check -p arxd --tests attempted for Linux and macOS. Both
  blocked in bundled SQLite C build (missing x86_64-linux-gnu-gcc and cc,
  respectively); no cross-target metadata success is claimed.
- Phase 3: implement bounded native query runner and fresh gate observations,
  optional-endpoint start preflight, retained-state ProvisionState with locks and
  durability, macOS service endpoint creation and native fixtures/ancestor policy.
  Validate actual-source Linux/macOS metadata and native failure/timeout behavior.
  PR30 coordinator must integrate None Unix batch observation on its later base.
  Independent different-provider review remains required before any merge.
- PR29 is not complete. Native and installed M2 paths remain **unverified on real
  hardware**. No real manager, privileged harness, owner administration, dispatch,
  settings, old Lab Manager or VM operation was invoked for this slice.

## ADR0014 phase 3a: bounded native runner candidate (2026-11-11)

- Provider/effort/outcome: OpenAI gpt-6.1-sol / medium / implementation and
  metadata checks complete; Unix runtime acceptance remains outstanding.
  Verified clean prepared base `3ced94d70fa990c5a61aabf4dde358602ca04210`.
  Worked alone. Last merged PR unchanged (#28); no push, PR write, merge or watch.
- `owner_unix::execute(&Plan) -> Result<(), Error>` now executes supported typed
  expectations, fresh command gates and UnitAbsent publication. Every observation
  checks the primary artifact and Linux drop-in through protected descriptor
  traversal and exact bytes. Foreign/Unknown halt; false gates record a skip
  count. The entire plan is allowlisted and scanned for unsupported StartPreflight
  or ProvisionState before any operation. Install/start remain refused in phase 3a.
- Added `owner_unix::status() -> Result<owner_query::Status, Error>`: fixed tokens,
  same production root identity, installed-binary and namespace boundary as execute.
  No CLI activation. macOS native parser remains Unknown for every result, pending
  captured output/exit fixtures; typed scripted macOS results are test-only.
- Private Runner uses fixed systemctl/launchctl vectors, cleared fixed environment,
  constant cwd, null stdin/stderr, 35s mutation deadline and 10s query deadline.
  Nonblocking stdout retention is capped at 4096/65536 bytes. Nonzero, signal,
  timeout, overflow, read/spawn failures halt; failure kills and polls reaping for
  at most 1s. No raw output escapes. No hidden enable/disable inside publication.
- Added cfg(test)-only ordinary-user protected Fs/Script construction and six
  tests covering whole Linux/macOS uninstall ordering, fresh bootstrap gates and
  skips, active/foreign/unknown refusal, stop proof before removal, failed-command
  suffix halting, whole-plan unsupported refusal, artifact checks/UnitAbsent,
  and safe `/bin/sleep 5` 100ms timeout, echo stdout bounds and false nonzero.
  These Unix tests compile as metadata but were NOT executed on this Windows host.
- Passed: fmt check; workspace/all-target Clippy with warnings denied; workspace
  test compilation; all 18 portable owner tests; diff whitespace check. Actual
  source mutations killed: ungated bootstrap, Unknown-as-Absent, removed stop proof.
  Mutations restored and portable owner tests passed afterwards. Ignored-timeout
  and ignored-nonzero mutation execution remains outstanding on ordinary Unix.
- Linux x86_64 and macOS arm64 actual-source test metadata and Clippy passed.
  Commands used LIBSQLITE3_SYS_USE_PKG_CONFIG=1 and SQLITE3_LIB_DIR pointing at the
  checkout solely to supply SQLite link hints for cargo check/Clippy, avoiding
  bundled SQLite C compilation. No compiler shim, linking, native executable,
  actual SQLite cross-build or native runtime success is claimed.
- Phase 3b contract: implement optional-endpoint StartPreflight and protected
  retained-state ProvisionState, then remove only those unsupported scan cases;
  preserve fresh query gates, root/incarnation preflight, CAS/private permissions,
  and bounded runner. macOS endpoint/ancestor policy and captured launchd fixtures
  remain open. Coordinator integrates PR30 Option<BatchObservation> separately.
- Acceptance gap: execute ordinary-user Unix fixtures and timeout/nonzero source
  mutations before phase 3a acceptance; independent other-provider review remains
  required. PR29, installed acceptance and remaining M2 are NOT complete. All real
  manager and installed paths are **unverified on real hardware**. No real manager,
  privileged fixture, owner admin, dispatch, settings, old Lab Manager or VM ran.

## macOS stock-directory observation preparation

- Codex / gpt-6.1-sol, medium added a read-only macOS fixture and a targeted
  hosted CI invocation to retain its bounded output. It opens five fixed system
  directories with descriptor-relative no-follow traversal and reports fixed
  labels, root-owner/group-zero booleans, mode bits, ACL-check success and the
  existing integrity-policy result. No raw IDs, account names, paths, directory
  listings or contents are emitted. It changes no owner, permission or policy.
- This is diagnostic evidence collection, not a passing startup/containment
  assertion. A diagnostic pass means the observations were captured, even when
  the policy rejects a stock folder. Native macOS execution remains pending;
  Windows-only checks cannot establish these facts. Architecture/security review
  is required before any rule is relaxed. Installed services remain
  **unverified on real hardware**.

- Windows formatting and all-target Clippy passed for this diagnostic candidate.
  They do not compile or execute the macOS-only fixture. Hosted macOS collection
  and independent review are pending; no startup-policy result is claimed.

## Hosted macOS stock-directory evidence

- Codex / gpt-6.1-sol, medium inspected completed macOS job130592903303,
  run43041133895 at69c1a6c. All five fixed-directory observations were captured.
  The current integrity policy accepts library/private/var, but rejects
  application_support (root owner, nonzero group, mode0755, confirmed empty ACL)
  and runtime_parent (root owner, nonzero group, mode0775, confirmed empty ACL).
  Exact bounded rows are in docs/evidence/macos-stock-directory-observations.json.
- This confirms a startup policy blocker on the hosted image; the two cases
  require distinct authority analysis before a rule is relaxed. No account IDs,
  names, paths, directory contents or private owner data are in the artifact.
  A diagnostic pass is evidence collection, not service acceptance.
- Windows jobs130590445341 (PR27) and130594256686 (PR30) passed and each logged
  the native hot-journal regression PASS after explicit fixture ownership.
  Their Ubuntu jobs130590445512 and130594255972, and PR31 job130592903706,
  each contain 52 mirror-DNS failure lines during Playwright dependencies.
  Required Ubuntu checks remain failed; no further retry or gate bypass occurred.
- Installed service, dedicated-account execution and physical hardware paths
  remain **unverified on real hardware**. Architecture review and the full
  milestone acceptance remain open.

## PR31 observation review follow-up

- Independent Anthropic / claude-opus-5-5 review passed6dda6e3 (requested high
  effort; the reviewer did not independently observe the effort setting). No
  blocking finding was raised. The retained source metadata now also names the
  immutable macos-26-arm64 image version20261102.0473.1, checked directly in the
  original job130592903303 setup log. Observation rows are unchanged.
- Each current_integrity_policy_accepts value checks that opened directory on
  its own; it does not establish a complete protected path or installed layout.
  The 0755 application's support group has no mode-bit write authority. The
  0775 runtime-parent group has write/execute authority with no sticky bit,
  allowing namespace entry creation/rename/removal; relaxing only the group-ID
  rule would correctly still refuse it. Neither fact alone justifies a policy
  relaxation. A dedicated macOS architecture correction remains in progress.
- This follow-up changes documentation/metadata only. No new native runtime,
  installed service or M2 acceptance is claimed. Hosted checks and the exact-head
  documentation recheck remain required before merge.

## Unix runner and macOS contract integration checkpoint

- Codex / gpt-6.1-sol, medium integrated candidatec7d7e85: bounded native
  manager queries, fresh gates/expectations, skip counting and root-required
  status. Builder formatting, Clippy, test compilation and 18 portable tests
  passed; six native Unix fixtures compiled as metadata but were not run.
  Timeout/nonzero mutation evidence is still outstanding. Unsupported
  StartPreflight/ProvisionState refuse during whole-plan validation.
- Anthropic / claude-opus-5-5, high supplied ADR0015; Codex / gpt-6.1-sol,
  medium independently reviewed26364b9 and accepted the bounded contract.
  Pass-through-only macOS ancestry can ignore a non-writing group, while leaves,
  Linux and Windows stay strict. The old writable runtime parent remains
  refused; the fixed endpoint moves beneath the existing traversable Arxcovo
  root. Created regular-file/directory group ownership is explicit, and socket
  ownership still follows the configured client. Primary XNU source checks,
  scope and remaining controls are in the retained review.
- This integration adds the reviewed contract and existing read-only observation
  fixture, not the macOS policy implementation. Owner preflight, retained-state
  provisioning and runtime-directory creation remain incomplete. No actual
  manager, owner or privileged operation ran; installed paths remain
  **unverified on real hardware**. PR29 remains a work in progress.


### ADR0014 phase 3b — bounded local implementation (2026-11-11)

- Provider/effort/outcome: OpenAI gpt-6.1-sol / medium; implemented locally,
  independent review pending. Prepared base verified exactly as
  `a24926310410239116b0bba0c888457d4c38109e`; no push/PR/merge or tracker write.
  Last merged PR remains as previously recorded; this slice merges nothing.
- `StartPreflight` now checks binary/private directories/config/state, accepts a
  missing endpoint directory, and validates any existing endpoint with the strict
  existing policy. Config bytes are bounded and parsed from the checked handle
  using `parse_inventory`; no workspace walk or PATH resolution is performed.
  Missing build execution facts remain Application execution-unavailable reasons.
  Service `observe_layout` still requires its endpoint directory.
- `ProvisionState` now accepts retained rollback databases and journals. All
  present members and WAL/SHM absence are validated before creation; lock must be
  empty. Only the shared native rollback-header predicate judges the first 20 DB
  bytes. Nonblocking flock, pinned lock identity and under-lock revalidation
  precede exclusive missing DB/journal creation. Created files and directory are
  synced. Existing files are never repaired, truncated, renamed or SQLite-opened;
  failures retain only fail-closed partial prerequisites for retry.
- Removed the unsupported-operation rejection for these implemented operations;
  the entire plan allowlist still runs before mutation. Runner gates, fixed
  vectors, identity requirements and strict ancestry are unchanged. No Windows,
  owner engine, SCM, CLI, namespace or endpoint-creator changes.
- Added three ordinary-user Unix owned-temp fixtures: retained/empty partial
  combinations preserve bytes/inodes/mode/ownership; WAL/SHM, malformed header,
  mode, symlink, hardlink, nonempty lock and held lock refuse without unexpected
  companion creation; preflight accepts missing/valid endpoints, refuses unsafe
  mode/symlink/file, requires binary/config/lock and uses a poison workspace
  resolver with a missing build account. Updated scripted whole-plan allowlist
  refusal. These NativeFs fixtures compiled but were NOT RUN on Windows.
- Actual checks passed: cargo fmt --all -- --check; workspace all-target Clippy
  with -D warnings; cargo test --workspace --no-run; portable owner tests (18),
  config tests (9), Application tests (5), shared header test (1); git diff --check.
  Linux x86_64 and macOS arm64 cargo check --tests and target Clippy --tests
  passed using LIBSQLITE3_SYS_USE_PKG_CONFIG=1 and checkout SQLITE3_LIB_DIR only
  as link hints. This is actual-source metadata compilation, NOT native linking,
  SQLite cross-build or execution proof.
- Actual mutation: shared header predicate changed to accept every header;
  service_refuses_wal_before_sqlite_can_create_unprotected_companions failed
  (exit 101). Source restored, same test passed. Unix-only mutations (reject
  missing endpoint, overwrite retained data, omit flock, privileged workspace
  observation) remain NOT RUN; their fixtures require an ordinary Unix runner.
  Phase 3a native ignored-timeout/nonzero mutations likewise remain outstanding;
  existing scripted command-failure halting and native ordinary-process tests
  compile, but scripted outcomes cannot prove native process effects.
- Remaining: execute owned-temp Unix fixtures/mutations; different-provider
  independent review; ADR0015 Mac ancestor/new-path/new-only-fchown endpoint
  creator slice; captured native launchd fixtures; installed-service acceptance.
  Mac startup stays blocked by the old strict ancestor/endpoint policy.
  All native manager/root-owned/installed paths are **unverified on real hardware**.
  No real manager, privileged harness, admin action, old Lab Manager or VM work ran.

## Unix preflight and retained-state coordinator integration

- Codex / gpt-6.1-sol, medium integrated candidatefc7aae4. Owner start now
  validates protected config/state and accepts only a missing endpoint directory;
  existing unsafe endpoint objects refuse. Config uses inventory parsing and a
  workspace-observation trap protects the privilege boundary. ProvisionState
  retains existing valid rollback-header DB/journal bytes, creates only missing
  empty prerequisites, takes the nonblocking lock and rechecks companions.
- Builder fmt, all-target Clippy, test compilation and33 relevant tests passed.
  Linux/macOS metadata and Clippy passed without native linking or execution.
  The header-bypass mutation failed and was restored. Filesystem/lock/workspace
  and runner-timeout/nonzero mutation execution remain outstanding on Unix.
- No Mac namespace/endpoint creation or installed-service acceptance is claimed;
  that reviewed correction is a separate isolated build. Real managers, LSA/SCM,
  account/right/task/service operations were not run. Native installed paths
  remain **unverified on real hardware**. Full independent implementation review
  and current-head hosted checks are still required.

## ADR0015 ancestor/runtime implementation candidate (2026-11-11)

- Provider/effort/outcome: OpenAI gpt-6.1-sol / high / scoped implementation
  complete locally from verified clean base a24926310410239116b0bba0c888457d4c38109e;
  native acceptance pending. Worked alone. Last merged PR unchanged (#28).
- Added Mac-only pass-through GID relaxation with unchanged ACL, UID, write and
  special-bit fences; Linux and all leaves stay strict. Fs and native traversal
  share the role policy. New directory/file handles get fchown before chmod;
  existing objects remain check-only. Config temporary files use the same helper.
- Fixed Mac endpoint moved per ADR0015. One generic-layout Mac service creator
  creates only missing run beneath its protected existing parent, checks EEXIST
  without repair, and pins descriptors before layout observation/binding. Linux
  creation remains with systemd. Socket ownership is configured client UID/GID0.
- Passed formatting, workspace/all-target Clippy, workspace test compilation,
  63 explicitly scoped service tests and seven local bridge tests. Linux/macOS
  actual-source all-target Clippy metadata checks passed with SQLite link hints;
  no Unix linking or runtime pass is claimed. Three pure-policy source mutations
  killed; two ownership mutations require native execution and remain unrun.
- Initial broad filter accidentally ran the existing production-refusal hook and
  ignored TokenOwner subprocess fixture via its parent test. This exceeded the
  requested boundary and was not repeated. The initial new layout test found
  runtime-directory overlap; corrected and all five layout tests passed. Exact
  selections, evidence limits and mutation obligations are in
  docs/briefs/0015-implementation-handoff.md.
- Hosted fixed-label read-only controls expanded to ten targets and now assert
  the required positives/refusals. No hosted execution here; historical evidence
  JSON is unchanged. Any native positive failure requires architecture handoff,
  with safe WIP retained and no ad hoc allowances. Opus review remains required.
- Phase 3b merge overlap: owner_unix Fs impl/tests and appended progress only;
  preserve strict leaf checks/new-owner helper. No StartPreflight/ProvisionState
  or manager changes, no duplicate endpoint creator. No push/PR/GitHub write,
  merge/watch, elevation, manager/install, old Lab Manager or VM operation.
- Native installed/cross-user/real hardware and dedicated-account M2 acceptance
  stay open and **unverified on real hardware**.

- Final portable ACL follow-up: included the existing pure native_macos_policy
  source in non-Mac test builds. All 66 scoped service tests passed, including
  its ACL absent/nonempty/unknown contracts. Final formatting, workspace/all-target
  Clippy, workspace test compilation and both Unix actual-source metadata Clippy
  checks passed again. Native runtime/ownership mutations remain outstanding.

## Integrated Unix owner and macOS namespace checkpoint

- Codex / gpt-6.1-sol, medium integrated the ADR0015 implementation candidate `546ca708` with retained-state/start-preflight head `7b346f1`. Only appended progress history conflicted; both histories were retained. The shared Fs code merged automatically and retains creation-only ownership plus StartPreflight/ProvisionState.
- On the integrated Windows tree, formatting and workspace/all-target Clippy with warnings denied passed. Full nextest passed **133/133**, with two subprocess-only fixtures skipped directly and exercised through parents. No owner operation, manager, account, rights or elevation action was performed.
- Hosted macOS positive controls and ordinary-user Unix fixtures remain required. Any failed stock-path positive control returns to architecture; no historical observation JSON was replaced. Native ownership mutations, macOS manager parsing, independent review and installed-service/dedicated-account acceptance remain outstanding and **unverified on real hardware**.

## Unix allowlist review regression coverage

- Codex / gpt-6.1-sol, medium closed the four allowlist-gate coverage gaps from the independent `1e1e48b` review. New portable tests enumerate every gate for Linux stop/disable and macOS bootout, reject foreign enable/disable targets, and allow conditional constants only for the Linux rights drop-in. Plan checks require conditional publication to appear only during install.
- All six focused portable owner tests, formatting and locked workspace/all-target Clippy passed. Each formerly surviving mutation now fails assertions: unrestricted stop gate, unrestricted disable gate, unrestricted bootout gate and conditional publication for another artifact. Source bytes were restored after every mutation. Production code before the test module is byte-identical to `1e1e48b`; no owner or service operation ran. Fresh exact-head recheck and hosted checks remain pending. This adds no macOS parser or installed-service/M2 acceptance.

## PR30 independent safety-guard coverage correction

- Anthropic / claude-opus-5-5, high reviewed a9327cf and found the pure engine
  correct but requested coverage for two backup safety guards. Codex /
  gpt-6.1-sol, medium added fresh-observation/stale-plan extraction checks in
  both presence directions, and direct unresolved-Intent uninstall refusals
  with and without the right present. Neither test runs recovery first.
- Both tests passed, then independently removing each production guard made
  its corresponding test fail at an assertion. Mutations were restored.
  Production engine bytes remain unchanged. The inherited parent hot-journal
  owner-fixture correction was integrated without loosening any policy.
- Corrected the guide: owner-record protection is checked by owner operations,
  not service startup. The old Journal integration heading is now explicitly
  superseded. Record-write errors after effect and the model's attribution
  wording remain a tracked review advisory; existing bounded model counts do
  not claim to cover every such write outcome.
- Fresh exact-head independent review and hosted Windows/Ubuntu gates remain
  required. No native store, LSA/SCM operation, owner activation or M2 acceptance
  is supplied by these test corrections.

- Combined correction validation: formatting and workspace/all-target Clippy
  passed, followed by 131/131 Windows nextest tests. Two subprocess-only fixtures
  were skipped directly and exercised by their parents. Both newly covered
  guards and the restored-production hot-journal regression passed.
## Service lifecycle refresh after merged CI unblock

- Codex / gpt-6.1-sol, medium refreshed #20 from reviewed head `4ead112` against actual main `3b6170b` (#32). The squash tree matches reviewed CI head `d6d3b31`. Only PROGRESS conflicted; both parents' provider tables and evidence were retained, Current state stays first, and last merged PR is #32. #31 is recorded closed as folded into #29.
- All service/crate, packaging, Cargo, toolchain and script bytes are unchanged from reviewed `4ead112`; CI workflow/helpers match merged main. Local formatting, locked workspace/all-target Clippy, **76/76 nextest tests** and **26/26 CI-helper tests** passed. The one direct-only subprocess fixture was skipped and exercised by its passing parent. No privileged service or owner operation ran. Fresh exact-head review and required hosted checks remain mandatory. Installed-service and dedicated-account M2 acceptance remain open.

## Main CI history after signed-mirror merge

- Codex / gpt-6.1-sol, medium prepared existing #26 locally on merged lifecycle main `64b50e9`; code, workflow and helper trees match main. Both historical duration sections and all provider table rows are preserved, with Current state first. No push or new feature PR.
- Added first-attempt main push CI run 43066365961 at `3b6170b`, success, using a fresh attempt API read: 2026-11-11T17:15:24Z to 2026-11-11T17:24:33Z = **549 seconds**. Earlier integration notes observed `updated_at` at 17:24:32 and recorded 548 seconds; the completed attempt API now ends at 17:24:33, so this history uses 549 seconds. Both are below 15 minutes. The 130-second signed-mirror Ubuntu job is a platform job duration, not the complete main CI duration.
- New lifecycle main run 43069045513 is not yet included because it is live. History remains evidence only, not an input to ci-time-budget. JSON/encoding and whitespace checks passed; independent review and hosted gates remain required. No policy, timeout, native/hardware or milestone-acceptance claim changed.

## Main CI after merged lifecycle

- Codex / gpt-6.1-sol, medium verified the completed first-attempt API for main CI run 43069045513, source `64b50e9187fd469bc886373230e324ace6dd4449`, success. Its run-start/update timestamps are 2026-11-11T17:31:49Z and 2026-11-11T17:41:00Z (**551 seconds**, below 15 minutes). Added this source-bound record without changing prior rows, policies or gates. The earlier queued/live entries are historical; this is the completion checkpoint. No installed-service, dedicated-account or M2 acceptance is established by CI. Existing #26 remains unpublished locally pending queue priority and independent review.
## Configuration queue preparation

- Codex / gpt-6.1-sol, medium prepared #21 locally on refreshed lifecycle head `3195f5a` while #20 awaits normal gates. No new feature PR was opened and this preparation has not been pushed. Crate/configuration, packaging, Cargo/toolchain and script bytes match reviewed `aadfa05`; CI workflow/helpers match the refreshed parent. Both provider/evidence tables remain intact and Current state stays first.
- Local formatting, locked workspace/all-target Clippy, **89/89 nextest tests**, **26/26 CI-helper tests** and three portable artifact checks passed. One direct-only subprocess fixture was exercised by its parent; two native Unix artifact checks skipped. No owner/admin/service operation ran. Publication waits for the actual #20 merge commit and a final metadata refresh; current-head hosted checks and independent review remain required. No installed-service/M2 acceptance is claimed.

## Main CI recovery confirmation

- Codex / gpt-6.1-sol, medium verified the Actions API for main run 43066365961: source `3b6170bbd07e38873deecbd7750432b74fa4cb87`, attempt 1, completed success. Using the repository policy's `updated_at - run_started_at` measure gives **548 seconds (9m08s)**. Main CI is green and under 15 minutes after #32. No rerun, timeout increase or gate exception was used. This record is prepared locally for the next dependency checkpoint; #20 remains the first service-stack merge target.

## Configuration refresh after lifecycle merge

- OpenAI / gpt-6.1-sol / medium integrated the actual #20 squash `64b50e9` into #21 without rewriting published history. The squash tree is identical to reviewed head `3195f5a`; duplicate-history merge conflicts retained the already validated configuration tree. Configuration source, packaging and Cargo/toolchain files remain identical to reviewed `aadfa05`; CI helpers/workflows match merged main. Prior local verification therefore applies: formatting, locked all-target Clippy, 89/89 nextest and 26/26 CI-helper tests passed; artifact checks had three passes and two native Unix skips. No implementation changed in this refresh. Fresh exact-head independent review and required hosted checks remain necessary.
- #20 merged at 17:31:46 UTC after all five gates passed; its Windows job completed successfully at 17:30:23. Main push CI for the earlier #32 unblock passed in 548 seconds. The new #20 main push is not yet claimed green. No new feature PR was opened, and Current state remains the first section.

## Unix executor queue refresh after configuration merge

- Codex / gpt-6.1-sol, medium prepared existing #14 on actual main `2bdcb20` (#21). Unix executor code, validation driver and ADR 0007 are byte-identical to published `9055ab0`; the combined manifest/library exports retain both executor and merged service foundations. Workflow conflict resolution keeps the pure Unix guards and both merged Linux CI-unblock steps; no original step or gate is removed. Both parent provider tables/evidence are retained and Current state is first.
- Actual Windows verification passed: formatting; locked workspace/all-target Clippy with warnings denied; **95/95 nextest** (one direct-only fixture exercised by its parent); **29/29 pure Unix driver guards**; **26/26 CI-helper tests**. Nextest run `e7dfa442-0229-4139-895d-c1599297095e`, 7.438-second test phase. No native Unix linking/runtime, root cgroup experiment, admin/account/service or VM action ran. No push/new PR; fresh independent review and hosted gates remain required. Installed/native dedicated execution remains **unverified on real hardware** and M2 acceptance stays open.
## Native validation queue preparation

- OpenAI / gpt-6.1-sol / medium prepared #25 locally on configuration head `9f74919` while #21 runs its fresh review and hosted gates. Published history is preserved. Only PROGRESS conflicted; the current state from the refreshed parent remains first, and both parent provider tables and task evidence are retained. Native source, packaging, Cargo/toolchain and scripts remain byte-identical to independently reviewed `d045cc4`; CI helpers/workflows match the configuration parent.
- This refresh ran formatting, 26/26 CI-helper tests and artifact checks (three passed, two native Unix skips). Clippy/nextest were not repeated because the Rust trees are identical to the reviewed head. No native service, owner, admin or account operation ran. No push or new feature PR; publication waits for #21 to merge, then a final refresh and exact-head independent review. Installed-service/dedicated-account M2 acceptance remains open and **unverified on real hardware**.

## Application queue preparation

- OpenAI / gpt-6.1-sol / medium prepared #27 locally on the refreshed #25 branch while #21 waits for its exact-head review and hosted gates. Only PROGRESS conflicted; Current state remains first and both provider/evidence tables are preserved. The entire crate, packaging, Cargo/toolchain and script trees are byte-identical to published Application head `a08187c`; the no-service-workspace preflight, typed refusal mapping, parent-sharing correction, hot-journal recovery and explicit test-owner fix are preserved. CI helpers/workflows match the prepared native-validation parent.
- Formatting and 26/26 CI-helper tests passed on this integration. Rust tests were not repeated for this source-identical history/CI refresh. A fresh full independent Application review remains required after its parents land; prior review blockers must not be treated as cleared by equivalence alone. No push, new feature PR or privileged operation. Main CI run 43069045513 on merged lifecycle `64b50e9` is queued, not claimed green. Installed services, dedicated execution and M2 acceptance remain open and **unverified on real hardware**.

## Hosted macOS namespace controls

- Codex / gpt-6.1-sol, medium inspected completed macOS job [130636095317](https://github.com/Dytschgo/arxcovo/actions/runs/43053335906/job/130636095317), source `b052d940`, image `macos-26-arm64` version `20261102.0473.1`. Root, Library, Application Support, private, var, Logs, LaunchDaemons and bin passed ancestor policy; launchctl passed binary-leaf policy. The old group-writable runtime parent and Application Support as a strict leaf remained rejected. Exact source-bound rows are retained separately in `docs/evidence/macos-namespace-controls.json`; the earlier stock-directory evidence remains unchanged.
- Hosted macOS nextest passed **132/132**, one direct-only fixture skipped. Ordinary-user native fixtures exercised manager scripting/gates, bounded ordinary-process timeout/output, start preflight, retained-state preservation, private publication, locking and new-object group ownership. No real service-manager mutation was invoked by these fixture tests. This is native ordinary-user evidence, not installed-root/service or cross-user acceptance.
- Remaining: independent full PR29 review, Linux hosted result, native ownership/timeout mutations, captured launchd manager-output parsing, installed protected SQLite and service lifecycle acceptance. The latter paths remain **unverified on real hardware**.

## Unix owner queue preparation

- Codex / gpt-6.1-sol, medium prepared existing #29 locally on refreshed Application `4f29ecf`, without changing the published head or opening a PR. All crate, packaging, Cargo/toolchain and script bytes match `40ab2f1` (the prior reviewed production code plus the coordinator gate-test follow-up). The only workflow additions are the two merged Linux CI-unblock steps; the macOS stock-control step is preserved. Helpers match the prepared parent. Only PROGRESS conflicted; both provider tables and historical task entries remain, with Current state restored first.
- Formatting, 26/26 helper tests and artifact checks (three passed, two native skips) passed. Native Unix tests and Rust suites were not repeated for unchanged source. A fresh exact-head recheck remains required after parent merges, including the portable gate-test follow-up. macOS manager parsing still refuses Unknown and CLI activation remains absent. Installed/native service and dedicated-account M2 acceptance remain **unverified on real hardware**.

## Windows owner adapter prerequisite handoff — 2026-11-11

- Provider/effort: Codex / gpt-6.1-sol high, implementation sub-agent working
  alone from `1a45ba15640b608ee4c953a104c06e451a85d07b`, branch
  `t3/windows-service-owner-adapter`. Outcome: partial transaction prerequisite,
  **native adapter task incomplete**. Last merged PR and milestone acceptance
  remain unchanged. No push, PR, merge, watch or delegation.
  Implementation commit: `e919c4d`.
- Added an opaque Windows configure-plan extractor and injected durable-intent/
  grant-receipt/publication recovery engine. Tests cover operation ordering,
  stale observations, ambiguous LSA/receipt errors, compensation and retry,
  publish-then-error, foreign-config conflict, existing-right preservation and
  bounded strict journal parsing. Native `Plan::execute` remains unsafe for
  configure: do not activate it.
- Concrete handoff obstacle: a crash between LSA grant and durable receipt leaves
  ownership ambiguous. Presence of the right cannot prove who granted it. The
  candidate refuses that state and preserves evidence; native recovery policy
  and durable uninstall provenance need other-provider architecture/security
  review. Exact API, integration requirements and all unimplemented native work
  are documented in service-foundation.md. This is not an ADR amendment.
- Actual checks: nine new tests passed; 16 combined owner-plan/transaction tests
  passed; four config and two layout tests passed. `cargo fmt --all -- --check`,
  `cargo test --workspace --no-run`, and workspace/all-target Clippy with
  `-D warnings` passed. Two safely
  restored guard mutations failed assertions. Full workspace runtime tests are
  excluded because existing tests invoke service dispatch/owner arguments.
  No privileged, service, dispatch, installation, account/right mutation, settings,
  provisioning, owner admin step or old Lab Manager/VM operation ran.
- Pending: native owner_windows adapter, protected file publication and journal
  storage, SCM/LSA calls and uninstall provenance, independent Opus review and
  installed-service acceptance. All native paths are **unverified on real hardware**.

- Codex / gpt-6.1-sol, medium closed the fresh-target identity test gap from the independent PR33 review. The new native owned-temp test stages an update, renames the old target, creates a distinct protected file with identical bytes, and asserts that stale publication conflicts before replacement. It covers both owner-record and config targets, preserved target identities/bytes, staging cleanup and a fresh successful transaction as positive control.
- Removing only the production fresh-target identity comparison made the test fail at its intended conflict assertion. The original production bytes were restored exactly. All 14 owner-store tests, formatting and workspace/all-target Clippy with warnings denied passed. Production behavior is unchanged; hosted checks and a fresh independent exact-head recheck remain required.
- Production ancestor pins exclude write sharing as well as delete sharing for the duration of an owner command; this can temporarily interfere with other writers to shared ancestors. Native production constructor, actual LSA/SCM, installed ownership and M2 acceptance remain **unverified on real hardware**. Raw session parts must still be narrowed before CLI activation.

## Windows SCM contract alignment

- Anthropic / claude-opus-5-5 / requested high authored `7e4cf06a`: ADR0012 SCM amendment and PR-C brief align demand-start install and last-command boot intent with accepted ADR0014. OpenAI / gpt-6.1-sol / medium independently reviewed both changed documents and recorded `docs/reviews/windows-scm-contract-review.md`. No blockers remain after clarifying native-call deadline limits, accepted stop-control handling, failure-test exceptions and fresh-install ordering.
- The adapter is not implemented yet. Pure/fake and synthetic-buffer implementation tests are specified; no actual SCM call, service, LSA, account, privilege, elevation or owner setup ran. Installed behavior and M2 acceptance remain **unverified on real hardware**.

## ADR0012 PR-C Windows SCM library implementation (local)

- Provider/effort/outcome: OpenAI / gpt-6.1-sol / high, implementation alone from
  verified `24d97e16c10a14d4686c6aa6b5b50964916d2eab`. Implementation commit
  `1c91ed9900cd40726c99c783eedc15787038971b`. Last merged PR is unchanged;
  this task made no push, PR, GitHub write, merge or watch.
- Complete: generic lifecycle/classification/status engine; opaque consuming
  deregistration proof; narrow Windows FFI with RAII handles and checked fixed
  aligned 8 KiB queries; portable stateful fake SCM/clock and synthetic native
  structure tests. Install/start/stop persistence follows the revised contract.
  layout::Error additions match object `1e1e48b` exactly; no Unix implementation
  source, session/parts, provenance engine, CLI/MCP or Plan::execute changed.
- Routine API visibility adjustment: WindowsScm is `pub(super)` to respect its
  private ScmCalls/Clock bounds. This narrows visibility without adding authority;
  the later wiring slice can export a concrete facade. No architectural deviation.
- Passed locally on Windows (exact commands):
  - `cargo build --workspace --all-targets --locked`
  - `cargo fmt --check`
  - `cargo clippy --workspace --all-targets --locked -- -D warnings`
  - `cargo nextest run -p arxd service::owner_scm --locked`: 18 passed.
  - `cargo nextest run --workspace --locked`: final implementation 163 passed,
    two existing subprocess fixtures skipped directly and exercised by parents;
    nextest test execution 9.097 s. This is local evidence, not hosted CI timing.
- Ten actual implementation-source mutations compiled and failed assertions:
  auto creation, missing auto write, early demand stop, disabled identity,
  1077 mapping, two recovery actions, case-folded image, ignored delete error,
  removed deadline, and allowed start-pending registration. Originals restored.
  Full failure-test names: `docs/reviews/windows-scm-mutation-evidence.md`.
- The first targeted run exposed two harness errors (proof declaration counted
  as construction, accepted 1056 incorrectly treated as an immediate halt); both
  corrected. The mutation driver initially doubled CRLF and rejected its second
  mutation at compilation; line handling corrected, then all ten compiled and
  failed tests. Initial Clippy collapsible-if warning corrected. No guard weakened.
- Blocked check: `cargo check -p arxd --tests --target
  x86_64-unknown-linux-gnu --locked` failed in bundled SQLite's C build because
  `x86_64-linux-gnu-gcc` is unavailable. No Unix metadata compilation pass or Unix
  runtime test is claimed. Unix-host pure tests remain required.
- No adapter test constructed the native adapter or made any real SCM call,
  including manager-open/read-only queries. No owner/admin/LSA/account/privilege
  operation, installation, dispatch or old Lab Manager/VM work was performed.
  Normal workspace tests retain the existing safe entry-refusal and parent-owned
  current-user fixtures; ignored fixtures were never run directly.
- Remaining: independent Opus implementation review and hosted required checks;
  PR-D proof consumption/session-parts narrowing, fresh protected-state
  provisioning and CLI wiring; separately reviewed disposable native experiments.
  LocalSystem/image readback, demand delayed-flag support/preservation, 1077,
  recovery/delete races, marked-for-delete semantics, privilege ordering/buffer
  sufficiency, real reboot persistence and privilege sufficiency remain
  **unverified on real hardware**. Poll deadlines cannot interrupt a blocking RPC.
  No installed acceptance is established; M2 remains open.

## Windows SCM coordinator checkpoint

- Codex / gpt-6.1-sol, medium verified candidate `6786a6f` on the Windows owner-store stack. Formatting, workspace/all-target locked Clippy and all 18 focused SCM tests passed. Native source is library-only; main/CLI routing is unchanged and no real SCM API call ran. The builder separately reported 163 normal workspace passes and ten restored source mutations caught by assertions. The coordinator did not repeat that full suite in this checkpoint.
- Architecture has the separate Sol cross-provider review in `docs/reviews/windows-scm-contract-review.md`. Implementation still requires independent Opus review and hosted checks. The distinct-handle marker failure observed on the separate executor-interface branch remains under diagnosis; no fix or global full-suite reliability claim is made here. Installed behavior, root provisioning, real SCM readback/boot semantics and M2 acceptance remain **unverified on real hardware**.
## Windows provenance queue preparation

- Codex / gpt-6.1-sol, medium prepared existing #30 locally on refreshed Application head `4f29ecf`. Provenance/config/plan source, packaging, Cargo/toolchain and scripts remain byte-identical to independently reviewed `4b2b769`, including both stale-plan directions and direct unresolved-Intent uninstall tests. CI helpers/workflows match the prepared Application. Only PROGRESS conflicted; historical task evidence and all provider rows are preserved. The older prerequisite-handoff section was moved below Current state, restoring the required first section.
- Formatting and 26/26 CI-helper tests passed. Rust suites were not repeated for a source-identical history/CI update. No push, new feature PR or native owner/admin operation. Publish only after the Application parent lands, then refresh metadata and obtain a fresh exact-head independent recheck. Native provenance/LSA and installed dedicated-account M2 paths remain **unverified on real hardware**.

## Windows owner store and SCM queue preparation

- Codex / gpt-6.1-sol, medium prepared existing #33 on the refreshed #30 branch, then existing #35 on that owner-store branch. Neither head was pushed and no new PR was opened. Source/packaging/Cargo/toolchain/script equivalence is exact against published owner-store `d6df821` and SCM `54c43fb` respectively; CI helpers/workflows match their refreshed parents. Only PROGRESS conflicted; provider rows and historical task evidence are preserved and Current state is first.
- Formatting and 26/26 CI-helper tests passed on the combined SCM tree. Rust/SCM suites were not repeated for unchanged source. Full SCM independent review remains required; the owner-store fresh identity-test follow-up requires its own exact-head recheck after its parents land. Publication stays in dependency order after #25/#27/#30. No actual SCM/LSA, production owner session, privilege or admin operation. Native installed-service/dedicated-account M2 acceptance remains **unverified on real hardware**.
## Native validation refresh after configuration merge

- Codex / gpt-6.1-sol, medium aligned existing #25 with actual configuration squash `2bdcb20`, whose tree is identical to reviewed `9f74919`. Duplicate-history conflicts retained the already validated native-validation tree; the crate, packaging, Cargo/toolchain and script trees remain byte-identical to independently reviewed `d045cc4`, and CI workflows/helpers match merged main. All published history is preserved. No Rust implementation changed. The local preparation passed formatting, 26 helper tests and three portable artifact checks (two native Unix skips); source-equivalent prior Rust verification remains applicable, without claiming it was rerun now.
- #21 merged at 17:46:19 UTC with all five gates green. #20 main CI completed successfully in 551 seconds. #25 requires a fresh exact-head independent recheck and all required hosted checks before merge; the full Application review is running separately on its unchanged public source. No new feature PR or privileged operation. M2 installed-service/dedicated-account acceptance remains open and **unverified on real hardware**.

## CI history checkpoint after native validation merge

- Codex / gpt-6.1-sol, medium aligned existing #26 locally with actual main `63b6555` without pushing. Code, workflow and helper trees match main; Current state records actual #25 merge first. All prior 21 history rows remain unchanged. A bounded API list of main push CI runs followed by each new first-attempt endpoint added 2 completed record(s): run 43071362527 at `2bdcb20`, 490 seconds, success; run 43073378419 at `63b6555`, 414 seconds, success. Live runs are excluded; the history still does not control the trusted budget gate. JSON/encoding/whitespace checks passed, no workflow/policy/timeout changed. Fresh independent review and required hosted gates remain pending; no installed/native M2 acceptance.
## Application review blocker and parent alignment

- Anthropic / claude-opus-5-5, high independently reviewed the full public PR27 range `d045cc4..a08187c`: 117 normal nextest tests passed, two direct fixtures were exercised only through their parents, formatting/Clippy passed, and 10 of 11 guard mutations were caught. Review failed on one confirmed blocker: long-lived read-share-only ancestor handles deny unrelated renames in shared Windows directories. Public evidence: https://github.com/Dytschgo/arxcovo/pull/27#issuecomment-6803252091. This status is not waived.
- Codex / gpt-6.1-sol, medium aligned the local Application with published parent #25 `7af3eac`; only PROGRESS conflicted and all source remains identical to reviewed `a08187c`. A separate Sol high worktree is fixing the ancestor-sharing blocker with owned-temp regressions; no implementation result is claimed yet. The lower-severity pipe casing, SYSTEM-check coverage, state-protection exit mapping, historical macOS endpoint creation and stale-doc findings remain tracked for follow-up. No new feature PR or privileged operation.
- #25 hosted Ubuntu, Windows, macOS and budget gates have passed; its new-head agent review remains pending. The parent must merge normally before Application publication. Native installed-service and dedicated-account M2 acceptance remain **unverified on real hardware**.

## Application alignment after native validation merge

- Codex / gpt-6.1-sol, medium verified #25 merged as `63b6555` at 17:59:09 UTC through all five gates. Its tree equals independently reviewed `7af3eac`. The local Application already contained that exact parent; duplicate-history squash conflicts retained the current Application tree. Crate/packaging/Cargo/toolchain/script bytes remain identical to public `a08187c` pending the separate sharing fix. No source fix or new acceptance is claimed by this alignment.
- Main configuration run 43071362527 was independently read from its first-attempt API, success, 2026-11-11T17:46:22Z to 2026-11-11T17:54:32Z = 490 seconds. Current state records the actual latest merge first. PR27 remains blocked by its review finding until the sharing fix, fresh independent review and hosted gates pass. No push/new feature PR or privileged operation; installed-service/dedicated-account paths remain **unverified on real hardware**.

## PR27 blocking ancestor-sharing correction — local candidate

- Provider/effort/task/outcome: OpenAI / gpt-6.1-sol / high; implement only the confirmed retained-ancestor sharing blocker from the [Opus review](https://github.com/Dytschgo/arxcovo/pull/27#issuecomment-6803252091). Prepared base `4f29ecf843c01d82fe38da16e8ef42e0b53a7611` on parentpreparedApplication (crate source identical to public `a08187c1e14a5722d4c37bfc9f2690d273d62eb3`). Local implementation complete; independent review/hosted gates pending. Current state and last merged PR above are unchanged; no acceptance claim.
- Only production change: service `native_windows::open_impl` adds FILE_SHARE_WRITE for non-leaf ancestors, preserving FILE_READ_DATA, retained handles and no FILE_SHARE_DELETE. Immutable final leaves still exclude writers; mutable DB/journal/lock leaves still share read/write without delete sharing. Structural, owner, DACL and namespace checks are unchanged. Owner Windows and development executor source/policies, other protected source and the separate handle-marker patch are untouched. ADR0013 and service-foundation now describe ACL namespace authority and retained nondelete handles accurately, without claiming pins freeze all modification.
- Regression evidence: nested owned outer temp directory, using existing test-only `open_state_fixture` and production `open_impl`, never fixed installed paths or a production constructor. Before the fix, unrelated sibling-file rename failed with error 32. After the fix, sibling file/folder renames and file/folder moves into a pinned ancestor succeed; protected next-directory rename and DELETE access refuse; mutable leaf writes succeed; delete access, rename and cleanup succeed after drop. Immutable final leaf rejects writers while pinned and permits them after drop.
- Required source mutations applied individually and restored: old read-only ancestor sharing failed the intended unrelated-sibling rename assertion (error 32); adding ancestor FILE_SHARE_DELETE failed the protected-directory DELETE-access assertion (unexpected successful open). Neither failure was a compiler failure. Restored candidate passed the four focused ancestor/immutable/mutable/hot-journal tests.
- Checks actually run: `cargo fmt --all` and `cargo fmt --all -- --check` passed; `cargo clippy --workspace --all-targets --locked -- -D warnings` passed; `cargo nextest run --workspace --locked --no-fail-fast` passed **118/118**, with 2 ignored subprocess fixtures skipped and exercised only through normal parents. Real hot-journal recovery/pin tests remain passing. Known unrelated ETW/handle-marker flake did not occur; no repeat or weakening was needed.
- Limits/remaining gates: fresh independent Opus review and green required hosted checks remain mandatory. Case-sensitive pipe check, Windows SYSTEM-guard test gap, macOS endpoint creation, state-file exit-code mapping and unrelated stale docs remain tracked nonblocking review follow-ups, outside this change. Native installed SYSTEM/service execution remains **unverified on real hardware**. No SYSTEM/root execution, SCM/LSA/account/rights/privilege/install/owner action, Lab Manager/VM operation, push, PR mutation, merge, watch or subagent was performed. Public review was read only; all changes remain local.

## Application sharing correction coordinator integration

- Codex / gpt-6.1-sol, medium integrated Sol high candidate `7544fa9` into the Application after actual native-validation merge `63b6555`. The complete crate/packaging/Cargo/toolchain/script trees match the tested candidate exactly; workflows/helpers match merged main. Only PROGRESS conflicted and both histories/provider rows remain. The production change is limited to ancestor FILE_SHARE_WRITE while keeping no-delete pins and strict final-leaf sharing; namespace authority still comes from the owner/DACL policy.
- Builder evidence: the new sibling file/directory rename and move-in regression failed before the fix; old ancestor read-only sharing and added ancestor delete-sharing mutations failed intended assertions and were restored. Formatting, locked all-target Clippy and **118/118 normal nextest tests** passed, with both subprocess-only fixtures exercised through their parents. Coordinator reran formatting and equivalence/whitespace checks; it did not claim a second Clippy/nextest run. Fresh exact-head Opus re-review and hosted gates remain required. The lower-priority prior advisories stay open. No new feature PR, actual service/SYSTEM/owner/admin/SCM/LSA or VM operation; installed and dedicated-account M2 paths remain **unverified on real hardware**.

## Unix owner integration with corrected Application sharing

- Codex / gpt-6.1-sol, medium integrated published Application `0232a80` into the local #29 preparation. Unix owner/query/plan source remains byte-identical to `40ab2f1`; the parent contributes the reviewed-sharing correction candidate and its native test. Only PROGRESS conflicted; histories/provider rows are retained and Current state is first. No push/new PR or actual owner/admin/service/VM operation.
- Formatting and locked workspace/all-target Clippy passed. The first combined nextest run (`d9dd5850-1962-4854-ae90-d68585ff5edd`) had **133 passed, 3 failed, 2 skipped**: development actual-job test at cancel returned ExecutionFailed, junction/handle-count loop returned ExecutionFailed, and the real bridge execution positive control did not observe its child. This is not the previously diagnosed distinct-handle-marker symptom and no cause or fix is claimed. Subsequent helper/artifact commands did not run after that failed chained command.
- Debug feedback with unchanged source: three complete normal nextest reruns each passed **136/136**, two direct fixtures skipped and exercised by parents (8.763 s, 11.004 s, 9.229 s). The failure is intermittent and not reproduced at a stable rate; diagnosis stops before hypotheses/instrumentation. No speculative production change, test skip, timeout increase or serialization was applied. Retain the initial failure as unresolved and investigate if it recurs; these repeats do not prove a repair.
- Separate helper/artifact checks now passed: 26/26 CI-helper tests; three portable artifact checks with two native Unix skips. Fresh independent exact-head review and hosted gates remain mandatory after the Application parent lands. Native Unix, installed services and dedicated-account M2 acceptance remain **unverified on real hardware**.

## Owner provenance refresh after Application merge

- Codex / gpt-6.1-sol, medium integrated reviewed Application head `0232a80` and actual squash `3f7c515` into existing #30 without rewriting published history. The squash tree equals that reviewed head; duplicate-history conflicts retained the already combined tree. Owner transactions/tests, owner plan and service config remain byte-identical to independently reviewed `4b2b769`; the parent contributes the sharing correction and regression. Workflows/helpers match merged main. Both provider tables and historical evidence are retained, with Current state first.
- Actual Windows validation passed: formatting, locked workspace/all-target Clippy with warnings denied, **132/132 normal nextest** (two subprocess-only fixtures exercised through parents), and **26/26 CI-helper tests**. Nextest run `a7b71b17-8b59-43e2-8ac5-ef74ec38095e`, 8.445-second test phase. No native owner session, LSA/SCM, account/rights/admin/service or VM operation. #27 merged through all gates at 18:13:01 UTC. Fresh exact-head independent review and hosted checks remain required; native/installed dedicated-account M2 paths remain **unverified on real hardware**.

## Early feasibility and owner-checklist queue preparation

- Codex / gpt-6.1-sol, medium prepared existing #10 locally on main `4f2b0c7`, retaining its unique proposed WHPX, Windows template, Raft and execution research plus the required pre-M4 owner setup guide. ADR0006 is explicitly historical and points to settled ADR0009 and the separate authorized ADR0010 development mode; the guide no longer suggests a password prompt absent from the current S4U contract. No research proposal is adopted or hardware result claimed by this metadata update.
- Runtime crate, workflow, tool, packaging, script, Cargo and toolchain trees match main. Encoding, relative-link and whitespace checks passed for the five retained documents. No Rust suite was run for docs-only changes, no push/new PR, owner/admin/account/service/VM operation or new architecture/implementation. Independent review and hosted gates remain required. Original hardware probes and installed/dedicated-account M2 acceptance remain **unverified on real hardware**.
## Intermittent development execution diagnostic checkpoint

- Anthropic / claude-opus-5-5 / requested high could not reproduce `ExecutionFailed` in 124 isolated runs and three reduced-concurrency arxd suites; CPU-starvation timeout failures were kept separate. No cause or fix was established.
- Codex / gpt-6.1-sol / medium then ran ten bounded default-concurrency full-workspace diagnostic passes at `f15a271`: nine passed 150/150; one failed only the older handle-slot reuse test. Zero `ExecutionFailed` occurrences or diagnostic error-stage lines. Original source bytes were restored; no instrumentation is committed. Details are in `docs/reviews/development-execution-failure-observations.md`. Passing reruns are not a repair or acceptance claim. Current-parent integration, independent review and hosted gates remain required; installed/dedicated M2 paths are **unverified on real hardware**.
## Owner store integration with refreshed provenance

- Codex / gpt-6.1-sol, medium prepared existing #33 locally on published provenance `bc3443f`, preserving published history and the Application sharing correction. The owner store, its tests and Cargo dependencies remain byte-identical to `d6df821`; native parsing exports and the new parent sharing test merged cleanly. Current state remains first and provider/task evidence is retained. No push or new feature PR while #30 runs normal gates.
- Actual Windows validation passed: formatting, locked workspace/all-target Clippy with warnings denied, **146/146 normal nextest** and **26/26 CI-helper tests**. Two subprocess-only fixtures were skipped directly and exercised through parents. Run `f820eda1-f85a-4d0e-9d33-d677b5cb35a6`, 14.628-second test phase. The earlier intermittent #29 combination failures are not claimed fixed by this separate passing run. No production owner session, SYSTEM layout, LSA/SCM/account/rights/admin/service or VM operation. Fresh exact-head independent recheck and hosted gates remain required after the parent merges; production installed/dedicated-account M2 paths remain **unverified on real hardware**.

## SCM integration with validated owner storage

- Codex / gpt-6.1-sol, medium prepared existing #35 on local owner-store `34a8f92`, carrying the reviewed Application sharing correction and current provenance parent without altering the SCM implementation or its tests. Published history is retained; only PROGRESS conflicted and both provider/evidence histories remain. No push or new feature PR.
- Actual Windows verification passed: formatting, locked workspace/all-target Clippy with warnings denied, **164/164 normal nextest** and **26/26 CI-helper tests**. Two subprocess-only fixtures were skipped directly and exercised through parents. Run `ecd8ad22-26be-48f4-b5ea-22ff223d3007`, 10.406-second test phase. SCM tests use fake calls and synthetic buffers; no production SCM constructor or actual SCM/LSA/account/rights/admin/service/VM operation ran. Full independent SCM review, publication after #33, hosted gates and installed evidence remain required. Real service registration, delayed start, reboot and dedicated-account M2 acceptance remain **unverified on real hardware**.
## Protected owner store refresh after provenance merge

- Codex / gpt-6.1-sol, medium aligned existing #33 with actual provenance squash `4f2b0c7`, whose tree equals independently reviewed `bc3443f`. The local store already contained that exact parent; duplicate-history conflicts retained the validated combined tree. Complete crate/packaging/Cargo/toolchain/script trees are identical to tested `34a8f92`; owner_windows source/tests and dependencies remain unchanged from `d6df821`. Workflows/helpers match merged main. All published history and provider/task evidence remain, with Current state first.
- Existing refreshed-tree verification applies: formatting, locked all-target Clippy, **146/146 normal nextest** (two subprocess fixtures through parents) and **26/26 CI helpers** passed. No test rerun is claimed for this identical-tree metadata refresh. Fresh exact-head independent recheck, including the d6df821 same-content identity-swap test follow-up, and hosted gates remain required. #30 merged at 18:24:31 UTC through all five gates; first-attempt Application main CI passed in 571 seconds. No native owner constructor, LSA/SCM/privilege/account/admin/service/VM operation. Installed/dedicated-account M2 paths remain **unverified on real hardware**.

## SCM alignment after protected owner-store merge

- Codex / gpt-6.1-sol, medium aligned local #35 with published store `6eb4b9e` and actual squash `7c150f0`, whose trees match. The SCM branch already contained the exact reviewed store source; duplicate-history conflicts retained the validated combination. Crate/packaging/Cargo/toolchain/script trees remain byte-identical to tested `ebe081e` (164 normal tests passed); workflows/helpers match main. No repeated tests are claimed for this metadata-only alignment.
- #33 merged at 18:34:47 UTC through all five gates, including independent identity-swap mutation coverage. Main provenance run 43077489596 passed on its first attempt in 535 seconds. Current state records the actual merge first. The public SCM head stays frozen at `54c43fb` during its full independent Opus review; no push/new PR or privileged action. A final refresh/recheck and hosted gates remain mandatory. Real SCM readback/registration/reboot and installed/dedicated-account M2 acceptance remain **unverified on real hardware**.
## Test-only occupied-handle CI correction publication

- Codex / gpt-6.1-sol, medium integrated previously reviewed local `5b2e865` onto actual main `7c150f0`. Anthropic / claude-opus-5-5, requested high authored the diagnosis/patch; the independent Sol patch review is retained in `docs/reviews/windows-unduplicable-slot-review.md`. A valid child ETW registration can refuse duplication with error 50; treating that occupied slot as absent skips the distinct-object marker. The independent SCM review observed this known failure on its old hosted head, so the correction is a CI unblock, not a new feature PR. It does not claim to fix the separate intermittent ExecutionFailed symptoms.
- All source changes are inside the existing test module and byte-identical to reviewed `5b2e865`. Production code, three-handle inheritance, Job/token checks, dependencies, workflows and timeouts are unchanged. The deterministic unsupported-slot fixture and real-leak countercontrols remain. Actual refreshed Windows verification passed formatting, locked all-target Clippy and **147/147 normal nextest**, with two subprocess-only fixtures exercised through parents; run `df181cad-0001-46ae-b0f8-bb99b0a31198`, 11.024-second test phase.
- This separate one-concern test PR is the narrow CI exception to the feature-PR pause; it does not resume feature publication and keeps the open queue below the original 16-PR cap. Fresh published-head integration review and all hosted gates remain required. No production owner/SCM/LSA/privilege/account/admin/service/VM operation; installed/dedicated-account M2 paths remain **unverified on real hardware**.

## Windows ETW handle-slot fixture correction
### Earlier local preparation

- Anthropic / claude-opus-5-5 / requested high diagnosed the missing marker: valid child ETW-registration slots can reject duplication with error 50; the fixture treated that as absent. The deterministic new seam failed 50/50 without the correction and passed 100/100 with it. The author reports 1000 parallel plus 300 sequential original-test passes and real-leak mutation detection; records are retained in `docs/reviews/windows-handle-reuse-marker-failure.md`.
- OpenAI / gpt-6.1-sol / medium independently reviewed and applied the test-only patch on main `0ceb174`. Personally reproduced the fallback-disabled failure and real handle-list-removal leak failure, restored all bytes, and passed formatting, locked all-target Clippy and **61/61 nextest tests** with zero skipped. See `docs/reviews/windows-unduplicable-slot-review.md`. Production behavior is unchanged; hosted checks and normal merge gates remain required. No installed-service or dedicated-account acceptance is claimed.

## Unix owner integration with merged protected storage and CI fixture

- Codex / gpt-6.1-sol / medium merged actual main `f06e9a2` into the local Unix owner candidate after recording the bounded diagnostic result. Preserved both typed Unix lifecycle modules and the Windows protected owner-store module; foundation and provider history retained. The merged PR37 test-only handle correction is included. No native authority or CLI activation is added. Combined checks and independent review remain required before publication; installed/dedicated M2 paths remain **unverified on real hardware**.

- Combined-tree validation at `06e8f68` passed formatting, locked workspace/all-target Clippy, **165/165 normal nextest**, **26/26 CI helpers** and artifact checks (**3 passed, 2 native Unix skips**). Two subprocess-only fixtures were exercised through their parents. Nextest run `b33a8813-53b7-48ec-897f-feea93182ccd`, 10.474-second test phase. No recurrence is claimed repaired by this passing combination. Fresh independent exact-head review and hosted gates remain mandatory; no native owner, SCM/LSA or privileged acceptance ran.
## SCM full review and readback-test correction

- Anthropic / claude-opus-5-5, high completed the full public SCM review at `54c43fb`: no blocking findings, 18 focused tests passed, six of seven mutations caught. The surviving mutation removed install demand-start readback; evidence is https://github.com/Dytschgo/arxcovo/pull/35#issuecomment-6804334081. The public head was frozen throughout that review. Architecture credit remains the independent Sol contract review, not Opus self-review.
- Codex / gpt-6.1-sol, medium added a fake manager that can report success without applying a start-type write. The new case requires registration to refuse the still-auto readback, then succeed on the same stopped registration once writes apply. Disabling only the real readback guard compiled and failed that exact new assertion (Ok versus expected Manager); production source was restored byte-for-byte. All 19 focused SCM tests pass. No production SCM behavior changed.
- Integrated merged test-only #37 into the validated parent stack and corrected the inherited progress-ordering regression: Current state is first, with exactly one queue-priority row; the earlier local handle preparation is grouped under its publication record. Main #33 run is now recorded completed at 532 seconds. ADR0012/SCM brief status is current; empty-privilege and exact-deadline native assumptions are explicit, not claimed fixed.
- Actual final validation passed formatting, locked workspace/all-target Clippy and **166/166 normal nextest**, with two subprocess-only fixtures exercised through parents; run `3cd2ff71-c6f1-4e4a-95b3-4d48702b3e62`, 10.155-second test phase. Existing ExecutionFailed diagnosis remains separate and unresolved. Fresh exact-head recheck of the strengthened test/integration and all hosted gates remain required. No real SCM/LSA/privilege/account/admin/service/VM operation. Installed/native dedicated-account M2 paths remain **unverified on real hardware**.

## Main CI duration history through merged SCM parent

- Codex / gpt-6.1-sol / medium refreshed existing #26 onto actual main `cd464b2` and retained every historical duration row. Added completed first-attempt main push CI evidence from the Actions attempt API: run 43075630346 at `3f7c515` = **571 seconds** (success), run 43077489596 at `4f2b0c7` = **535 seconds** (success), run 43079116241 at `7c150f0` = **532 seconds** (success), run 43082880556 at `f06e9a2` = **578 seconds** (success). The new #35 main run is still pending and is not added as completed.
- This JSON remains historical evidence, not a budget-policy input. No workflow, timeout, status gate, runtime or DNS behavior changes; no failures or earlier over-budget runs are removed. Independent exact-head review and required checks remain pending.
## Early feasibility documentation refresh on service stack

- Codex / gpt-6.1-sol / medium refreshed existing #10 onto actual main `cd464b2`, preserving the earlier proposed research and owner-only setup checklist. ADR0006 remains historical and points to the settled dedicated-service and separate development contracts; no password fallback, new authority or architecture adoption is introduced. Research documents and setup prerequisites do not establish hardware or M2 acceptance. Current state stays first; fresh exact-head independent review and hosted gates remain required.

- Checked the five feasibility/setup documents for valid UTF-8, balanced fences and resolving local Markdown links; whitespace checks passed. Runtime, dependency, workflow and tool trees match merged main, so no Rust or hardware run is claimed for this documentation-only refresh. The proposed M3/M4 designs remain research and require their milestone-specific decisions, implementation and acceptance.
## Unix executor refresh on merged service stack

- Codex / gpt-6.1-sol / medium refreshed existing #14 onto actual main `cd464b2` after the SCM merge. Executor source and Unix validation drivers remain unchanged from the published candidate; Cargo feature union preserves the merged service stack. Current state stays first, with no new feature PR. Combined verification, fresh independent review and hosted gates remain pending. No dedicated execution activation, owner operation or installed M2 acceptance is claimed; native privileged paths remain **unverified on real hardware**.

- Refreshed #14 combined validation at `1d8cf24` passed formatting, locked workspace/all-target Clippy, **172/172 normal nextest**, **29/29 Unix driver guards**, **26/26 CI helpers** and **3 artifact checks** (2 native skips). Two subprocess fixtures were exercised only through parents. Nextest run `84551cf7-d7c1-41ca-8fa1-216596171209`, 9.889-second test phase. The automatic Cargo merge duplicated the identical Unix dependency table; it was consolidated before these checks. Executor source remains byte-identical to published `9055ab0`; no privileged experiment or native acceptance ran.
## Unix owner refresh after SCM merge

- Codex / gpt-6.1-sol / medium incorporated actual #35 squash `cd464b2` while preserving published #29 history. Typed Unix gates and Windows provenance remain intact; shared error variants and owner modules merged without a source conflict. Independent exact-head review and hosted gates remain required. Native installed/service/dedicated M2 acceptance remains **unverified on real hardware**.

- Final SCM/Unix combined verification at `63af692` passed formatting, locked workspace/all-target Clippy and **184/184 normal nextest**, two subprocess fixtures exercised through parents; run `dc2e7bef-7883-46db-a3ef-2fb74bb6b122`, 10.435-second test phase. Existing helper/artifact bytes are unchanged from the preceding 26-helper and 3-artifact pass. Corrected the SCM brief to distinguish malformed single-NUL input from a valid double-NUL empty privilege list, matching the independent review. No production code changed for that wording correction.

## CI history alignment after Unix owner merge

- Codex / gpt-6.1-sol / medium incorporated actual #29 main `aafaa3c` into existing #26 and retained all earlier duration rows. Appended completed first-attempt Actions API records: run 43087250975 at `cd464b2` = **596 seconds** (success), run 43088994955 at `aafaa3c` = **461 seconds** (success). In-progress runs are not transcribed as completed. This is historical evidence only; budget logic, workflows and runtime remain main-identical. No application rerun is claimed for the documentation-only refresh.
## Feasibility notes after Unix owner merge

- Codex / gpt-6.1-sol / medium incorporated actual #29 main `aafaa3c` into existing #10. The five research/setup documents remain byte-identical to checked `2b935e6`; the inherited ADR0014/0015 contracts do not adopt any M3/M4 research proposal. Current state and both provider histories remain. No hardware or application rerun is claimed for this documentation-only refresh; independent exact-head review and hosted gates remain required.
## Unix executor alignment after owner lifecycle merge

- Codex / gpt-6.1-sol / medium incorporated actual #29 main `aafaa3c` into existing #14. The native owner lifecycle and macOS namespace stack is retained alongside the Unix execution library. The CI merge retains both Unix driver guard and macOS namespace observation steps with all original gates. Source executor/driver behavior is unchanged; no command route or privileged experiment is activated. Combined checks and a fresh exact-head recheck remain required.

- Post-#29 combined verification at `d8ee351` passed formatting, locked workspace/all-target Clippy, **190/190 normal nextest**, **29/29 Unix guard tests** and **26/26 CI helpers**. Two subprocess-only fixtures were exercised through parents. Nextest run `a273041d-e9c9-4cd8-a314-064143cf2eca`, 10.302-second test phase. Executor source remains byte-identical to the previous public head; CI preserves the main macOS observation and all gates. A fresh exact-head review remains required after this parent update. Native/privileged Unix execution remains **unverified on real hardware**.

## First reviewed disposable Linux execution dispatch

- Codex / gpt-6.1-sol / medium verified the #14 squash tree equals independently reviewed `9513b2d`, all five gates passed, and the selected main SHA was exactly `5b4f6b2`. Dispatched the guarded manual workflow once on a fresh GitHub-hosted Ubuntu runner: [run 43091339273](https://github.com/Dytschgo/arxcovo/actions/runs/43091339273), job 130768897109, attempt 1. This is separate from Windows S4U; no owner-PC operation occurred.
- Guard tests and unprivileged dependency fetch passed. The guard/stage/run/cleanup step failed with fixed code `E_PROTECTION`. No harness-pass or cleanup-confirmed line was observed; the exact refusal site and mutation phase are not established by this bounded output. No containment pass, fixture cleanup proof, installed-service or M2 acceptance is claimed. Do not reuse an unknown-cleanup environment or retry this probe without diagnosis and review.
- Cleared `UNIX_EXECUTION_REVIEWED_SHA` immediately after the terminal failure and verified the variable is absent. Sanitized evidence is in `docs/evidence/unix-execution-first-dispatch.json`; raw logs remain local. This experiment is not a main-CI duration row. Native and installed paths remain **unverified on real hardware**.

## Local Windows E1 narrowing handoff

- OpenAI / gpt-6.1-sol / high implemented only the reviewed local E1 scope on base `ce30060`, final `ff5a198`. Added guard/session wrappers, proof-gated uninstall and shared private pinning; removed raw parts and public concrete adapters. Reported formatting, locked workspace/all-target Clippy, 20 focused U/N/S tests, 190 normal nextest (two parent-driven direct fixtures) and four pin-call/checker mutations caught. Coordinator has not independently reproduced those checks. E2, publication and activation remain absent; fresh independent Opus review is required before publication/merge.

- Main #14 first-attempt push run 43091130349 completed successfully in **549 seconds**, verified against the Actions attempt API. Appended this as row 30 while preserving the prior 29 rows. The separately failed disposable Linux experiment is not a main-CI timing row. PR36 full review found three latent execution/audit blockers; Sol high is fixing them locally before any re-review or merge. No gate bypass or privileged probe retry.
## Feasibility documentation alignment with Unix library main

- Codex / gpt-6.1-sol / medium incorporated actual #14 main `5b4f6b2` with published history preserved. Five research/setup documents are unchanged from 9193e93 and remain proposed or historical; their runtime and hardware limits are retained. The added Sol report reviews the retained Opus research, not Sol own metadata edits. Current state is first, with one queue priority; independent exact-head review and hosted gates remain required. No native activation, privileged experiment retry or new feature PR.

## CI evidence after feasibility merge

- Codex / gpt-6.1-sol / medium incorporated actual #10 main `b5e6556`, preserving published history. All 30 timing rows and the separate failed Linux experiment record remain byte-identical to independently reviewed 31dc149. No evidence value, runtime, workflow, timeout or budget-policy change is introduced; earlier API and privacy verification applies. Current state remains first with one queue priority. Independent exact-head recheck and all required gates remain necessary; no feature PR, native activation or experiment retry.
