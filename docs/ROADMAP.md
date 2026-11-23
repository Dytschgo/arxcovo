# Post-v1 roadmap

Milestones 1–6 remain the v1 scope in KICKOFF.md. Owner amendment 2026-11-23 ([ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md)): static VMs (first target an Ubuntu web server) and Cloudflare Tunnel publishing (off by default, owner credential) moved into v1. Further owner amendment the same day ([ADR 0064](adr/0064-v1-scope-docker-static-vm.md)): Docker Compose multi-service hosting on that static VM is in v1, with Calm Life as the first deployment and public-only outbound internet. Later work, in order:

1. Linux hosts.
2. WireGuard overlay.
3. Backups and maintenance updates. Static VMs moved into v1 (ADR 0058). Docker Compose hosting on the static VM is also in v1 (ADR 0064). The agent no-delete fence for static VMs, their volumes and backups applies now.
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
- **Intermittent `authenticated_sender_mismatch_and_assumed_founder_control` NotLeader.** `cluster::consensus::issue::tls_engine_tests::authenticated_sender_mismatch_and_assumed_founder_control` at `three_node.rs:222`. Red main `2bdcb657` (Ubuntu) and `4a8e425f` (macOS, attempt 1 of `45148496421`, green on attempt 2). It stays under the existing Engine fix hold. No v1 feature or acceptance check is newly blocked.
- **Dashboard session follow-ups from PR #283 (P3).** Bind the logout 204 to the session generation. Add a test for an earlier-session 401 with no session held.
- **Calm Life on the static VM.** [ADR 0064](adr/0064-v1-scope-docker-static-vm.md) adds Docker Compose hosting to v1. Calm Life (Node 24, byte-range media and SQLite) is the first deployment. Guest Docker, the public-only egress fence, Windows custody and the live tunnel are **unverified on real hardware**.
