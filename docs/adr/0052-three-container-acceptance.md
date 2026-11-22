# 0052. Three-container acceptance with native Linux isolation

## Status

Proposal A was adopted for R2 on 2026-11-21. M3 step 16a Part 3 applies the same containment to live C16. The adoption record is the M3 step 16a design, SHA-256 7d66e7e1de9bfd4d2eab8fe5bc3550cff45e337d00883aaf27db295f75c4de98, sections 1–4. Live C16 remains **unverified on hosted runner**.

## Context

The proposed literal Docker topology failed four frozen runtime predicates.

| Docker layout | Frozen predicate |
| --- | --- |
| Overlay root | The native account resolver starts its protected passwd walk at the root filesystem and refuses ordinary overlay2. |
| Host broker exposed by a mounted Unix socket | The broker peer must have a positive PID visible inside B. A host or sibling PID namespace peer renders as zero. |
| Broker at the root of a private cgroup namespace | The hierarchy parser requires a service path ending in /arxd.service/supervisor. |
| Default cgroup mounts and capabilities | The executor requires a read-only cgroup root, one writable service mount, the domain and controller layout, an empty unit parent, a sole supervisor PID, the byte-exact delegation marker, and bounding mask 0xc1 with no-new-privileges. |

This evidence covers the original topology and its defaults. A custom OCI staging layer would need independent qualification for the same predicates.

## Decision

Use three native Linux containers under attributable transient systemd units. Enter each pinned network namespace before creating private mount, UTS, IPC and PID namespaces. Stage the same sealed ext4 root image for all nodes. Allocate a separate 256 MiB ext4 loop image per node. The fixture mounts a bounded private proc and devpts view and bounds /run, /tmp and /dev independently of persistent data.

Privileged staging creates the mounts and cgroup hierarchy. Container PID 1 drops every capability and handles only process supervision after staging. B's in-namespace broker stand-in enters the sole supervisor leaf through a parent barrier. It retains only CAP_CHOWN, CAP_SETGID and CAP_SETUID and verifies the profile after exec. Only the nested B service mount remains writable, and the BUILD identity must be unable to write its controls. Each node has fixed local role accounts. The ordinary B workspace slot stays on its data filesystem, with root-owned ancestors and a BUILD-owned output directory.

An unnumbered bridge carries three fixed veth endpoints in `192.168.72.0/24`. The coordinator repinned R2 and C16 after the Rust startup gate rejected the design's original benchmarking addresses. The new addresses satisfy the existing RFC 1918 policy, including the private peer-control decoder. The ignored Rust test helper uses the same route and original issuer. Preflight refuses overlapping explicit IPv4 host routes across routing tables. The bridge table pins forwarding and local input per interface, MAC, IP and port. Scoped host inet input and forward chains supply a second boundary. R2 verifies nonce traffic on allowed paths and traces controlled denials to rule handles. Synthetic receivers give the denied tests a known destination without contacting external or infrastructure services.

Persisted installation, configuration, history and recovery artifacts belong on the original attributed data image. Exclusive-bind socket parents belong on volatile /run. After an attributed process loss, a new /run and re-created process namespaces permit exclusive rebinding while the original data image remains attached. R2 uses an inert socket and a persistent marker to exercise that lifecycle.

R7 separately owns the authenticated local Mint receipt and applied-state witness used by live C16. Its framed owner-local operation proves the named retained log entries and their matching applied pending records under checked local state. It also binds the local membership and installation observations required by the campaign. R2 exercises Linux peer and containment predicates with synthetic data. The live dispatcher runs those startup gates once before launching the real daemons. B runs the actual broker as the sole supervisor PID with a sealed policy, a volatile control socket, and the inner BUILD limits. The foreground daemon and owner and MCP helpers run under their own identities inside their node's existing unit and namespaces.

C16 uses the protected owner PTY to join B, then sends B's consumed paste through an anonymous pipe to the exact ignored AnchorQuery test. A's owner evidence must correlate the same query handle, checked clock sample and leadership epoch with one more Query and no more Redeem. The C join then allows uniform membership and the two CA handoffs. C16 checks one delegated build on B and two private Mint receipts on every voter before fault injection.

The checked A-leader precondition bounds the intentional loss to the founder. A's daemon PID is pinned before SIGKILL. Its unit, mount namespace, volatile sockets and network endpoint are retired before B and C write and independently observe all four receipts. Recovery recreates A's endpoint and namespaces once while retaining its original ext4 data image. The same four receipts must then be present on all three original installations.

## Qualification

Keep the protected matrix and required gates. Reuse the locked, loader-checked daemon and bind the exact ignored library test executable to its own Cargo message and artifact receipt. Both artifacts undergo no-follow ELF and digest checks during sealed staging. The dry entry retains its 85-second budget and inert dispatcher. The Linux workflow invokes live once with a five-minute step timeout. Its six phases total 240 seconds including a 25-second cleanup reserve. A receipt only passes after the exact operation counts and attributed teardown are checked. The 720-second worker and 900-second run gates still require hosted measurement.

The [hosted pin brief](../briefs/m3-container-hosted-pins.md) records the host and userland identities and evidence limits. The repinned live containment and timings are **unverified on hosted runner**. Installed service, physical suspend and multi-host paths are **unverified on real hardware**.
