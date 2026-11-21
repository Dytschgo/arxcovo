# 0052. Three-container acceptance with native Linux isolation

## Status

Proposal A adopted for the R2 dry-run implementation, 2026-11-21. Hosted R2 qualification and live C16 enablement are pending. The adoption record is the M3 step 16a design, SHA-256 7d66e7e1de9bfd4d2eab8fe5bc3550cff45e337d00883aaf27db295f75c4de98, sections 1, 3 and 4.

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

An unnumbered bridge carries the three fixed veth endpoints. The bridge table pins forwarding and local input per interface, MAC, IP and port. Scoped host inet input and forward chains supply a second boundary. R2 verifies nonce traffic on allowed paths and traces controlled denials to rule handles. Synthetic receivers give the denied tests a known destination without contacting external or infrastructure services.

Persisted installation, configuration, history and recovery artifacts belong on the original attributed data image. Exclusive-bind socket parents belong on volatile /run. After an attributed process loss, a new /run and re-created process namespaces permit exclusive rebinding while the original data image remains attached. R2 uses an inert socket and a persistent marker to exercise that lifecycle.

R7 separately owns the authenticated local Mint receipt and applied-state witness used by live C16. Its framed owner-local operation proves the named retained log entries and their matching applied pending records under checked local state. It also binds the local membership and installation observations required by the campaign. R2 exercises Linux peer and containment predicates with synthetic data. Live C16 requires reviewed exact-head R2 evidence and its own separate change.

## Qualification

Keep the protected matrix and the required gates. Reuse the locked, loader-checked artifact for no-follow ELF, digest and sealed staging inspection. The R2 dispatcher has a single dry entry. The probe budget totals 85 seconds including cleanup, within the two-minute workflow step. The eventual 240-second C16 budget and 720-second worker and 900-second run gates require separate measured acceptance.

The [hosted pin brief](../briefs/m3-container-hosted-pins.md) records the host and userland identities and evidence limits. The containment and timings are **unverified on hosted runner**. Installed service, physical suspend and multi-host paths are **unverified on real hardware**.
