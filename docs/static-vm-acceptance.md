# Static VM and Compose owner-host acceptance

Run this journey on the enrolled Windows x64 host under the owner's non-elevated
console login, after the [host runbook](dev-host-setup.md), [M4 acceptance](m4-acceptance.md),
and independently reviewed native static-VM, Compose recipe, and patched
QEMU/libslirp public-egress qualification are complete. The Rust integration
test keeps the physical journey owner-run (`#[ignore]`). Its deterministic
checks run in normal CI with fakes.

All owner-host static-VM paths — guest Docker, patched QEMU/libslirp public
egress, Windows DPAPI/NTFS custody, live VM persistence and restore, and
Cloudflare Tunnel publishing — are **unverified on real hardware**.

## Scope and boundaries

This runbook covers the v1 static-VM and Docker/Compose acceptance criteria in
[ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md),
[ADR 0064](adr/0064-v1-scope-docker-static-vm.md),
[ADR 0067](adr/0067-vm-store-v12-compose-host.md),
[ADR 0069](adr/0069-vm-store-v13-multihost-publish.md),
[ADR 0070](adr/0070-vm-store-v14-compose-backup.md), and
[ADR 0071](adr/0071-vm-store-v15-compose-backup-retirement.md):

- **Compose static VM serving two configured names:** deploy the primary
  workload (Calm Life) and a synthetic second service on the static Ubuntu
  Compose VM; verify direct TLS over the localhost-only forward using the
  custodied per-generation origin CA and pinned `originServerName`
  (`arx-<generation>.invalid`) serves distinct HTTP 200 responses for each
  configured `Host` and rejects unknown `Host` headers.
- **Byte-range behaviour:** verify full GET returns `200` with
  `Accept-Ranges: bytes`, a single valid `Range` returns `206` with exact
  `Content-Range`, `Content-Length`, and byte slice, an unsatisfiable range
  returns `416` with `Content-Range: bytes */<length>`, and `If-Range` matches
  or falls back to `200` when ETag is present.
- **SQLite persistence across restart and offline backup restore:** verify
  unauthenticated admin access is refused, ingest a synthetic SQLite event,
  create a protected host SQLite backup via `compose_backup`, restart the
  static VM (`vm_stop` -> `vm_start`) and confirm the event survives on the
  persistent volume, then restore the settled host backup offline into a
  disjoint disposable target directory and confirm `PRAGMA quick_check`,
  exact SHA-256, portable DELETE journal mode, event presence, and unchanged
  settled backup source bytes.
- **Agent delete fence:** verify `list_tools` exposes none of the forbidden
  delete/restore/host-execution tools and direct calls to `static_vm_delete`,
  `vm_delete`, `backup_delete`, `vm_backup_delete`, `compose_backup_delete`,
  `compose_volume_delete`, `compose_project_delete`, `compose_backup_restore`,
  `vm_snapshot_delete` (against an owned Compose backup ID), `run_command`,
  and `discover_tools` return their exact typed refusal codes while leaving
  the static VM, Compose volumes, and settled backup intact.
- **Public-only egress:** verify the enrolled v1 public-egress policy file and
  patched QEMU/libslirp pin (refusing stock QEMU 11.1.3 bytes), require
  daemon attestation of Design §2 network qualification, and verify inside the
  guest that IPv6 is disabled, public IPv4 TCP/UDP work, and slirp aliases
  (`10.0.2.2`, `10.0.2.3`), link-local (`169.254/16`), CGNAT (`100.64/10`),
  private RFC1918, and enrolled host/LAN/hairpin targets are refused.
- **Public publishing default off:** keep `allow_agent_public_sites: false`
  with no `publication` block in the foreground profile; verify `publish_site`
  refuses with `public_sites_disabled`, `publish_status` reports `unpublished`
  with `host_count: 0`, and `vm_publication` / `vm_publication_hosts` remain
  empty.

## Cloudflare Tunnel publishing (ADR 0058)

Cloudflare Tunnel publishing stays **off** throughout this acceptance driver.
The driver refuses to launch if `allow_agent_public_sites` is `true` or a
`publication` block is present. It **never** contacts the real Cloudflare API,
never launches `cloudflared`, and requires no Cloudflare token or DNS
credential.

Live Cloudflare Tunnel publishing (`allow_agent_public_sites: true`, real
`cloudflared`, real DNS record creation, multi-hostname public HTTPS through
the tunnel, and unpublish cleanup) is a separate owner-authorized step after
M4 and static-VM acceptance qualify, and remains **unverified on real
hardware**.

