# Post-v1 roadmap

Milestones 1–6 remain the v1 scope in KICKOFF.md. Owner amendment 2026-11-23 ([ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md)): static VMs (first target an Ubuntu web server) and Cloudflare Tunnel publishing (off by default, owner credential) moved into v1. Further owner amendment the same day ([ADR 0064](adr/0064-v1-scope-docker-static-vm.md)): Docker Compose multi-service hosting on that static VM is in v1, with Calm Life as the first deployment and public-only outbound internet (the coordinator's interpretation of the owner's full-outbound grant). The patched QEMU/libslirp build (#292, `881e0966`) and the PublicEgress variant (#293, `e77d9512`) have merged. Guest Docker, the public-only egress fence, Windows custody and the live tunnel are **unverified on real hardware**. Later work, in order:

1. Linux hosts.
2. WireGuard overlay.
3. Backups and maintenance updates. Static VMs moved into v1 (ADR 0058). Docker Compose hosting on the static VM is also in v1 (ADR 0064), including only a minimal protected same-host SQLite backup of the project data. Broader backups (scheduled, off-host, restore tooling) and maintenance updates stay here. The agent no-delete fence for static VMs, their volumes and backups applies now.
4. Host board and warm migration.
5. Mac hosts and macOS guests.
6. Ephemeral GitHub runners.
7. Agent spawning and encrypted secrets vault.
8. Cloudflare exposure beyond the v1 tunnel publishing of ADR 0058.
9. Signing and public release.

## Known issues / later release

Non-blocking issues triaged under the owner's 2026-11-23 rule (ADR 0056). Each keeps its evidence.

- **Intermittent hosted C13 owner mint refusal.** PR #259 CI `45062459949` failed once in the hosted C13 activation campaign (`phase=join category=assertion message=mint-refused`, owner mint exit 8, clean teardown). It passed in all 19 later runs that executed it through `830b9de7` (one later run skipped C13). Cause undetermined; no v1 feature or acceptance check is blocked. If it reddens `main` again, quarantine that single case.
- **Hosted windows-2025 restricted PowerShell crash `0x80131623`.** Cause unknown. Diagnosis `D:/Temp/arx-m4-pr5a-stuck2/review.md`, SHA256 `0c2f675a…7aa3`, on PR #273. A test-only quarantine applies on GitHub-hosted `CI=true` when `ImageOS` starts with `win25`, for that exact code. Qualification on the owner host is required. The guest path stays **unverified on real hardware**.
- **Electron asset loader cannot detect Windows non-symlink reparse tags from Node.** P3 from PR #271 review 4 (`D:/Temp/arx-pr271-review4/review.md`, SHA256 `2fb5beb7…eb9f`). The M6 shell defers that check.
- **Vault JSON escaping may push a 4 KiB script past the 8 KiB local MCP request frame.** The stdio bridge refuses the oversized JSON request before vault validation. P3 from PR #269 review 2 (`D:/Temp/arx-pr269-review2/review.md`, current SHA256 `0511D63B…6086`; the verdict on `180d09e4` was recorded as `79b3231b…9276` before the artifact noted the refresh).
- **Quarantined `authenticated_sender_mismatch_and_assumed_founder_control` NotLeader.** `cluster::consensus::issue::tls_engine_tests::authenticated_sender_mismatch_and_assumed_founder_control` at `three_node.rs:222`. Red main `2bdcb657` (Ubuntu), red main `4a8e425f` (macOS, attempt 1 of `45148496421`, green on attempt 2), and red PR #292 CI `45246868970` (Ubuntu, green on attempt 2). PR #295 (`237f6b74`, agent-review status `59709693412`, review PASS `d29151e6…dab8`) quarantines that single case on every platform. Four `peer_transport` tests still cover the sender-mismatch refusal on every OS. The Engine fix hold remains. No v1 feature or acceptance check is blocked.
- **Intermittent macOS `r5_shutdown_read_owners`.** PR #294 CI `45264812377` failed in `cluster::consensus::three_node::r5_shutdown_read_owners` on macOS rust. The PR is docs-only, so the job was rerun. It is the same R5 family as the closed diagnostic PR #162. Quarantine that single case if it reddens `main` again. No v1 feature or acceptance check is blocked. The older Windows quarantine for this name stays in ADR 0024.
- **Server logout 204 clears the session cookie (P3 from #288).** After review PASS `3ed53d33…2382`, the logout that returns 204 writes `Set-Cookie` with `Max-Age=0` at `crates/arxd/src/dashboard.rs` lines 360–367. PR #288 (`ac6387a4`) already bound logout completion to the session generation and added the earlier-session 401 test with no session held. This server clear is the remaining follow-up.
- **Windows junction handle-count flake.** `development::windows::tests::junction_cwd_and_tool_refused_and_handles_do_not_leak` failed on the process-wide handle count during Cloudflare B2 Opus #2 Windows runs (297→326, 285→329, 287→312, 310→331, and 310→329). Each of those failures passed when the test was run alone. One isolation run took 9.5 s. Evidence: `D:/Temp/arx-cf-b2-opus2/report.md` (the builder report behind the #290 notes). The case is outside the #290 diff. No v1 feature or acceptance check is blocked.
