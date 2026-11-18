# M3 authenticated identity delta recheck

Publication remains on hold. The two original reproductions are closed. One
related parent-binding check still accepts an inactive required attribute.

Independent single-agent review by Astra 6.2, high, on 2026-11-18. Reviewed
head e66d298e0290aebe94a027e90fd32c9fb9803cf1 against previously reviewed
092dcf9c729e3e34111fa33dd8bc753b3b98c944. Main comparison base was
e3e79089e6c36f6483cb3db8842e5894f26ee00d. The delta contains fd67bbda
(retained review, identical file bytes to b989fb90) and e66d298e (guard,
evidence and progress).

## Standards

The correction is confined to the new identity guard and its tests. Removing
the identity inventory additions from the reviewed Python blob reproduces
the main blob byte for byte. The pre-identity prefix also matches 092dcf9c.
All 40 original top-level AST nodes match in order, including all 16 function
and class definitions. The delta whitespace check passed.

The prior current-work advisory is closed. PROGRESS now identifies the
integrated main and distinguishes the pending D-volume work from the
compile-only failure on C. The evidence discloses the six transient unpack
errors and the coordinator's subsequent 9/9 run in 0.124 s. I independently
ran the pure Python guard once at the reviewed head. All nine passed in
0.958 s. The earlier failure count and timing come from the coordinator.

## Spec

- **B1 closed for its reported cases.** In-memory probes around the entire
  final test and each complete parent binding were rejected. The guard
  rejects either block-comment delimiter anywhere in the three inputs. This
  also rejects such text in strings and line comments. That restriction is
  documented, reversible, and compatible with all three current files.
  Line-commenting the test attribute of each required function also failed.
- **B2 closed.** Each single-signature change from plain fn to async fn was
  rejected independently. The declaration matcher anchors the attributes at
  line start and explicitly refuses the captured async prefix.
- **B3 open, parent binding.** The brief requires the guard to "pin both
  parent bindings" (docs/briefs/m3-self-admission-kernel.md, lines 170-172).
  In each parent independently I changed only the leading `#[cfg(test)]` of
  the required binding to `// #[cfg(test)]`. Both modified inputs passed
  assert_identity_inventory. The literal count and trailing attribute search
  start inside the line comment. In pki.rs this removes the test-only gate on
  mod tls_tests. In tls_tests.rs the outer parent still gates the child, but
  the child's required binding has changed. The committed Rust declarations
  are correct. Match the first attribute at the start of an active declaration
  line in the existing canonical layout and add a negative case per parent.
  These two cases can be covered with the existing text guard.

## Security

The three production/test blobs match both 092dcf9c and implementation
a75962718aa6290681a2392c1ec825307af10bf0 by Git object identity.

| PKI file | Git blob |
| --- | --- |
| verify.rs | 7f60471a863eb8423316e919c83bc254903c12f9 |
| tls_tests.rs | 8bed50125797725ed4bafc40b3836334ae0353dc |
| authenticated_identity_tests.rs | 8bfa9a416bb757dce4e43d68b6f60532ede563d1 |

The prior production review carries forward. B3 concerns the inventory proof.
This delta introduces no change to TLS verification or identity authority.

## Release disposition

Close B3 and independently recheck the correction before publication. The
separate Sol D-drive TLS baseline and two designated mutation results remain
pending at this reviewed head. This recheck exercised only Python and local
source comparisons. Publication also awaits the runtime evidence. The release
still requires independent exact-head review, all five hosted gates, feature
Windows <=660 s and fresh green main. Only the coordinator merges.

M2 remains open and M3 unchecked. Sol 3/3 and historical Opus 0/2 are
unchanged. Hardware paths are **unverified on real hardware**.

Standards findings 0. Spec findings 1 (B3). Security findings 0.
