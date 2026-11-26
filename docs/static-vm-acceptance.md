# Static VM and Compose owner acceptance

The local journey checks Calm Life and a synthetic second project through a PID-bound, authenticated foreground MCP session. A separate ignored test drives the public tunnel. Both require a qualified Windows x64 host and the owner's non-elevated login. Native Docker, public-only networking, Windows custody, persistence and Cloudflare publishing remain **unverified on real hardware**. The product gaps below currently prevent a live acceptance claim. See [ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md), [ADR 0064](adr/0064-v1-scope-docker-static-vm.md) and the [M4 host procedure](m4-acceptance.md).

## Enroll a dedicated acceptance VM

1. Complete M4 host setup, independently review the M4 and patched public-egress qualification receipts, and qualify the digest-pinned Calm Life release. Prepare a new isolated state directory on D: and enroll its volume and directory identities using the M4 procedure. Never select an existing owner workload for this journey.
2. Start the qualified foreground daemon against that store with `test_vms.expected_store` and one acceptance principal. Omit the Compose secret root, its pin, the release list and publication configuration during bootstrap. Use the authenticated local MCP client to call `vm_catalog`. Select the qualified Ubuntu Compose recipe and its sealed template. If needed call `vm_template_build` with the catalog recipe ID and a fresh request key, then poll `vm_operation` until it succeeds. Mutation keys consist of 16 lowercase hexadecimal digits for current Unix milliseconds followed by 16 random hexadecimal digits. Reuse the same key only for a retry of the same request.
3. Call `static_vm_create` with that template ID, a unique acceptance name and a new request key. Poll the returned operation to success. Verify `static_vm_get` reports the expected VM ID, name, stopped state and recipe digest. Drain the bootstrap daemon. In the stopped owner-private ledger, use a read-only SQLite connection to run `SELECT lower(hex(id)) FROM vm_generations WHERE lower(hex(static_id)) = ? AND active = 1` with the acceptance VM ID as parameter. Put that generation and VM ID into the private plan.
4. Prepare the workload foreground config for the same store, endpoint and principal. Set `test_vms.compose_secret_root` to a disjoint owner-private directory. Supply the current-user DPAPI ciphertext named `<reference>.dpapi` for the workload's admin secret. In `test_vms.compose_releases`, supply exactly the two qualified entries. Each has this VM's `vm_id`, its own `release_id`, the typed `manifest` (including `project_id`), and the enrolled `secrets` slot/epoch/reference list when needed. Use the reviewed image digests and actual workload configuration. On the driver's next daemon launch, arxd enrolls these releases against the already committed static VM. Before boot the client verifies both release rows, the caller, name, recipe, template and generation.
5. Capture the secret directory's identity independently using the same procedure as for the store. Put those identities in the private plan as `store` and `secret_root`. Omit `expected_store` and `expected_compose_secret_root` from the source profiles for both the local and public stages. The client inserts both pins into a private launch copy and verifies the held roots before launch. For the default-off run also omit both `allow_agent_public_sites` and `publication` from `test_vms`. Supply existing disjoint owner-private evidence, backup and restore directories. Keep credential files private, local and outside those roots. Plan and config paths must be on D: without reparse points.

## Qualify static egress and TEST isolation

Use the independently supervised, disposable network qualification fixture described in Design §2 of ADR 0064. The Compose guest disables agent scripts. Perform these observations using the owner's fixture console or offline-provisioned qualification payload. Keep the deployed Compose image and the agent interface intact. If a qualified fixture entry point or capture is unavailable, stop here.

1. On the qualified Windows route and patched binary, operate benign witness listeners with receiver-side captures. Establish the positive controls from a permitted source. Use an owned public TCP service and public recursive UDP DNS resolver. Include the synthetic gateway and DNS aliases, host interfaces and endpoints, gateway, private, link-local and CGNAT destinations, enrolled public-numbered LAN and WAN hairpin. Bind the listed ports to the agreed controls. Capture the corresponding path for alias translations. For the TEST comparison, use the same working public endpoints in the restricted TEST recipe. Use the actual approved targets only.
2. Copy **only** the `public_egress` target inventory (the three keys `public_tcp_target`, `public_dns_target`, `denied_targets`) and [the network observer](../tools/acceptance/static_network_witness.py) to the private fixture. Run the following commands on their respective fixtures. The static observer attempts TCP and a UDP DNS datagram to every denial target. The TEST observer checks public TCP and UDP DNS. The static positive control requires a successful DNS response with a complete answer section and an IPv4 address for the queried name, either directly or through up to eight CNAME links. Any UDP datagram from a denial target fails that denial check.

   ```sh
   python3 static_network_witness.py --owner-qualification-fixture --role static inventory.json static.json
   python3 static_network_witness.py --owner-qualification-fixture --role test inventory.json test.json
   ```

