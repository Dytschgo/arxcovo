# M5 owner-host acceptance

Run this journey on the enrolled Windows x64 host under the owner's non-elevated
console login, after the [M4 owner-host acceptance](m4-acceptance.md) has a
qualified TEST runtime. The Rust integration test keeps the physical journey
owner-run. Its deterministic checks run in normal CI.

M5 reuses that client's foreground launch, authenticated local MCP bridge,
kernel PID binding, enrolled store pins, and evidence directory. It does not
open a second transport. The bridge still checks the server PID on the
owner-checked pipe before any credential byte, and the same connection carries
authentication and every MCP call.

The journey depends on the M4 selected TEST qualification. The plan pin is
required. The client reads `vm_catalog`'s `selected_test_qualification` receipt
and refuses to add a script unless that receipt matches the pin and both TEST
recipe IDs and pins. A missing pin, a missing receipt, or a mismatch stops the
journey before the idle delay and before any vault write.

Use the same reviewed foreground, owner-private TEST profile, and store pins as
the M4 runbook. Keep a Compose secret root out of this profile. Paths below are
placeholders. The owner-host commands are **unverified on real hardware**.

The principal's vault must be clean before the journey. M5 creates the only
script for that principal and reads one unpaginated audit page. A vault that
already has scripts, versions, or audit rows can fail after this run has
written its versions, including when every current-run row is present. Use a
dedicated principal, or an empty vault for this principal. Repeating the
journey on the same vault is not a valid acceptance.

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
  "qualification_sha256": "<reviewed M4 TEST qualification SHA-256>",
  "evidence_dir": "D:/acceptance/evidence",
  "idle_delay_s": 660,
  "ubuntu": { "recipe_id": "<Ubuntu TEST recipe ID>", "recipe_sha256": "<recipe SHA-256>" },
  "windows": { "recipe_id": "3c2fe2069942ce6c6cebe5c3e700e758", "recipe_sha256": "<recipe SHA-256>" }
}
```

`qualification_sha256` is not optional. The Windows recipe ID is the pinned TEST
recipe. The Ubuntu recipe ID is the other TEST recipe and must differ. Set
`idle_delay_s` to 660, or to 0 when the console is already inside the qualified
idle window. Build scratch, the binary, the plan, and the evidence stay on D:.

The deterministic checks do not start the daemon:

```powershell
cargo test --locked -p arxd --test m5_acceptance
```

Build the owner test ahead of the idle window, then run only the ignored journey.
The session code lives in the M4 acceptance binary so it can use that client's
bridge and store preflight.

```powershell
$env:ARX_M5_ACCEPTANCE_PLAN = 'D:/acceptance/private/m5-plan.json'
cargo test --locked -p arxd --test m4_acceptance --no-run -j 4
cargo test --locked -p arxd --test m4_acceptance -j 4 -- owner::m5::owner_host_m5 --exact --ignored --nocapture
```

The client checks vault and catalog schemas, then the qualification receipt.
After the idle delay it rechecks that receipt. It adds an Ubuntu script named
`m5-accept` whose body is `exit 0`, queues `vault_test_version`, and waits until
the tester records a pass. A pass is `state` `testing`, `test.status` `exited`,
`exit_code` 0, and `cleanup_confirmed` true. The client then calls
`vault_activate` and records `vault_list`. That list's `active_version_id` is
the active version before the broken add. It adds a second version whose body
is `exit 7` and queues that test. The checks bind every result to this run: the
script ID on the known-good version, the broken version ID from its add
receipt, and the tester operation ID from that version's submit receipt. The
started audit row and the failed `exited` audit row must both carry that
operation and version. `vault_activate` on the broken version must return
`pass_required`. The active version before the broken add and after the refused
activation must both be the known-good version, and that version's `state` must
be `active`. A mismatch or a second match fails closed. Activity rows for the
add, the passing test, the activation, and the refused activation must be
present. Any other shape fails closed.

The poll budget is 12 minutes. The tester's own lease is 10 minutes, so a clone
that does not finish inside that lease is a failed acceptance, not a longer
wait. Each run writes `acceptance.json` and `manifest.json` under a generated
run ID in the evidence directory. Evidence holds digests, the script ID, version
and operation IDs, test receipts, and the refusal. `vault_list` and the
acceptance proof keep the daemon's `active_version_id` from the list row, both
before the broken add and after the refused activation. Script bodies, raw
paths, identities, credentials, and raw MCP transcripts are excluded.

## Current refusal behavior

- The plan parser rejects a missing or null `qualification_sha256` before the
  daemon starts.
- An authenticated catalog with closed admission returns `admission_closed`
  with a reason such as `qualification_missing`. An open catalog without the
  selected-record receipt returns `daemon_qualification_evidence_unavailable`.
- The product catalog does not yet emit `selected_test_qualification`. That is
  the M4 7c receipt this journey waits on.
- `CandidateRuntime::capability` does not return `Qualified`. `LaunchAuthority`
  is uninhabited, so `vault_test_version` on the real foreground returns
  `vm_admission_closed` until a qualified runtime opens that gate. The client
  records the refusal and fails. This journey does not change that product
  behavior.
- Both journeys require the vault `oneOf` `list_activity` schema. The
  pre-vault activity object, and any other shape, is `schema_changed`.
- A broken version that activates, exits 0, skips cleanup, or replaces the
  active version fails the acceptance.

If the client starts the foreground, it signals that foreground in a test-owned
console on completion or refusal. A clean exit is required for a pass. Preserve
the ledger and vault after a timeout or uncertain release and use the
[host rollback procedure](dev-host-setup.md). The journey is **unverified on
real hardware**.