The local HTTPS checks cover the origin only. ADR 0064 also requires an independent
public client, video seeking, cache and forwarded-client-IP checks, explicit republish
after restart, and removal of local ingress and owned DNS on unpublish. These remain
open owner checks. The guest probe exercises a subset of Design §2. The independent
network qualification must also cover container traffic, UDP destination enforcement,
TEST isolation, route drift under traffic and the other §2 witnesses.

## Owner-private plan

Prepare an owner-private JSON plan on `D:`. All paths (`daemon`,
`daemon_config`, `client_config`, `evidence_dir`, `backup_root`, `restore_dir`,
`public_egress.policy_path`, `origin_ca_path`, and `admin_password_file`) must be local
`D:` paths without reparse points. `state_dir`, `evidence_dir`, `backup_root`,
and `restore_dir` must be four mutually disjoint directories. Use a fresh
acceptance store with no existing static or TEST VMs. A repeat requires a new
isolated enrollment. Provision the origin CA as DER at the owner-specified path
and supply a private HTTP bearer credential for the qualified workload.
Ensure the guest has bash, timeout and dig. Supply a reachable TCP witness,
a public recursive IPv4 DNS resolver, and approved denial witnesses with
known-live services. Include the enrolled uplink source and gateway, an
RFC1918 destination, the public-numbered LAN and the WAN hairpin.

`qualification_sha256` is an expected pin only: the client records it only
after an authenticated, PID-verified daemon `vm_catalog` response presents a
matching M4 `selected_test_qualification` digest and a `selected_static_qualification` receipt covering both the Compose
recipe and the non-stock patched public-egress build and policy.

Paths and values below are placeholders:

```json
{
  "daemon": "D:/acceptance/bin/arxd.exe",
  "daemon_sha256": "<reviewed release SHA-256>",
  "origin_ca_path": "D:/acceptance/private/origin-ca.der",
  "daemon_config": "D:/acceptance/private/daemon.json",
  "client_config": "D:/acceptance/private/client.json",
  "store": {
    "volume_guid": "\\\\?\\Volume{<lowercase volume GUID>}\\",
    "volume_serial": "<16 lowercase hex digits from FILE_ID_INFO.VolumeSerialNumber>",
    "directory_file_id": "<32 lowercase hex digits from FILE_ID_INFO.FileId.Identifier>"
  },
  "qualification_sha256": "<reviewed static/public-egress qualification SHA-256>",
  "m4_qualification_sha256": "<reviewed M4 qualification SHA-256>",
  "evidence_dir": "D:/acceptance/evidence",
  "backup_root": "D:/acceptance/backups",
  "restore_dir": "D:/acceptance/restore",
  "idle_delay_s": 660,
  "static_vm": {
    "name": "calm-life-static",
    "recipe_id": "<Ubuntu Compose recipe ID (32 lowercase hex)>",
    "recipe_sha256": "<Ubuntu Compose recipe SHA-256>"
  },
  "public_egress": {
    "manifest": "qemu-11.1.3-arx-public-egress-windows-x64",
    "qemu_sha256": "<patched qemu-system-x86_64.exe SHA-256 from tools/qemu/public-egress-receipt.json>",
    "receipt_sha256": "<patched RECEIPT.txt SHA-256 from tools/qemu/public-egress-receipt.json>",
    "policy_path": "D:/acceptance/private/egress-policy.txt",
    "policy_sha256": "<enrolled policy v1 SHA-256>",
    "public_tcp_target": "<public IPv4 witness host:port>",
    "public_dns_target": "<public IPv4 DNS resolver:53>",
    "denied_targets": [
      "10.0.2.2:443",
      "10.0.2.3:53",
      "169.254.169.254:80",
      "100.64.0.1:443",
      "<enrolled Windows uplink IPv4:port>",
      "<enrolled gateway IPv4:port>",
      "<private LAN witness IPv4:port>",
      "<public-numbered LAN witness IPv4:port>",
      "<WAN hairpin witness IPv4:port>"
    ]
  },
  "compose": {
    "primary": {
      "project_id": "calm-life",
      "release_id": "<enrolled Calm Life release ID (32 lowercase hex)>",
      "hostname": "calm.example.invalid",
      "marker_path": "/health",
      "expected_marker": "<distinct primary marker>",
      "media_path": "/media/intro.mp4",
      "range_start": 4,
      "range_end": 15,
      "event_post_path": "/api/events",
      "event_query_path": "/api/admin/events",
      "admin_auth_path": "/api/admin/events",
      "admin_password_file": "D:/acceptance/private/admin-bearer.txt",
      "sqlite_table_query": "SELECT payload FROM events ORDER BY id DESC LIMIT 64"
    },
    "secondary": {
      "project_id": "synthetic-two",
      "release_id": "<enrolled synthetic second service release ID (32 lowercase hex)>",
      "hostname": "second.example.invalid",
      "marker_path": "/health",
      "expected_marker": "<distinct secondary marker>"
    }
  }
}
```

