# 0075 Authenticate Docker receiver capture

Status: proposed (2026-11-27). This extends [ADR 0074](0074-bind-docker-trial-https-to-serial.md).

## Decision

Pin the independent receiver's Ed25519 public key with its exact receiver
inventory. The fixture dependency digest binds the new enrollment format.
Targets-only enrollments require a new pin.

The collector issues a private challenge while the guest is paused. The
receiver signs the challenge once its capture is armed. The collector
rechecks the live route before continuing. The receiver signs the same
challenge and the exact report bytes after observing the run. The signatures
use independent domain labels for the two phases. The collector checks the
scope, target inventory, time window, controls and drift events before a
report can contribute its digest to a Docker dossier.

The Windows origin reservation sets `SO_EXCLUSIVEADDRUSE` before binding
IPv4 loopback. The collector offers the socket once after checked suspended
creation. The launcher releases the reservation at the process resume edge.
Before guest execution, the collector checks the replacement listener
against the held child. Port competition or ambiguous listener ownership
refuses the trial. The installed endpoint still requires the two listener
samples around its boot-bound HTTPS check.

## Verification

Deterministic fixtures exercise the signature and port boundaries and the
held dossier reader. The checked native QEMU phase host and static runtime
remain gated. Native Docker, public-only networking and Windows custody are
**unverified on real hardware**.