3. Complete the rest of Design §2 in the separately qualified root-capable test environment. Exercise the static guest and its container, local firewall removal and DNS/route changes, pre/post-translation denial, DNS rebinding and UDP peer changes, IPv6 disabled, route drift during traffic, public TCP/UDP/DNS, and the localhost-only inbound forward. Verify the corresponding Windows/receiver capture records no forbidden connection or datagram, including traffic to the host services and through the slirp aliases. Record whether the gate closed on drift. A timeout or an unavailable listener alone is insufficient.
4. Assemble an owner-private JSON receipt with integer `version: 1` and the exact `qualification_sha256`, `m4_qualification_sha256`, `recipe_sha256`, `policy_sha256` and patched `qemu_sha256` from this reviewed qualification. Under `static` put the static observer's JSON. Add the observed `root_capable_fixture`, `ipv6_disabled` and `route_drift_closed` booleans, and the independently measured integer `witness_tcp_arrivals` and `witness_udp_arrivals` to **each** denial row. Under `test` put the TEST observer's JSON and its public controls' measured `witness_tcp_arrivals` and `witness_udp_arrivals`. Retain capture evidence privately. An independent reviewer checks the receipt against the qualification and captures. Put the reviewed file's path and SHA-256 into the plan. The driver checks the pins and all transport outcomes before daemon launch.

## Private local journey plan

