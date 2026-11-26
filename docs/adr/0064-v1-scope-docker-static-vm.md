# 0064: v1 scope amendment — Docker multi-service static VM with outbound internet

Status: accepted (2026-11-23). Owner decisions of 2026-11-23, direct. This amends [ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md). The design is `D:/Temp/arx-static-docker/design.md`, SHA256 `87f580b5…d426`. Implementation waits until static A2 and Cloudflare B2 have merged. The work runs in parallel with M4–M6 under [ADR 0056](0056-parallel-milestones-and-throughput.md).

## Decision

Docker Compose hosting on the static Ubuntu VM moves into v1. Calm Life, a Node 24 browser game service with byte-range media and SQLite data, is the first deployment. The VM may host more than one Compose project. M4–M6 keep their priority.

### Network

The owner grants the static VM full outbound internet. The coordinator interprets that grant as the public internet only, and this record is written to that interpretation. The guest and its containers are denied the Windows host, including slirp's 10.0.2.2 and 10.0.2.3 aliases and the host arxd, dashboard, MCP and guest-agent endpoints, and private, link-local, CGNAT and Windows LAN destinations. Inbound stays closed except for Arxcovo's localhost-only forward. TEST VMs and offline installer networking stay restricted.

The owner chose a patched QEMU/libslirp pair to enforce that egress outside the guest. The alternatives set aside were Windows Filtering Platform and allowing LAN access. The patched build must be separately pinned, reviewed and qualified, with its own source, build and native receipt. Stock QEMU does not express this policy. Admission refuses a boot when the policy, route or binary proof is missing. The patched build adds no elevation beyond the current M4 login. The reviewed stock QEMU 11.1.3 pin in #284 is the starting baseline, or a newer qualified baseline. The patched bytes need their own receipt.

The accepted network default is public IPv4 TCP/UDP only. Native IPv6 and Internet ICMP are outside this decision.

### Accepted defaults

- Registry images are digest-pinned. The broker renders Compose from a typed project. Agents may deploy, start, stop and bring a project down within policy.
- The first VM envelope is 2 vCPU, 4 GB RAM and a 32 GB disk, inside the existing host reserve.
- Secrets use current-user DPAPI ciphertext on the host. An enrolled Windows x64 foreground store with a Compose secret root requires an independent volume GUID, serial and directory ID for each root. The daemon holds both disjoint directories and checks their bindings before opening the store and at each secret read boundary. Ciphertext opens relative to the held owner-private secret directory. Runtime bytes go to a per-service guest tmpfs over the verified serial channel. Plaintext stays out of source, the host store, logs, audit, MCP and backups.
- Diagnostics are typed and bounded. Raw container logs are not an agent tool.
- Agents cannot delete volumes or backups, and they still cannot delete the static VM. Deletion stays outside agent authority.
- nginx remains the single guest reverse proxy. Publishing stays off by default, on one tunnel, for a bounded list of exact names. Unpublish removes the route.
- CI uses fakes. It does not run Docker or a VM. At most three additional seconds per CI leg.

## Build order

This record lands in the docs batch. The four implementation PRs start after static A2 and Cloudflare B2:

1. Public-only static boot fence, including the patched QEMU/libslirp pin.
2. Compose recipe, lifecycle and secrets.
3. Multi-hostname routing on the B2 tunnel.
4. Protected host backup of the project SQLite data.

Calm Life's workload checkout is qualified after those four PRs. It is not one of the four.

## Acceptance

On the qualified Windows host, deploy Calm Life through the foreground VM profile. With publishing off, publish is refused. With it on, two approved names serve distinct HTTPS responses through one tunnel. An mp4 byte-range GET returns the correct 206 body and headers. Event ingestion and admin authentication work, and a SQLite event survives a VM restart. A protected host backup restores that event into a disposable target. Agent deletion of the static VM, a volume and a backup is refused. Public TCP/UDP and DNS work, and a root-capable guest is denied the host and the LAN, including the slirp aliases and private, link-local, CGNAT and enrolled LAN destinations, enrolled public-numbered LAN addresses, and hairpin to the host's public WAN address. Design section 2 is the complete network qualification, including route drift under traffic. Design section 8 is the complete workload qualification, including a synthetic second service, HTTP 416 for an unsatisfiable range, and an explicit republish after restart. Unpublish removes local ingress and owned DNS. TEST guests keep their restricted network.

Guest Docker, the patched egress fence, Windows custody, persistence and the live Cloudflare path are **unverified on real hardware** until they are exercised and recorded in PROGRESS. Fakes do not accept this item. Prior milestones keep their own checks.

## Unchanged

The hard rules in AGENTS.md and ADR 0058 still apply, including D: scratch, no changes to Lab Manager or existing Hyper-V VMs, coordinator-only merge, independent review and the required CI gates. The execution identity remains the owner's current login ([ADR 0054](0054-m4-dedicated-windows-runtime.md) as amended for this host). Backups beyond the minimal same-host SQLite copy, a general vault, and scheduled or off-host restore stay on the roadmap.
