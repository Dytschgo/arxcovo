# Actual CA sentinel no-sign oracle

Candidate99f892e8223585bc1f7a2649d94a2f6c2fdecc71, coordinator Sol6.1/high,
2026-11-19. Only pki/ca.rs changes,87 additions, all cfg(test). Production
statements/APIs, founder/Engine, subject seals, dependencies and CI are unchanged.
Independent exact-source review and required hosted checks passed; PR188 merged
as8bd12b7f. [Merge evidence](m3-pr188-first-ci.md), [main evidence](m3-pr188-main-first-ci.md).

The dormant bridge slice remains source NO-GO. This test-only prerequisite
addresses its missing CA oracle; it does not implement speculative lifecycle or
claim operational retained-owner/custody/publication endpoints. Independent
Sol6.1/high contract review GO, Standards0/Spec0, at
`D:/Temp/arx-founder-ca-oracle-review-v1`. Contract SHA256
`78c651fc8e7383499355df5100c945616f13ad0d97fd66e51e4c6bd74d055937`.

Both tests call actual ClusterCa::sign_leaf with a matching existing sealed
IssueTicket/ProvenCsr fixture already used by PKI tests. It constructs in-memory
enrollment, no Engine. No new subject/factory or committed Found/guard provenance.

| Exact suffix under cluster::pki::ca::tests | First result / body / command wall |
| --- | --- |
| sentinel_finite_ca_refuses_before_signing | 1PASS / 0.00s / 26.276s including25.80s compile |
| sentinel_conversion_refuses_before_signing | 1PASS / 0.00s / 0.442s |

Finite CA: MAX refuses CaExpiring at conversion/signs0/0; healthy NOW produces
fully consumed parsed DER at1/1; repeatMAX adds neither. Conversion fixture:
only the private local test CA horizon becomes i64::MAX; its real issuer/cert
remain finite, so it is deliberately inconsistent, not production validity.
leaf_fits(MAX) must be true; independent checked constants before/after are
unrepresentable. Actual MAX returns Sign at1/0, healthy control signs at2/1,
repeatMAX refuses at3/1. No horizon helper is exported.

The test marker precedes real conversion after serial RNG; an earlier RNG error
cannot satisfy the oracle. Existing signs increments just before signed_by;
zero proves that boundary was not reached. MAX fails the first conversion;
independent after-range assertion and healthy path support the second's range,
not an executed second conversion at MAX. No site4/pre-initialize/erasure claim.

Static quarantine/seal guard PASS9/9,0.140s body/0.332s wall; final fmt PASS;
locked/offline all-target Clippy PASS5.573s. Build-only receives no behavior credit.
Initial fmt failed two line wraps before tests; retained, then corrected only
ca.rs. No semantic test failure, rerun/mutant or engine fix attempt.

Raw results/exit records/source hash/diff and effective D environment are retained
at `D:/Temp/arx-ca-sentinel-build-v1`. No local Engine/founder/native/daemon/client/
network/container execution, admin/owner-data/LabManager/state/VM/cache/OAuth/
babysitter action. EngineSol3/3EXHAUSTED/historyOpus0/2 unchanged. Unreviewed owner
handoff V2 at `D:/Temp/arx-m3-owner-handoff-design-v2` now has independent offline
Design GO while frozen Engine/store-binding gaps remain; no runtime scope. Qualified custody,
actual Found-origin publication and M3 acceptance remain open.
M2 OPEN/M3 UNCHECKED; **unverified on real hardware**.
