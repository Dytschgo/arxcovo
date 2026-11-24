# M4 owner-host acceptance

Run this journey on the enrolled Windows x64 host under the owner's non-elevated
console login, after the [host runbook](dev-host-setup.md) and independently
reviewed native qualification and production issuance are complete. The ignored
Rust integration test uses the same authenticated stdio bridge as the other local
MCP tests. Keeping it in that test target gives the deterministic contract checks
normal CI coverage and keeps the physical journey owner-run.

Use the reviewed foreground executable and an isolated owner-private TEST profile
with one acceptance principal, public publishing disabled, and a fresh endpoint.
Keep the daemon configuration, client configuration, and enrollment receipt
private. The [local bridge setup](windows-execution-accounts.md) describes the
client configuration and server identity pin. Use the approved capped store with
a fresh acceptance state directory and media root, or one containing only the two
qualified TEST goldens and historical receipts. The media root follows the slice 6
layout, with generation directories containing `disk-<key>` and `vars-<key>`
leaves. The client checks that the isolated ledger and physical inventory agree
before creating a clone. It also checks the stopped store before launch because the foreground sweeps at startup.

Prepare an owner-private JSON plan on D:. Use lowercase hex digests from the
reviewed release and enrollment. Point the plan at the exact qualification record
selected by the daemon configuration. The recipe IDs and SHA-256 values identify the
two exact TEST recipes in the qualification scope. Paths below are placeholders.

```json
{
  "daemon": "D:/acceptance/bin/arxd.exe",
  "daemon_sha256": "<reviewed release SHA-256>",
  "daemon_config": "D:/acceptance/private/daemon.json",
  "client_config": "D:/acceptance/private/client.json",
  "qualification": "D:/acceptance/private/qualification.json",
  "qualification_sha256": "<reviewed qualification SHA-256>",
  "artifact_root": "D:/acceptance/media",
  "evidence_dir": "D:/acceptance/evidence",
  "idle_delay_s": 660,
  "ubuntu": { "recipe_id": "<Ubuntu TEST recipe ID>", "recipe_sha256": "<recipe SHA-256>" },
  "windows": { "recipe_id": "3c2fe2069942ce6c6cebe5c3e700e758", "recipe_sha256": "<recipe SHA-256>" }
}
```

Use an existing evidence directory outside the state and media roots. Set
[build scratch paths on D:](dev-scratch.md) and build the test ahead of the idle
window. Set `idle_delay_s` to 660 to let recent console input age out, or use
0 when the unattended run is already inside the qualified idle window. Leave the
console active and unlocked and allow up to five hours for first-time template
builds, boot and the real leases.

```powershell
$env:ARX_M4_ACCEPTANCE_PLAN = 'D:/acceptance/private/plan.json'
cargo test --locked -p arxd --test m4_acceptance --no-run -j 4
cargo test --locked -p arxd --test m4_acceptance -j 4 -- owner::owner_host_m4 --exact --ignored --nocapture
```

The client starts its own foreground and uses one authenticated MCP session. It
checks tool schemas and admission, selects or builds both qualified goldens, and
checks the clone pairs under 30-minute Ubuntu and 40-minute Windows leases. Boots
run sequentially. The Ubuntu guest changes only a disposable marker and returns
exit 7 before offline restore replaces the generation. The foreground's normal
sweep retires the old pair and expires the live pairs and the TEST snapshot. The
client checks the restored bytes, fresh boot and original marker before verifying
expiry and remaining charges. Daemon expiry appears through the `vm_get`
tombstone and the local ledger. The same session's `list_activity` supplies the
caller operation receipts.

Each run writes `acceptance.json` and its SHA-256 and length in `manifest.json`
under a generated run ID. Evidence includes the binary and qualification digests,
request and operation IDs, guest exits, generation and lease references,
read-only custody predicates, receipt attribution, and drain result. Keep the
evidence local until the owner reviews it for release. Script bodies, raw paths,
identities, credentials, and raw MCP transcripts are excluded.

Closed admission returns an `admission_closed` failure with a typed reason
such as `qualification_missing`. A diagnostic run can omit both qualification
fields. Missing or changed tool schemas fail before mutation, and an open
catalog still requires the pinned qualification. On success or failure the client
ends the bridge and signals the foreground in a test-owned console. A clean
foreground exit is required for the pass. For a timeout or uncertain release,
preserve the run's ledger and charged artifacts and use the
[host rollback procedure](dev-host-setup.md). The client uses no force stop for the
foreground or guest processes. The journey is **unverified on real hardware**.

## M5 journey

The M5 mode follows once the production vault tester wiring merges. The TEST VM
foreground returns before inventory initializes the vault, and the inventory
vault currently opens without its trusted throwaway-VM tester.
[Foreground dispatch](../crates/arxd/src/main.rs#L99-L108) ·
[Vault initialization](../crates/arxd/src/vault/mod.rs#L46) ·
[Tester contract](../crates/arxd/src/vault/mod.rs#L205-L212)
