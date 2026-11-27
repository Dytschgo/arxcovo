# Static VM and Compose owner acceptance

The local journey checks Calm Life and a synthetic second project through a PID-bound, authenticated foreground MCP session. A separate ignored test drives the public tunnel. Both require a qualified Windows x64 host and the owner's non-elevated login. Native VM boot, Docker, public-only networking, Windows custody, persistence and live Cloudflare publishing remain **unverified on real hardware**. The procedures below require owner execution and independent review before live acceptance. See [ADR 0058](adr/0058-v1-scope-static-vms-and-cloudflare.md), [ADR 0064](adr/0064-v1-scope-docker-static-vm.md) and the [M4 host procedure](m4-acceptance.md).

## Native adapter status

The Windows x64 foreground accepts the owner-local `test_vms.static_compose` opt-in with `test_vms.enabled: true`. It verifies and holds the enrolled roots before opening the store and serving MCP. It can verify a Docker-class dossier against independently held inputs, a separately pinned Design §2 receipt bound to the selected M4 TEST seal, and the live IPv4 topology. `--full-docker` preflight reads the M4 TEST selection from the pinned Docker runtime profile. It verifies the selected TEST seal with the production store stopped, then checks the Compose recipe, stock and patched distributions, fixture inputs, policy and live route topology. After preflight passes, Part B2g claims fresh private evidence and a journaled disposable workspace for the native Docker trial. The reviewed comparison witness is added for foreground selection after dossier emission because it names the final seal digest.

`vm_catalog` reports the selected Compose recipe and build availability while the Windows x64 checked static runtime, M4 qualification, Docker dossier, network witness, root custody and route watch are current. If any proof is unavailable, `capability.static_compose` reports `network_fence_unverified` or a more specific refusal and the selected Compose recipe is withdrawn. The fence covers fresh static admission, including publishing after its policy check. The live boot owner can still stop an owned run after the watch retires.

Every configured root must exist, have an owner-private ACL with inheritance disabled, and be free of reparse points. The state, dossier, optional secret and optional backup roots must be pairwise disjoint. Pin each independently using its volume GUID, volume serial and directory file ID.

- The state root requires `test_vms.expected_store`. If `test_vms.compose_secret_root` is set, supply `test_vms.expected_compose_secret_root`.
- `test_vms.static_compose.selection` requires the Docker-class `enrollment`, `dossier`, `expected_dossier_root`, `seal_sha256` and `scope`. Keep the private enrollment file outside the held roots. The TEST dossier has its own selection and grant. Foreground Docker verification uses that qualification's held Ubuntu inputs and storage scope and binds the separate network comparison to its seal. Each file input in the Docker enrollment carries an exact local `path`, `size` and lowercase `sha256`, including `network_witness`. The witness file is private and outside the dossier and production roots. The daemon verifies the witness against both selected seals, the recipe, the policy and the patched executable before using its public targets to sample routes.
- The pinned Docker `runtime_profile` must select the same M4 TEST seal as foreground. The pinned `fixture_network_profile` is a JSON object containing exactly `public_tcp_target` and `public_dns_target` as IPv4 socket addresses. The latter uses port 53. The independently reviewed comparison must match both addresses and ports. The receiver enrollment includes TCP and UDP for the Windows loopback case (`HostLoopback`) in addition to the host interface and translated aliases.
- Optional `test_vms.static_compose.backup` requires an existing D: `root`, its `expected_root` pin and bounded `targets`. Each target has `vm_id`, `project_id`, `volume`, `database` and `max_bytes`. With a verified selection, startup commits releases and backup targets in one transaction for VMs of the selected sealed recipe. Releases with secret references require the held DPAPI root. The backup path samples capacity from held volume handles at admission and rechecks it before guest work and host staging.
- Optional `private_routes` carries the enrolled `vm_id`, `zone` and sorted `routes`. Use the same fixed hostname, project, service and slot fields as the publication block. The verified origin's serial session installs these bindings before the start settles, including on the first boot before project deployment. The guest reserves each enrolled project slot and checks the service and HTTP port at deployment. Private installation needs no Cloudflare credentials or public setting. If private and public bindings are both configured for the VM, their union must fit one valid routing table in the configured zone.

The selected recipe ID derives from the Compose recipe class and digest. Its sealed template is ineligible for a TEST clone. Static lifecycle work uses a separate dispatch boundary from TEST. The controller refuses agent scripts on running Compose static VMs. The topology fingerprint covers the enrolled uplink, competing defaults, site LAN routes and public-numbered routes on secondary interfaces. Relevant address, route and IPv4 interface notifications retire the watch. Fresh samples also check link state and the pinned baseline. A failed route read retires the watch for this foreground selection, even when a later read would succeed. An observation gap cannot establish continuity.

