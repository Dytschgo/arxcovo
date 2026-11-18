# Independent fresh membership observation source review

Sol6.1/high independent reviewer,2026-11-18. PASS.
Base6f4618c76256c2ead9dfb501c2c3946fde14a81b through
1d068d5f566006ca68ff948ed82d5f50bdc60d45.
Reviewed blobs consensus.rs11c515954f057dd7c31acbfa1940d9270b95c662 and
membership_observation.rse925a7e23db98f6502e5d8554c5f051f1e6b9851.

## Standards

Zero findings. Exactly2non-doc paths; removing one module declaration reproduces
base consensus.rs byte-for-byte, preserving fresh_read/Owners/start/shutdown and
observers. Seven synchronous tests with no ignore/platformhiding/unsafe. No
speculative abstraction or documented-standard breach identified.

## Spec

Zero findings. Existing canonical encoding bounds native membership before clone,
watch borrows end before await. The existing fresh_read closure reads registry,
watermark and membership under one store guard. Effective/committed IDs AND full
contents must match applied membership; joint incl duplicate-set configuration
refuses. Applied watermark, committed local vote/currentleader, established read
leader and unchanged second sample required. Actual entry accepts AuthenticatedPeer
only; synthetic identity refuses immediately via actualAPI with no engine/runtime/
store/readcapacity. Result owns only bounded membership/positions, no peerkey or
resourcehandles. Guarded watermark, not atomic snapshot or lasting authorization.
Tests cover actual Found/Remove, exact result, binding/content/watermark/leader
refusals, bounds, unrelated progress and immediate synthetic entry refusal.

## Security

Zero findings. No new endpoint/mutation/ownerauthority/forwarding/sensitiveoutput.
Staticerrors, retained tuple freshly authorized on the same applied image. Existing
trusted TLS config assumption retained. Actual adapter quorum/cancel/race proof
remains required before admission relies on this API; pure tests do not prove it.
Hardware **unverified on real hardware**.

## Evidence and final-head applicability

Reviewer read final7/7 output and Clippy7.63s, verified12recorded artifacthashes,
validated CI history603wall/600charged/3queuecredit. No reviewer Rust/engine/hosted
execution. Standards0/Spec0/Security0; no blockers/advisories.
Source verdict remains applicable after planned semanticcontrols only if coordinator
verifies exact source restoration and ALL non-doc tree matches1d068d5f. Accurate
review/evidence/progress docs may follow without source rereview. Future mutation
outcomes are not part of this audit; attribute separately. Allfive gates,
freshgreenmain/Windows<=660s required for merge. Sol3/3 exhausted/Opus0/2 unchanged.
