# 0065: VM store v10 records static site publication intent

Status: implemented for the foreground VM profile under [ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md).

## Decision

Migrate the VM store forward from v9 to v10. The migration retains activity IDs and sequence, operation receipts and replay bindings, static identities and generation evidence. Existing VMs start with unpublished sites. A new singleton intent binds one exact Cloudflare name to the owned static VM, its active generation, its live boot and the effective owner profile. The owner supply is one exact hostname in a bounded list. Additional names can extend the ingress and DNS reconciliation in ADR 0064.

The intent persists the original zone, tunnel, DNS ciphertext reference, ownership nonce, possible-POST flag and, once verified, the DNS record ID. These fields stay in the private VM store, outside MCP and activity projections. The possible-POST flag commits before creating a record. An observed absence alone cannot discharge a possible POST without a recorded ID. Cleanup stops the local connector first, verifies the original owned tuple before removal, and keeps ambiguous effects in the slot. The daemon reconciles remaining intent on restart without restarting the connector. Request-ID replay uses the existing per-principal operation ledger and has no effect authority.

The foreground VM configuration defaults `allow_agent_public_sites` to false. The optional `publication` binding contains `hostnames` (exactly one lower-case DNS name), `static_name`, `host_id`, `tunnel_id`, `account_id`, `zone_id`, `private_root`, `dns_ciphertext`, `tunnel_ciphertext`, `connector`, `connector_length` and `connector_sha256`. The two ciphertext references point to distinct files directly inside the private root. The owner encrypts the locally managed tunnel credential JSON and the zone-limited DNS bearer with DPAPI under the non-elevated foreground runtime login, using user scope and UI disabled. The connector receives plaintext JSON in its isolated environment at launch. The bearer is used only by the DNS HTTPS transport. Generated ingress files contain only the fixed routing and verified origin TLS configuration.

The Windows connector verifies the configured executable receipt, holds the process in a separate kill-on-close Job and requires both an attributed readiness listener and the generation CA and fixed TLS name on loopback. It retries a pre-readiness process exit at most once. Loss of a published connector or its current origin triggers revoke. A qualified native static launch must provide the active generation, a live boot witness, and the custodied origin CA and port. The existing native admission gate stays closed until those proofs are available.

## Verification

Local foreground and DNS/connector fixtures use synthetic credentials, a loopback origin and exact owned-record evidence. The live Cloudflare tunnel, DNS and native static VM origin remain **unverified on real hardware**.