The boot settlement adapter commits the static start and serial boot witness together before publishing the lifecycle and backup sessions. Both share one pipe and the verified origin's retirement signal. Deterministic tests cover mismatched origin identity, a selection that retires during the transaction, and loss of backup capacity across effect boundaries.

The deterministic Docker collector now negotiates real loopback TLS in its fake-QEMU journey before emitting a seal that the held foreground dossier reader verifies. The endpoint trial returns a fresh public certificate over the attributed serial pipe. The collector requires the exact certificate and boot-specific SNI and health body, between listener ownership checks. The detached boot stays offline, and the trial key remains in the endpoint guest's private runtime directory. The Compose install, detached, and public-egress argv require the enrolled 2 vCPU, 4 GiB RAM and 32 GiB disk envelope. See [ADR 0074](adr/0074-bind-docker-trial-https-to-serial.md).

The Docker collector requires a signature from the enrolled independent
receiver before it releases guest execution. Its capture mailbox uses a
private, held directory and bounded files. The result must match the exact
challenge and measurement window. Windows holds an exclusive IPv4 loopback
socket through checked suspended creation. The collector consumes the
reservation once and requires attribution to the held child before allowing
guest execution. The deterministic journey continues through HTTPS and the
held foreground reader. See [ADR 0075](adr/0075-authenticate-docker-receiver-capture.md).

Part B2g wires the [Windows owner qualifier](docker-qualification.md) to the native QEMU phase host. It stages the fixture, runs the restricted installer and detached health boot, and verifies the public-egress endpoint on a separate copy. It checks the suspended child, attributes QMP, serial and the loopback listener, and requires checked shutdown and disposable release before sealing. One watch covers the pre-execution fixture checks. A signed drift result and settled fixture permit a fresh watch only after the enrolled route is restored. That watch covers the remaining phases through dossier publication. [ADR 0076](adr/0076-native-docker-qualification-phases.md)

Part B2h adds the Windows x64 `SelectedStatic` adapters for qualified template build, independent full copy, checked start and checked stop. It keeps the storage and process authority separate from TEST clones. A checked start attributes QMP, the guest serial pipe and the loopback listener to the held child, then verifies boot-bound HTTPS before settling the start. Compose start publishes the lifecycle and backup sessions together. Restart recovery keeps the same committed generation, volumes and backups and requires the exact workspace lease and writer exclusion on both media files. See [ADR 0077](adr/0077-native-static-lifecycle.md).

The installed guest must include the static origin handshake. Qualify the M4 and Docker dossiers again with that guest build. The guest keeps the generation CA private key and the daemon pins the public DER at `origin-ca/origin-ca.der` inside the generation workspace.

Part B2i installs the enrolled private routes with publishing off, activates and cleans up Cloudflare on explicit opt-in for the same verified boot, and opens static admission and catalog availability only with live route monitoring and the remaining qualification checks. Only an explicit publish call for a new verified boot can reactivate public ingress after a stop or restart. A retired watch closes ingress and leaves unfinished owned DNS cleanup in the ledger for reconciliation. See [ADR 0078](adr/0078-watched-static-routing-and-publication.md). Native VM boot, Docker, public-only networking, Windows custody, persistence and live Cloudflare publishing remain **unverified on real hardware**.

## Enroll a dedicated acceptance VM

