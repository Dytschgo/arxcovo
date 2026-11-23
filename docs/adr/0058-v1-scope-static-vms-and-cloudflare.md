# 0058: v1 scope amendment — static VMs and Cloudflare Tunnel publishing

Status: accepted. Owner instructions of 2026-11-23, relayed by the babysitter. This is a scope amendment to docs/KICKOFF.md v1. It runs in parallel with M4–M6 under [ADR 0056](0056-parallel-milestones-and-throughput.md).

## Decision

Two roadmap items move into v1. They keep the settled design and are built on the M4 VM runtime.

### Static VMs (from roadmap item 3)

- Long-lived VMs with stable, owner-meaningful names. They are distinct from expiring TEST clones and are not reaped by lifetime expiry.
- **Agents cannot delete static VMs or backups.** That is a technical fence, not an approval flow. Agents may start, stop and run scripts in a static VM within its policy. Deletion and backup removal stay outside agent authority.
- **The first static VM is a Linux web server.** It is an Ubuntu guest built through the M4 cloud-init Linux template path. Windows guests on this Windows host remain TEST-only.
- Disk accounting, the cap and reserve, custody and the native admission gates are the same as M4 (ADR 0054). Exception (owner, 2026-11-23): on this PC the execution identity is the owner's current login (ADR 0029 `current_user`, elevated tokens refused) rather than a dedicated account. ADR 0054's dedicated-account and distinct owner/supervisor identity gates, including the WTS owner-activity signal that assumes distinct SIDs, must be amended in the native activation change before native admission opens. Native paths stay closed until those gates pass and are **unverified on real hardware** until exercised.
- Backups and maintenance updates stay on the roadmap, except for the protections above.

### Cloudflare Tunnel publishing (from roadmap item 8)

- Exposure uses `cloudflared` and Cloudflare Tunnel on the owner's own Cloudflare account.
- The setting "Agents may publish public sites" **defaults to NO**. Publishing is an explicit, reversible action, and unpublishing removes the public route.
- **Owner inputs.** The owner supplies the Cloudflare account and the tunnel token or credential.
  - Agents never create accounts or handle payment.
  - Agents never put credentials in source, logs, MCP responses or commits.
  - When the build reaches the step that needs the credential, the coordinator records exactly what is needed in docs/PROGRESS.md and asks the owner once through a private secret request, while continuing other work.
- **Tests.** CI tests use fakes and mocks of `cloudflared` and the Cloudflare API. The real tunnel path is **unverified on real hardware** until it is exercised with the owner's credential.
- **Target.** The first publication target is the static Ubuntu web-server VM.
- **Foreground controls.** An agent may publish an enrolled running static VM, unpublish it, or read its publication status. The daemon uses a dedicated locally managed tunnel with one approved exact hostname, a generation-specific TLS origin on IPv4 loopback, and a zone-scoped DNS credential. The [v10 store contract](0065-vm-store-v10-publish.md) records ownership and reconciles incomplete effects without automatically reopening ingress.

## Acceptance

Each item gets its own acceptance check, recorded in PROGRESS when its design is adopted:

- **Static VMs:** an agent creates and starts the named Ubuntu web-server VM; it survives a lifetime sweep; an agent delete request is refused.
- **Cloudflare publishing:** with the setting off, publish is refused; with it on and the owner credential present, publish exposes the web server and unpublish removes it.

Neither item's acceptance is inferred from M4–M6 or from fakes.

## Unchanged

All hard rules in AGENTS.md remain: CI under 15 minutes, no bypass of `ci-time-budget` or `agent-review`, D: scratch, no Lab Manager or `agent-windows-test`, no secrets, and honest hardware labels.
