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

- **Intermittent hosted C13 owner mint refusal.** PR #259 CI `45062459949` failed once in the hosted C13 activation campaign (`phase=join category=assertion message=mint-refused`, owner mint exit 8, clean teardown). It has passed in every later run (19 through `830b9de7`). Cause undetermined; no v1 feature or acceptance check is blocked. If it reddens `main` again, quarantine that single case.