1. Complete M4 host setup, independently review the M4 and patched public-egress qualification receipts, and qualify the digest-pinned Calm Life release. Prepare a new isolated state directory on D: and enroll its volume and directory identities using the M4 procedure. Never select an existing owner workload for this journey.
2. Start the qualified foreground daemon against that store with `test_vms.enabled: true`, `test_vms.expected_store`, the enrolled `test_vms.static_compose.selection` and one acceptance principal. Omit the Compose secret root, its pin, the release list and publication configuration during bootstrap. Use the authenticated local MCP client to call `vm_catalog`. Select the qualified Ubuntu Compose recipe and its sealed template. If needed call `vm_template_build` with the catalog recipe ID and a fresh request key, then poll `vm_operation` until it succeeds. Mutation keys consist of 16 lowercase hexadecimal digits for current Unix milliseconds followed by 16 random hexadecimal digits. Reuse the same key only for a retry of the same request.
3. Call `static_vm_create` with that template ID, a unique acceptance name and a new request key. Poll the returned operation to success. Verify `static_vm_get` reports the expected VM ID, name, stopped state and recipe digest. Drain the bootstrap daemon. In the stopped owner-private ledger, use a read-only SQLite connection to run `SELECT lower(hex(id)) FROM vm_generations WHERE lower(hex(static_id)) = ? AND active = 1` with the acceptance VM ID as parameter. Put that generation and VM ID into the private plan.
4. Prepare the workload foreground config for the same store, endpoint and principal. Set `test_vms.compose_secret_root` to a disjoint owner-private directory. Supply the current-user DPAPI ciphertext as an uncompressed file named `<reference>.dpapi` for the workload's admin secret. Enroll the two local workload names and their slot bindings under `test_vms.static_compose.private_routes` for this VM. In `test_vms.compose_releases`, supply exactly the two qualified entries. Each has this VM's `vm_id`, its own `release_id`, the typed `manifest` (including `project_id`), and the enrolled `secrets` slot/epoch/reference list when needed. Use the reviewed image digests and actual workload configuration. On the driver's next daemon launch, arxd enrolls these releases against the already committed static VM. Before boot the client verifies both release rows, the caller, name, recipe, template and generation.
5. Capture the secret directory's identity independently using the same procedure as for the store. Put those identities in the private plan as `store` and `secret_root`. Omit `expected_store` and `expected_compose_secret_root` from the source profiles for both the local and public stages. The client's `pinned_config` step inserts only the store and Compose secret pins into a private launch copy. Keep `static_compose.selection.expected_dossier_root` and, when backup is configured, `static_compose.backup.expected_root` in the source profiles. The client checks and holds all configured roots before launch. The daemon verifies the same pins before opening the store. For the default-off run also omit both `allow_agent_public_sites` and `publication` from `test_vms`. Supply existing disjoint owner-private evidence, backup and restore directories. Keep credential files private, local and outside those roots. Plan and config paths must be on D: without reparse points.

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
      "127.0.0.1:443",
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

To create the two ciphertext leaves, the owner runs [`scripts/seal-cloudflare-credentials.ps1`](../scripts/seal-cloudflare-credentials.ps1) with PowerShell 7.2 or later, non-elevated, in their own login, for example `-Mode Both -TargetDirectory D:\ArxcovoData\<private-root> -CreateDirectory -AccountId <account_id> -TunnelId <tunnel_id>`. It writes `dns.dpapi` and `tunnel.dpapi` for `dns_ciphertext` and `tunnel_ciphertext`, and the target directory is the profile's `private_root`. The DNS API token and tunnel credential JSON are read only from a secure interactive prompt or, with `-FromStdin`, from redirected standard input, one leaf per run. They are never accepted as arguments or environment variables, never logged, and the script refuses to run while a transcript is active. A DNS token piped with a trailing newline is refused. It checks the DNS token with the same rules arxd applies when it loads it. For the tunnel credential it accepts only a strict subset of what arxd accepts: a plain JSON object with unescaped keys and string values, as cloudflared writes it. Every sealed file is therefore accepted by arxd, and escaped key spellings or the JSON array form are refused. It stores current-user DPAPI ciphertext only under `D:\ArxcovoData`. Each folder is opened relative to its already-open parent, never through a reparse point, and the target must be an owner-private directory. It never overwrites an existing leaf and deletes a new leaf through its own handle if a read-back check fails. It prints only paths, ciphertext lengths and SHA-256 digests. It never edits the daemon configuration, so public publishing stays off by default. [`scripts/test-seal-cloudflare-credentials.ps1`](../scripts/test-seal-cloudflare-credentials.ps1) exercises it with synthetic values only. Live Cloudflare publishing with these leaves remains **unverified on real hardware**.

1. Prepare a second private plan for the same VM, generation, qualified releases and receipt. Change `daemon_config` to an owner-private public profile. Add `witness_dir`, a fresh owner-private D: directory disjoint from state, secrets, backup, restore and evidence. In the public profile set `allow_agent_public_sites: true`. Supply the B2 `publication` block with the acceptance `static_name`, `host_id`, one `tunnel_id`, `account_id`, `zone_id`, `zone_name`, sorted exact `hostnames` and matching ordered `routes` entries. Each entry has `hostname`, `project_id`, `service`, `project_slot`, `service_index` and `route_slot`. Supply `private_root`, `dns_ciphertext`, `tunnel_ciphertext`, and the reviewed Windows x64 `connector`, `connector_length` and `connector_sha256`. Review the connector release provenance and runtime dependencies before enrollment. Use the two approved names in the enrolled zone, assigned to the respective projects. Keep the current-user credentials in their enrolled private ciphertext leaves. See [ADR 0069](adr/0069-vm-store-v13-multihost-publish.md).
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
- The local journey requires the enrolled private routes and matching project/service slots. The public stage separately verifies the exact public hostname bindings and the current boot before opening ingress.
- Cloudflare Tunnel publishing and other owner paths remain **unverified on real hardware** until completed under owner control and independently reviewed. Preserve uncertain custody, VM media and backups and follow the [host runbook](dev-host-setup.md).