## Running the owner journey

Use this command only after the product gaps below close and the owner has
reviewed the daemon and qualification digests. The daemon starts to verify
its authenticated catalog. VM mutations begin only after the receipt matches.

Set [build scratch paths on D:](dev-scratch.md), verify at least 20 GB free on
`C:`, keep the console active and unlocked, and build ahead of the idle window:

```powershell
$env:ARX_STATIC_VM_ACCEPTANCE_PLAN = 'D:/acceptance/private/static-plan.json'
cargo test --locked -p arxd --test static_vm_acceptance --no-run -j 3
cargo test --locked -p arxd --test static_vm_acceptance -j 3 -- owner::owner_host_static_vm --exact --ignored --nocapture
```

The client reuses the M4 acceptance client's named-pipe transport,
authentication, kernel server PID check (`bind_foreground_bridge`), enrolled
volume/directory store pinning (`pin_store` / `EnrolledVmStore`), private
Ctrl+C `Console` drain, and preflight schema-version checks (`VM_SCHEMA_VERSION`
compiled from `crates/arxd/src/store/vm.rs`).

## Evidence and redaction

Each run writes `acceptance.json` and its SHA-256 and byte length in
`manifest.json` under `evidence_dir/<run_id>/`. Recorded evidence contains only
opaque IDs, SHA-256 digests, typed refusal reasons, HTTP status codes, byte
counts, and boolean predicates. Hostnames, admin passwords, DPAPI ciphertext or
plaintext, guest script bodies, private LAN/host IPs, raw paths, and raw MCP
transcripts are excluded and rejected by the client's sanitization check before
writing.

## Current refusal behavior and product gaps before live runs

Until M4 and static/Compose native activation land, running the owner journey
fails closed at the earliest unmet gate:

1. **Closed native admission (`admission_closed: qualification_missing`):**
   `Supervisor::capability` in `crates/arxd/src/mcp/vm.rs` keeps native
   admission closed behind an uninhabited `LaunchAuthority` until M4 and
   public-egress qualification receipts are issued.
2. **Daemon qualification receipt (`daemon_qualification_evidence_unavailable`):**
   `vm_catalog` does not yet emit `capability.selected_static_qualification`
   binding the reviewed qualification digest, the `ubuntu-noble-amd64-compose-v1`
   recipe pin, and the patched `qemu-11.1.3-arx-public-egress-windows-x64`
   binary/policy pins with `network_qualified: true`.
3. **Static origin witness (`static_origin_evidence_unavailable`):**
   `static_vm_get` does not yet emit `static_origin_witness` (bound to `vm_id`,
   `generation`, `operation_id`, `witness_id`, `origin_port` in
   `49152..=49215`, `origin_server_name`, `origin_ca_sha256`,
   `network_class: "public_egress"`, and `policy_sha256`), and `StaticRow.ready`
   is closed in non-test builds.
4. **Enrolled store pin vs. Compose secret root (`store_compose_conflict`):**
   `TestVms::validate` in `crates/arxd/src/config.rs` currently refuses
   combining `expected_store` (injected by `pinned_vm_config`) with
   `compose_secret_root` (`VmStoreComposeConflict`). Profiles requiring
   DPAPI-backed `ADMIN_PASSWORD_FILE` delivery need product support for
   co-pinning `expected_store` alongside a disjoint `compose_secret_root` (and
   `backup_root`) before live runs with Compose secrets can use store pinning.
5. **Closed production Compose & backup wiring:**
   `Supervisor::open` does not yet enroll the `ubuntu-noble-amd64-compose-v1`
   recipe slot, wire `BackupHost` custody/capacity or config fields for
   `compose_backup_root` / `compose_backup_targets`, or attach live
   `ComposeChannel` / `BackupSession` instances on static VM boot (see
   [ADR 0070](adr/0070-vm-store-v14-compose-backup.md)). In addition,
   `store::vm::compose::enroll` requires a committed `vm_static_instances` row
   at daemon open, so fresh static-VM creation and initial Compose release
   enrollment currently require either a pre-created static VM or a daemon
   restart between `static_vm_create` and release enrollment.

If the foreground fails to drain cleanly or custody is uncertain, preserve the
ledger, static VM media, and settled backups and follow the rollback and
[owner backup retirement](dev-host-setup.md#retire-a-compose-backup)
procedures. Never bypass agent delete refusals.
