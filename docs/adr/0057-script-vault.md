# 0057 — Foreground script vault

The general foreground local MCP Inventory serves caller-owned scripts. The
authenticated principal owns its per-OS name namespace, request receipts, and
activity entries. Only that principal can request content explicitly. HTTP,
the installed service, and the M4 VM profile have separate authority.

The vault database follows the VM store's private path and lock checks, pinned
SQLite main-file identity from ADR 0035, and full-sync rollback journal. It
limits named scripts to 128, versions per script to 64, stored script bytes to
1 MiB, and replay receipts to 512. Scripts match the guest contract of 1–4096
UTF-8 bytes with no NUL. The caller's request ID and bound arguments identify
accepted mutations for 24 hours. Cursors use restart-scoped HMACs bound to the
principal and listing, encoded as 82 lowercase hex characters.

Each principal can retain 96 versions and 256 KiB of script bytes across both
OS namespaces. Every named script has at least one version, so one principal
can occupy at most 96 of the 128 name slots. Vault versions are retained
indefinitely and MCP provides no deletion. Several principals can still fill
the host together. Capacity reclamation requires a separate owner-maintenance
design.

Without a daemon-qualified `VaultTester`, test admission is unavailable. An
adapter receives immutable bytes and a bound completion receipt after the
testing intent commits. A zero exit code and confirmed throwaway teardown
record the pass. The passing version remains in testing until activation. A
restart settles unresolved intents as rejected. Activation atomically marks
the prior active version superseded and advances the active version. Each
script can have one active version after its first activation.

Audit events commit with the vault mutation and appear in `list_activity`
with `source: "vault"`. This preserves atomicity without a two-database commit
or a legacy activity migration. Audit holds fixed labels, timestamps, and IDs.
Read projections join the content hash. Only the version table stores bytes.
Production builds cap tracing and its log fallback at INFO. The SDK's debug
events include full tool arguments and results, including script bytes.

`template_tools` reports the pinned Ubuntu recipe's declared guest agent and
the unavailable Windows recipe. Observed tools and sealed instance IDs need
evidence from the M4 journey.
