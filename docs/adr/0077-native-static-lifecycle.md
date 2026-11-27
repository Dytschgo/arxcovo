# 0077 Native static lifecycle and restart custody

Status: proposed (2026-11-27). Extends [ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md) and [ADR 0076](0076-native-docker-qualification-phases.md).

## Decision

The selected Docker dossier binds the web and Compose recipe inputs to the selected M4 proof. The static builder uses the qualified offline installer, a blank 32 GiB QCOW2 disk and paired firmware. A full disposable copy supplies detached health evidence before the original pair is sealed. Static creation copies the immutable sealed bytes to two fresh objects under a generation lease. A failed or interrupted copy that might have created files stays charged in owner custody.

VM store v18 records the production directory, generation directory and lease identities when the independent static pair commits. It derives no native custody for preexisting rows. After restart, a recovery claim can settle only with the exact committed workspace lease and both writer exclusions held through the transaction. The static recovery path has no disposal authority. A missing or disputed identity leaves the row recovering. Compose volumes and settled backups retain their existing protections.

Production boot selects stock QEMU for restricted web origin or the enrolled patched QEMU and policy for Compose. An exclusive IPv4 loopback reservation stays held until the checked suspended child resumes. The QMP release checks the replacement listener against that child before guest execution. The independent serial handshake supplies a fresh boot challenge and obtains the generation CA and leaf. The guest retains the generation CA key and issues a fresh origin leaf on each boot. The daemon keeps only a public copy of the CA and checks the exact HTTPS leaf, hostname and challenge response between listener ownership checks. On first boot the daemon publishes that public copy atomically, by a hidden temporary file and a rename, in the generation workspace's dedicated `origin-ca/` subdirectory, so the workspace root keeps its read-only sharing; a later boot never overwrites the pinned copy. Before removing Docker's install-time masks, the guest installs persistent unit conditions that require a boot-local marker on tmpfs. The checked serial handshake creates the marker.

Boot settlement holds process and media custody through the transaction. The route watch retires the associated origin and serial sessions on notification or a failed continuity check. Checked stop retires the origin first, then requests QMP powerdown and confirms process and media release. Inconclusive release enters recovery without deleting or rewriting the committed pair.

Part B2i wires private routes with publishing off and opt-in Cloudflare activation and cleanup. It controls lifting the foreground static admission fence.

## Verification

Native VM boot, Docker, public-only networking, Windows custody, persistence and live Cloudflare publishing are **unverified on real hardware**.
