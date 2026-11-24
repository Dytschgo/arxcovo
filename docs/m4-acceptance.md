# M4 owner-host acceptance

Run this journey on the enrolled Windows x64 host under the owner's non-elevated
console login, after the [host runbook](dev-host-setup.md) and independently
reviewed native qualification and production issuance are complete. The Rust
integration test keeps the physical journey owner-run. Its deterministic checks
run in normal CI.

Use the reviewed foreground executable and an owner-private TEST profile with one
acceptance principal, public publishing disabled, and a fresh endpoint. Keep the
daemon configuration and client configuration private. The
[local bridge setup](windows-execution-accounts.md) describes the server identity
pin. The client derives the media inventory from the foreground's configured
custody root. Before startup, it verifies an empty store or an idle ledger
at the VM store schema version compiled into the client. The reader refuses
older, newer, or unknown versions. The only media allowed are the selected TEST
goldens. The scan includes generation
directories, `disk-<key>` and `vars-<key>` leaves, and the ledger metadata.
The root may also contain the foreground vault database, lock and DELETE-mode
rollback journal as regular files.

Prepare an owner-private JSON plan on D:. The qualification digest is an expected
pin. The client records a digest only after an authenticated daemon receipt
identifies the selected record and matches the expected digest and both TEST
recipe IDs and pins. Paths below are placeholders.

```json
{
  "daemon": "D:/acceptance/bin/arxd.exe",
  "daemon_sha256": "<reviewed release SHA-256>",
  "daemon_config": "D:/acceptance/private/daemon.json",
  "client_config": "D:/acceptance/private/client.json",
  "store": {
    "volume_guid": "\\\\?\\Volume{<lowercase volume GUID>}\\",
    "volume_serial": "<16 lowercase hex digits from FILE_ID_INFO.VolumeSerialNumber>",
    "directory_file_id": "<32 lowercase hex digits from FILE_ID_INFO.FileId.Identifier>"
  },
  "qualification_sha256": "<reviewed qualification SHA-256>",
  "evidence_dir": "D:/acceptance/evidence",
  "idle_delay_s": 660,
  "ubuntu": { "recipe_id": "<Ubuntu TEST recipe ID>", "recipe_sha256": "<recipe SHA-256>" },
  "windows": { "recipe_id": "3c2fe2069942ce6c6cebe5c3e700e758", "recipe_sha256": "<recipe SHA-256>" }
}
```

Take all three store pins from the independently verified production enrollment.
The serial is the numeric value rendered in hex, and the directory ID is the
identifier bytes in order. The configured state and runtime state must name
the same directory under the configured runtime drive root. The production
store can use another mounted drive letter when the enrollment verifies its
backing on D:. The client compares the live mount and the held directory
identity with the plan and refuses changed mounts, aliases and reparse points.
It checks the ledger through the enrolled volume path and creates a temporary
configuration alongside the private daemon configuration. The daemon checks
all three enrollment pins on its held directory handle before opening state,
then opens the ledger and vault through the held volume path. The client uses
that volume identity for live ledger and artifact reads and rechecks the
binding before and after each batch. The configured drive spelling is used
as the expected qcow backing header value.

The acceptance run trusts the enrolled owner account. The client refuses when
it detects a drive rebind. Deliberate races by the owner against their own
acceptance run are outside this threat model.
[ADR 0054 same-account boundary](adr/0054-m4-dedicated-windows-runtime.md#storage-and-owner-activity)

Use a TEST-only profile without a Compose
secret root. Config load refuses the combination of an expected store identity
and a Compose secret root before any state opens.
Keep the build scratch, binary, configuration, plan and evidence on D:.

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

The client checks the kernel server PID on the owner-checked named-pipe handle
against its held foreground child before writing any credential bytes. The
same connection carries authentication and every MCP call. A missing PID or
a mismatch stops the journey.
The journey checks tool schemas and admission, selects or builds both qualified
goldens, and checks the clone pairs under 30-minute Ubuntu and 40-minute Windows
leases. The client polls both leases during the Ubuntu expiry wait and continues
polling Windows through its expiry. It rejects early teardown or more than one
second of disagreement between UTC and monotonic elapsed time. During the final
three seconds it schedules polls 100 ms apart. An expiry also requires a stopped
reply to a request sent no earlier than 250 ms before the monotonic deadline.
Using the request start gives a conservative lower bound on when the daemon
observed the stopped VM. An observation gap or stalled read can cause
`lease_timing_inconclusive`. The remaining early-expiry tolerance is 250 ms
relative to the monotonic deadline, and each wait ends by that deadline plus
three minutes. Boots run sequentially. The Ubuntu guest changes only a disposable marker and returns
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

## Current refusal behavior

- An authenticated catalog with closed admission returns `admission_closed`
  with a reason such as `qualification_missing`. An open catalog without the
  selected-record receipt returns `daemon_qualification_evidence_unavailable`.
  The journey waits on 7c's selected-record receipt, including both TEST recipe pins.
- Windows readiness requires a native guest-handshake witness tied to the VM,
  active generation, and current start operation. A ready flag and a ledger
  admission epoch leave `windows_boot_evidence_unavailable` until the daemon
  exposes that witness.

If the client starts the foreground, it signals that foreground in a test-owned
console on completion or refusal. A clean exit is required for a pass. Preserve
the ledger and charged artifacts after a timeout or uncertain release and use the
[host rollback procedure](dev-host-setup.md). The journey is **unverified on real
hardware**.

## M5 journey

The TEST VM foreground opens the vault with its throwaway-VM tester after the
startup sweep. M5 acceptance requires a qualified TEST golden and its own
vault journey. [Foreground vault wiring](../crates/arxd/src/local.rs#L271-L273)
