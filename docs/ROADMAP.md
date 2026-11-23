# Post-v1 roadmap

Milestones 1–6 remain the v1 scope in KICKOFF.md. Owner amendment 2026-11-23 ([ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md)): static VMs (first target an Ubuntu web server) and Cloudflare Tunnel publishing (off by default, owner credential) moved into v1. Later work, in order:

1. Linux hosts.
2. WireGuard overlay.
3. Backups and maintenance updates. Static VMs moved into v1 (ADR 0058); the agent no-delete fence for static VMs and backups applies now.
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
