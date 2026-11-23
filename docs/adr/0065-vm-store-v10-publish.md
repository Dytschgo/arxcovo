# 0065: VM store v10 records static site publication intent

Status: implemented for the foreground VM profile under [ADR 0058](0058-v1-scope-static-vms-and-cloudflare.md).

## Decision

Migrate the VM store forward from v9 to v10. The migration retains activity IDs and sequence, operation receipts and replay bindings, static identities and generation evidence. Existing VMs start with unpublished sites. A new singleton intent binds one exact Cloudflare name to the owned static VM, its active generation, its live boot and the effective owner profile. The owner supply is one exact hostname in a bounded list. Additional names can extend the ingress and DNS reconciliation in ADR 0064.

The intent persists the original zone, tunnel, DNS ciphertext reference, ownership nonce, possible-POST flag and, once verified, the DNS record ID. These fields stay in the private VM store, outside MCP and activity projections. The possible-POST flag commits before creating a record. Only a worker that exits before submitting its POST may withdraw the flag. An observed absence alone cannot discharge a possible POST without a recorded ID. The record ID from a validated, owned create response is stored before the separate exact-name verification, so a failed verification lookup never discards it; success still requires that verification. Cleanup stops the local connector first, verifies the original owned tuple before removal, and keeps ambiguous effects in the slot. The daemon reconciles remaining intent on restart without restarting the connector. Restart recovery interrupts every pending publish and unpublish receipt. Request-ID replay uses the existing per-principal operation ledger and has no effect authority.

### Owner revocation always wins

The daemon keeps the current activation in memory: its owner, its admitting request key, a monotonic generation and a revocation token. An authenticated `unpublish_site` from that owner for that VM cancels the token and queues the stop of that generation's connector before it takes the foreground dispatch gate. Revocation therefore runs before the replay ledger, the SQLite write transaction, store headroom, the pending limit, the audit write and the DNS effects slot, and another writer holding any of them cannot delay it. The local connector slot is never held across a SQLite call. After revocation the activation cannot commit `published`, every readiness check fails and a connector start in progress is aborted.

The in-memory fence excludes request faults without consulting the store. The activation's own publish key is a collision. A key this process already admitted for publication is a replay. A key minted before the activation's publish predates it. None of them revokes. Any other well-formed key from the owner revokes, even when the ledger later refuses it as a collision with an unrelated action or as stale, because revocation only closes ingress. An admitted unpublish also revokes after committing `revoking`. A foreign caller cannot revoke.

The publish worker's final commit runs in one transaction that re-checks the revocation token, the origin and shutdown, and changes the intent only from `publishing` with durable record evidence. A `revoking` or `cleanup_pending` intent committed by an owner unpublish, the reaper or recovery is never overwritten, and the worker reports `cancelled` and cleans up instead.

A deferred revocation stop is bound to its activation generation, so a delayed stop cannot end a later activation of the same VM. An unpublish worker's pending receipt keeps publish admission for that VM closed until it settles.

The admitted unpublish never returns `busy`. Its worker waits for an in-flight publish, including a held DNS POST, which completes or times out and whose acknowledged record is then removed. When admission is refused for headroom, the pending limit or a failed audit write, the refusal stays typed and the reaper completes DNS cleanup from the durable intent. A failed reaper turn, including a failed reconciliation audit write, keeps the durable intent and is retried on the next tick instead of ending the reaper. A failed local stop stays retryable by later unpublish calls and reaper turns.

The foreground VM configuration defaults `allow_agent_public_sites` to false. The optional `publication` binding contains `hostnames` (exactly one lower-case DNS name, validated once and carried as a typed hostname into the intent and connector), `static_name`, `host_id`, `tunnel_id`, `account_id`, `zone_id`, `private_root`, `dns_ciphertext`, `tunnel_ciphertext`, `connector`, `connector_length` and `connector_sha256`. The two ciphertext references point to distinct files directly inside the private root. The owner encrypts the locally managed tunnel credential JSON and the zone-limited DNS bearer with DPAPI under the non-elevated foreground runtime login, using user scope and UI disabled. The daemon accepts only a version 1 blob from the default DPAPI provider without `CRYPTPROTECT_LOCAL_MACHINE`, checked before decryption. A machine-scope blob decrypts for every local account and is refused. The connector receives plaintext JSON in its isolated environment at launch. The bearer is used only by the DNS HTTPS transport. Generated ingress files contain only the fixed routing and verified origin TLS configuration.

The Windows connector verifies the configured executable receipt, holds the process in a separate kill-on-close Job and requires both an attributed readiness listener and the generation CA and fixed TLS name on loopback. It retries a pre-readiness process exit at most once. Loss of a published connector or its current origin triggers revoke. A qualified native static launch must provide the active generation, a live boot witness, and the custodied origin CA and port. The existing native admission gate stays closed until those proofs are available.

## Open before native admission

Native publication stays closed with the A2 origin seam. Before it opens, the native activation change pins one reviewed Windows x64 `cloudflared` release with provenance, exact size and digest, and its runtime dependency receipt, and supplies a reviewed owner provisioning step that emits the accepted user-scope DPAPI form.

## Verification

Local foreground and DNS/connector fixtures use synthetic credentials, a loopback origin and exact owned-record evidence. The live Cloudflare tunnel, DNS and native static VM origin remain **unverified on real hardware**.
