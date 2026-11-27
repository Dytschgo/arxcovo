# 0074: Bind Docker trial HTTPS to the attributed serial peer

Status: proposed (2026-11-27). Applies to the disposable endpoint copy in
[ADR 0064](0064-v1-scope-docker-static-vm.md).

## Decision

The Docker qualification guest generates a fresh P-256 self-signed TLS
certificate for the boot's challenge-derived DNS name. It keeps the key in a
root-only directory on guest tmpfs and sends the public certificate in the
fixed trial provisioning reply over the attributed serial pipe. The guest
runs a dedicated nginx instance with one health response containing the
challenge generation and nonce. The host pins the endpoint protocol through
the exact enrolled origin profile bytes.

The collector requires a successful HTTPS exchange on the leased IPv4 loopback
port. It checks the held child owns the listener on both sides of that exchange.
The TLS client trusts only the certificate returned on serial and checks the
exact peer leaf, SNI and challenge response. The probe is bounded and observes
cancellation. Before the endpoint stage passes, the collector rechecks the
enrolled route. The trial retains its clean-stop and disposable-custody checks.

This replaces the earlier trial runbook's private CA/SNI input. The certificate
and its key are scoped to the disposable boot. Production static origins use
the separate generation-bound provisioning and settlement boundary.

## Verification

A Windows fixture uses a real TLS server in the collector's full dossier
emission and held-reader round-trip. Wrong certificate, wrong SNI and wrong
boot-response cases withhold the completion marker. The Linux guest's
fixed nginx configuration has a deterministic test. The native QEMU trial
host and independent receiver remain required to run the owner CLI.
Native VM boot and guest nginx execution are **unverified on real hardware**.
