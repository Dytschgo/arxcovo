# Offline startup identity and recovery preparation

Accepted technical design, 2026-11-19; candidate implemented, source acceptance pending. Origin contract:
`D:/Temp/arx-m3-startup-contract-v2/contract.md`,21916bytes,SHA256
`fe9bcaaa0d9fbe2aeb89edebc0e0cea6eb0f3b272d580a857b095934fefe930f`.
Independent Sol6.1/high Design GO: `D:/Temp/arx-m3-startup-contract-review-v2/report.md`,
Standards0/Spec0. All27source pins match merged main eea7af57. Main's first
CI44432988620 qualified560wall/556charged before the source assignment.

## Outcome and scope

An internal bounded preparation call returns an owned offline observation or a
static refusal before any Engine, network, key release or readiness. It retains
the actual outer OS lock and checked identities for later transfer. It does not
prove custody, repair a store or create an active daemon configuration.

| Mode | Public identity requirement | Result boundary |
| --- | --- | --- |
| create-founder | No not-yet-issued record | Owned fresh layout; no credentials/ready |
| join | No not-yet-issued record | Owned fresh layout; no code/proof ingress |
| recover-founder | FounderIdentity N1 / Found origin | Matching trusted role and public consistency; no join anchors |
| recover-joiner | JoinerReceipt N>=2 / original Redeem E | Matching trusted role/origin/anchors; ownN excluded; no restart Redeem |

Fresh modes refuse occupied state, identity remnants or partial recovery instead
of inferring founding from a missing DB. Fresh no-files and initialized untouched
genesis are distinct cases. No-files gives no fabricated persisted floor/identity.
Only a verified untouched genesis can establish exact next/first_free2. Occupied
current next does not prove historical first_free; requests needing that proof refuse.

## Required boundaries

- Strict internal plan input <=16KiB, unknown fields denied; explicit bounded
  numeric peer/enrollment addresses and checked disjoint absolute paths. Preserve
  MCP listener restrictions. No recursive mkdir or occupied-directory replacement.
- Closed versioned role-tagged public identity codec <=8192bytes, exact consumption,
  bounded DER and canonical unique anchors, checked artifact basename. No key,
  code, proof, verifier or claimed historical-genesis field. Freeze concrete codec
  bytes with independent literal fixtures. Check CA/chain/SAN/SPKI against a separate
  trusted expected role/cluster/N/key/CA/origin/anchor tuple, never the same untrusted
  record re-labelled expected. DER does not authenticate E or anchor provenance.
- Retained directory/file identity and real exclusive outer lock, plus existing
  store locks. Reject alias/overlap/reparse and object replacement across phases.
  Unsupported binding guarantees yield unavailable. This does not defend against
  arbitrary external same-account writers or qualify daemon/build isolation.
- Recovery requires existing validated owner/store lock entries. No lock creation
  may precede an unchanged-refusal claim. Inspect SQLite read-only/no-create before
  mutable openers: actual supported schema, types/counts/bytes/pages and canonical
  images. Reject empty/version0/partial pairs and sidecars needing recovery; an
  immutable URI must not hide pending WAL/journals. Refusal preserves every file's
  bytes and directory entries. No schema/format/migration/repair changes.
- Reuse StateStore::inspect_snapshot. In the new helper, bounded read-only log
  validation uses current ARXLM1/version2 and ARXLE2 grammar, checksums/canonical
  equality/index/tail/purge and <=256metadata/4163row/512live+one overflow sentinel,
  reusing accessible command, position and membership codecs. Private log-store
  inspectors are not callable and the store core is outside the allowlist.
- Only after validation use existing open/preflight, obtain actual persisted
  committed_ms through one read-only accessor, construct HostClock and require its
  first sample. Preserve the boundary wall+30000<floor; do not refuse every wall<floor.
  Drop stores while retaining outer ownership; no floor sidecar or active Engine hook.
- Public artifact presence is preparation only, never key possession, successful
  unsealing, authenticated history, credentials or readiness. Missing required
  record/artifact/trusted expectation refuses. Actual custody/publication waits.

## Six-path source allowlist

1. New `crates/arxd/src/cluster/startup_plan.rs`: parser/layout/outer owner and inline tests.
2. New `crates/arxd/src/cluster/join_receipt.rs`: typed public records/validation and inline tests.
3. `crates/arxd/src/cluster.rs`: two private declarations only.
4. New `crates/arxd/src/cluster/consensus/startup_recovery.rs`: private composition and inline tests.
5. `crates/arxd/src/cluster/consensus.rs`: one private declaration only.
6. `crates/arxd/src/cluster/enrollment/registry.rs`: read-only cluster-visible committed_ms accessor/direct tests only.

No new dependency, feature, storage/engine/start/shutdown/Owners, PKI, transport,
config/main/service, workflow, quarantine or capacity change. No optional quarantine
guard update is required. Report a concrete necessary expansion for scoped review
before editing outside these paths. The coordinator owns ADR/brief/evidence/PROGRESS.

## Meaningful evidence and gates

Use disposable synthetic fixtures only: exact founder/joiner records and trusted
wrong-E/key/CA/SAN/anchor/cross-role/codec/basename refusals; real empty SQLite,
schema/version0, partial pairs and independently corrupted current images/log rows
refuse unchanged including locks/sidecars; positive actual-store floor/current
binding and reopen/skew-edge checks; duplicate owner retention, aliases, reparse,
object swap and unsupported identity refusal. Independent expected outcomes must
exercise the API rather than mirror fields. Unsupported platform paths stay labelled.

Run only named new non-Engine tests locally, fmt and locked/offline all-target
Clippy. No Engine/founder/native-executor/production/client/container runtime.
Independent exact-source review and normal first-attempt cross-platform required
CI remain mandatory. Every command/delegate sets D: TMP/TEMP/Cargo/cache/observer
paths and verifies Cfree>=20GB under dev-scratch.md. Preserve first failures and
existing exhausted counters. No owner admin or protected-resource actions.

Live startup, sealing/founder credential-cycle and checked signing observations,
same-connection enrollment continuation, receipt-scoped replication, admission/
promotion, CA distribution and uncertain startup ownership need a separate reviewed
contract. Remote build routing and real900000ms expiry remain open. No mem_net,
direct3voter initialization or seeded acceptance substitutes for genuine containers.
M2 OPEN / M3 UNCHECKED; hardware **unverified on real hardware**.