Replace all placeholders with the qualified private values. The bearer credential applies to the workload HTTP admin endpoint. The origin CA file contains the current generation's DER certificate.

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
  "secret_root": {
    "volume_guid": "\\\\?\\Volume{<secret root volume GUID in lowercase>}\\",
    "volume_serial": "<16 lowercase hex digits from the secret root's volume serial>",
    "directory_file_id": "<32 lowercase hex digits from the secret root's directory file ID>"
  },
  "qualification_sha256": "<reviewed static/public-egress qualification SHA-256>",
  "m4_qualification_sha256": "<reviewed M4 qualification SHA-256>",
  "evidence_dir": "D:/acceptance/evidence",
  "backup_root": "D:/acceptance/backups",
  "restore_dir": "D:/acceptance/restore",
  "network_receipt": "D:/acceptance/private/reviewed-network.json",
  "network_receipt_sha256": "<digest of the reviewed network receipt>",
  "idle_delay_s": 660,
  "static_vm": {
    "name": "calm-life-static",
    "vm_id": "<dedicated acceptance VM ID>",
    "generation": "<active generation ID>",
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

## Run the local journey

Use [D: build scratch](dev-scratch.md) and verify the host reserve. Keep the owner's console unlocked and idle during the configured delay.

```powershell
$env:ARX_STATIC_VM_ACCEPTANCE_PLAN = 'D:/acceptance/private/static-plan.json'
cargo test --locked -p arxd --test static_vm_acceptance -j 3 -- owner::owner_host_static_vm --exact --ignored
```

Require a successful drain and successful owner evidence. The client demands a typed publishing refusal under the omitted setting and an empty publication ledger. It checks both HTTPS markers at the local origin, exact MP4 bytes and range headers (206/416), unavailable anonymous admin access, authenticated event visibility before and after VM restart, and the same event in a verified offline restore. Agent deletion of the static VM, project/volume and backup must be refused. The client checks the static disk/firmware identities, volume count and settled backup after the attempts. Leave the protected VM and backups in owner custody.

## Optional public tunnel journey

Only the owner runs this stage, after the default-off journey passes and the host and connector have qualified. Agents and CI use the fake transport tests. The local journey does not read tunnel credentials.

1. Prepare a second private plan for the same VM, generation, qualified releases and receipt. Change `daemon_config` to an owner-private public profile. Add `witness_dir`, a fresh owner-private D: directory disjoint from state, secrets, backup, restore and evidence. In the public profile set `allow_agent_public_sites: true`. Supply the B2 `publication` block with the acceptance `static_name`, `host_id`, one `tunnel_id`, `account_id`, `zone_id`, `zone_name`, sorted exact `hostnames` and matching ordered `routes` entries. Each entry has `hostname`, `project_id`, `service`, `project_slot`, `service_index` and `route_slot`. Supply `private_root`, `dns_ciphertext`, `tunnel_ciphertext`, and the qualified `connector`, `connector_length`, `connector_sha256`. Use the two approved names in the enrolled zone, assigned to the respective projects. Keep the current-user credentials in their enrolled private ciphertext leaves. See [ADR 0069](adr/0069-vm-store-v13-multihost-publish.md).
2. Prepare an independent public client on a different network with normal public DNS and trusted HTTPS roots. Transfer the [public observer](../tools/acceptance/static_public_witness.py) there. Keep the plan, workload credentials, VM identifiers and tunnel credentials on the owner host. Only the per-phase challenge and response need transferring.

   The observer allows 60 seconds in total for the HTTPS fetches in each phase, including DNS resolution, TLS, headers and response bodies. If a fetch reaches the deadline, the observer exits without a receipt. Cleanup DNS errors count only if they arrive before the deadline.
3. Start this exact ignored entry point in the owner console. Unset the opt-in after the run.

   ```powershell
   $env:ARX_STATIC_VM_ACCEPTANCE_PLAN = 'D:/acceptance/private/public-plan.json'
   $env:ARX_STATIC_VM_PUBLIC_TUNNEL = 'owner-opt-in'
   try {
       cargo test --locked -p arxd --test static_vm_acceptance -j 3 -- owner::owner_host_public_tunnel --exact --ignored
   } finally {
       Remove-Item Env:ARX_STATIC_VM_PUBLIC_TUNNEL
   }
   ```

4. The driver starts the pinned VM, deploys both releases, publishes through authenticated MCP and checks the two owned DNS rows, one parent tunnel and both verified route slots. In the new private subdirectory of `witness_dir`, transfer `first-request.json` to the independent client. Inspect the two exact names and paths, then run the command below. Transfer `first-response.json` back to the same subdirectory using a private temporary filename and rename it atomically after transfer. Each phase waits up to 15 minutes. The driver checks the fresh challenge, distinct markers, full-body digest, exact 206 slice and headers, and 416 for an unsatisfiable range.

   ```sh
   python3 static_public_witness.py --owner-live-public-https first-request.json first-response.json
   ```

   During this first public window, exercise video seeking in the browser. Use the qualified workload's event and admin endpoints to ingest a fresh synthetic event, verify anonymous admin refusal and authenticated visibility. Confirm bypass of shared caching for private routes and that a synthetic forged forwarding header cannot replace the real client address at the trusted proxy boundary. Record these manual workload observations privately before returning the response.
5. The driver stops and restarts the static VM, requires a new boot witness and empty publication state, and explicitly calls publish again. Repeat the same independent command and transfer using `republished-request.json` and `republished-response.json`. Verify the pre-restart event through the workload. No implicit republish qualifies.
6. After unpublish, the driver requires an empty local publication ledger and unpublished status. When `cleanup-request.json` appears, independently inspect the enrolled connector and its local ingress, and use the owner's Cloudflare zone dashboard to verify both owned DNS records have been removed. For each published phase, compare the two record IDs and markers in the owner zone dashboard with the private ledger query `SELECT hostname, record, marker FROM vm_publication_hosts ORDER BY position` opened read-only during that phase. For final cleanup use the pair saved during republish. After both records are absent and the enrolled connector has stopped, run the command below. It checks the two names no longer serve the markers. Return `cleanup-response.json` atomically. Preserve evidence and follow the existing owner cleanup procedure if any child stays ambiguous or cleanup-pending.

   ```sh
   python3 static_public_witness.py --owner-live-public-https --confirmed-connector-stopped-and-both-owned-records-absent cleanup-request.json cleanup-response.json
   ```

## Evidence and current blockers

Under `evidence_dir/<run digest>/`, the client writes `acceptance.json` and a SHA-256/size manifest. The evidence includes result codes, bounded measurements, timestamps and digests. Raw VM, template, recipe, activity, operation and backup identifiers are excluded. Keep the plan, qualification captures, per-phase witness exchange and independent manual observations private.

- M4 native admission and the daemon-selected static qualification and current-boot origin receipt are still required. Closed admission or a missing proof blocks the journey before the corresponding effect.
- Production enrollment of the Ubuntu Compose recipe, boot-time Compose/backup channels, host backup custody and capacity must be available. E2 enrollment runs at daemon startup and requires the committed VM row already present. The bootstrap and workload starts above are distinct.
- The qualified Compose guest disables legacy scripts. The separate root-capable qualification and restricted TEST witnesses must exist before either stage. The network observer only measures the paths described above. The reviewed §2 qualification and captures supply the remaining evidence.
- E3 installs multi-host routing during publish. If the qualified workload cannot serve its configured names at the private origin while policy is off, the local journey fails the routing check. Record the missing origin routing support rather than enabling publication to pass the default-off run.
- Cloudflare Tunnel publishing and other owner paths remain **unverified on real hardware** until completed under owner control and independently reviewed. Preserve uncertain custody, VM media and backups and follow the [host runbook](dev-host-setup.md).
