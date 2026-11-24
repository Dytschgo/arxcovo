# 0069: VM store v13 — multi-hostname publication child rows

Status: implemented for the foreground VM profile under [ADR 0064](0064-v1-scope-docker-static-vm.md) (Docker leg E3, extending [ADR 0065](0065-vm-store-v10-publish.md)).

## Decision

Migrate the VM store forward from v12 to v13. The migration retains all activity IDs and sequence numbers, operation receipts and replay bindings, static VM identities and generations, restore points, and Compose state.

Expand the B2 singleton publication tuple in `vm_publication` into a parent publication intent (`vm_publication`) plus per-hostname child rows (`vm_publication_hosts`):

- **Parent row (`vm_publication`):** holds the single host-wide publication lock (`singleton = 1`), static VM ID, active generation, boot ID, binding revision digest (covering the entire sorted hostname set, zone binding, route slots, tunnel, credentials, and connector pins), tunnel ID, zone ID, DNS ciphertext reference, parent intent nonce, publication phase (`publishing`, `published`, `revoking`, `cleanup_pending`), and aggregate `cleanup_reason`.
- **Child rows (`vm_publication_hosts`):** hold `1..=8` ordered entries (`position` in `0..=7`) referencing `vm_publication(singleton) ON DELETE CASCADE`. Each row records:
  - exact lowercase `hostname` (unique within the intent),
  - optional fixed Compose route binding (`project_id`, `service`, `project_slot` in `0..=7`, `service_index` in `0..=3`, unique `route_slot` in `0..=7`),
  - distinct 32-hex ownership `marker` (used as Cloudflare DNS comment `arxcovo:<marker>`),
  - `post_possible` (`0 | 1`),
  - `record` (32-hex Cloudflare DNS record ID, nullable; requires `post_possible = 1`),
  - `ambiguous` (`0 | 1`; requires `post_possible = 1`),
  - per-row `state` (`pending`, `verified`, `cleaned`, `cleanup_pending`), and
  - per-row `failure_reason`.

### Singleton migration (v12 → v13)

If a v10–v12 database contains an existing `vm_publication` singleton row, `MIGRATE_V13` copies the parent metadata into the rebuilt `vm_publication` table and migrates the singleton hostname and ownership proof into child row `position = 0` of `vm_publication_hosts`:

- `marker` is copied verbatim from the v12 singleton `nonce`, preserving the exact `arxcovo:<nonce>` ownership proof for already-created or uncertain DNS records.
- `post_possible` and `record` are copied verbatim.
- `ambiguous` is set to `1` when `post_possible = 1` and `record IS NULL`, and `0` otherwise.
- `state` is set to `verified` when the parent phase is `published`, `cleanup_pending` when `cleanup_pending`, and `pending` otherwise.

### Multi-hostname publish and cleanup invariants

- Owner policy `allow_agent_public_sites` remains `false` by default.
- The owner binding is a sorted, unique list of `1..=8` exact lowercase hostnames in the same enrolled zone, tied 1-to-1 for Docker publication (the legacy B2 singleton may omit routing) to fixed Compose `(project_id, service, project_slot, service_index, route_slot)` slots mapping to bounded guest loopback ports (`20000..=20031`).
- Admission journals the parent row and all child rows in one transaction before any external effect. Any existing publication row (active or `cleanup_pending`) blocks replacement with `publish_cleanup_pending`; changing the hostname set requires explicit unpublish followed by publish.
- Publish preflights every hostname before starting the single connector Job or creating any DNS record, probes every requested Host over verified TLS against the per-generation CA and `originServerName`, and creates DNS records in stable `position` order with per-row `post_possible`, `record`, and `ambiguous` persistence.
- Final commit transitions the parent to `published` only when at least one child row exists and every child row has `post_possible = 1`, `record IS NOT NULL`, `ambiguous = 0`, and `state = 'verified'`.
- On failure, unpublish, stop, or policy-off, the local connector is stopped first. Cleanup then processes every child row independently without short-circuiting: owned records are verified and deleted even when another row is blocked or ambiguous. Rows proven clean transition to `cleaned`; ambiguous or conflicted rows stay `cleanup_pending` with their original bindings and markers intact across drift and restart, blocking replacement until reconciled.
- `publish_status` projects only non-secret per-host states (`position`, `state`, `ambiguous`, optional `route_slot`/`project_id`/`service`, `reason`), keeping hostnames, zone IDs, tunnel IDs, markers, record IDs, and credential paths in the private VM store.

The foreground worker preflights all DNS names, then sends the exact owner bindings over the verified Compose session attributed to the same static generation and origin boot. The guest verifies the committed project, slot, service index and published HTTP port under its lifecycle lock before installing the nginx table. The host rejects deployments which would reassign an active route slot. The connector probes a per-binding nginx location over custodied TLS and probes the service before creating DNS. A legacy B2 binding keeps its TLS identity probe. The existing native static-boot qualification gate still controls activation.

## Verification

CI exercises v12 → v13 singleton migration, list/zone/slot bounds, single-connector multi-host ingress generation (`httpHostHeader`, `originServerName`, `caPool`, `noTLSVerify: false`, `warp-routing: enabled: false`, `http_status:404`), guest nginx rendering and atomic validate/reload rollback, header trust and spoofing rules, deterministic HTTP 206/416 `Range`/`If-Range` checks, and fake tunnel/DNS multi-host failure and cleanup paths.

Real Cloudflare API calls, `cloudflared`, Docker, guest nginx execution, and live VM networking remain **unverified on real hardware**.
