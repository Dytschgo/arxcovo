# Membership-v1 extraction contract review

Root Sol6.1 medium, 2026-11-17. Separate-author review of Sol high brief
15e4adbd against actual consensus codec, its tests, StateStore MembershipBlob
and enrollment Reader. Owner routing permits separate Sol review.

## Standards

PASS with the appended implementation clarifications. The shared module is
std-only and engine-independent; wrappers keep existing signatures and errors.
Its private slice cursor avoids the enrollment Reader's StateStore coupling.
No caller, storage format or acceptance policy changes in this slice.

## Spec

PASS. Literal U/J/D lengths and layout agree with existing v1. Existing tests
pin the 3605-byte maximum, every truncation and invalid domain. Duplicate joint
configs are accepted today and must stay ordered and duplicated. All metadata,
including learners, is part of the exact wire contract, not just voter sets.

Root requires wrapper golden/error tests and the two old pure codec tests in
the local focused selection. New pure tests alone cannot prove the adapters
preserved OpenRaft conversion and StoreError normalization. For mutation2, a
direct constructor assertion checks duplication before secondary encode-back
validation can mask the intended assertion. No engine test is needed locally.

## Security

PASS for this extraction only. Byte/count/address bounds precede allocation;
constructors validate invariants and fields remain immutable. Existing broad
membership address syntax remains unchanged; endpoint validation belongs to a
different contract. Opaque StateStore/Blob acceptance is not tightened here.
Future apply-time decode, join anchors, schema/custody and transport remain open.

Actual work: static source/test/brief review and literal-layout arithmetic.
No build, test, mutation, network or hardware action. Compile/runtime feasibility
is unproven. Root permits at most two mechanical compile/import/lint corrections
before the first natural behavior run, with evidence, not semantic repairs.
Natural behavior/static red or unsuccessful/unrelated mutant stops the round.

Build follows fresh green exact main, then independent source review and all
normal hosted gates/feature Windows<=660s. Preserve quarantine28/sample8 and
existing exhausted repair counters. M2open/M3unchecked; hardware paths remain
**unverified on real hardware**.
