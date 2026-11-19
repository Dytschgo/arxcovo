# ADR0033: Offline startup identity preparation

Status: design accepted; candidate implemented, source acceptance and hosted gates pending.
Date: 2026-11-19.

## Context

PR186 provides the private required peer RPC boundary. Live startup still needs
credential custody, legitimate founding, enrollment continuation and admission.
Those unresolved dependencies must not be hidden inside a startup flag. A useful
prior slice can validate private owned layout, public identity consistency and
the actual recovered clock floor without starting an Engine or releasing keys.

## Decision

Add one private offline preparation boundary. Its result retains real outer
ownership and checked object identities; it is neither credentials nor readiness.
The daemon configuration and production entry points remain unchanged.

Use distinct create-founder, join, recover-founder and recover-joiner modes.
Fresh modes require no not-yet-issued identity record and refuse occupied remnants.
Recovery requires a matching typed founder or joiner record. Founder N1 has a
Found origin and no join-anchor exception. Joiners N>=2 retain original Redeem E
and canonical anchors excluding their own N. Validate public data against a
separate trusted expected tuple: leaf DER does not authenticate origin or anchors.
Opaque sealed-artifact presence proves no unsealing, key possession or isolation.

Recover validates existing initialized stores read-only before create-capable
openers. Missing/empty/version0/partial/corrupt layouts and recovery sidecars refuse
without changing bytes or entries, including locks. Existing required lock files
must already exist. Fresh lock creation is permitted only after verified fresh
layout qualification. No schema migration or repair is introduced.

Read the validated persisted committed_ms, run existing preflight and require the
first checked-clock sample under ADR0030's30000ms skew rule. Drop store handles
while retaining outer ownership. Fresh no-files preparation supplies no fabricated
persisted floor or identity. Exact first_free/next2 belongs only to verified
untouched genesis; occupied historical provenance remains unavailable. Requests
requiring that unavailable proof refuse. No floor sidecar or schema field is added.

## Consequences

The source allowlist has six Rust paths and ordinary source/behavior review and
CI gates. No Engine/network execution, founder repair, credential release or
production listener belongs to this slice. Existing engine/start/shutdown/storage
cores, PKI authority, dependencies, quarantine and capacities remain unchanged.
The offline change is reversible; actual durable key publication/founding needs
a distinct reviewed contract and its own recovery evidence.

The next live slice still needs a legitimate founder credential bridge, qualified
sealed custody, same-connection enrollment continuation, receipt-scoped catch-up,
admission/promotion, clock wiring and truthful uncertain-start ownership. Containers,
remote execution and real-time expiry acceptance remain open. Engine Sol3/3 and
historical Opus0/2 remain unchanged. M2 OPEN / M3 UNCHECKED; hardware remains
**unverified on real hardware**.

See the [implementation contract](../briefs/m3-startup-identity-preparation.md).
