# ADR0017 unverified-identity output boundary probe

Author: OpenAI / gpt-6.1-sol / medium, coordinator, 2026-11-11.
Pinned implementation: `c394ec5c27268a1ebffa68db82e7314d6b18f009`.
Scope: synthetic backend report assessment through real dispatch and an owned temporary activity store. No native backend or private owner data was used.

## Reproduced behavior

The current Completed arm assesses/sanitizes metadata, then always passes both CapturedOutput streams through output_value. A failed identity assessment does not gate those values.

The adjacent temporary patch has one diagnostic test with two cases. Both use a Windows Standard pin and normal exit/cleanup/boundary facts. Case 1 observes the contract's distinct synthetic SID B; case 2 supplies no observation or point. In both, stdout head and stderr tail contain that same synthetic marker.

Actual command: `cargo nextest run -p arxd --locked diagnostic_unverified_identity_output_is_returned --status-level fail --final-status-level fail`. Run `b977ed9e-612e-4aff-b679-22f979ce4c16`: 1 diagnostic passed (two cases), 228 filtered/skipped. It asserts the observed behavior, not approved policy:

- outcome is failed and the executor quarantines;
- metadata.observed_id is NULL;
- stdout.head and stderr.tail still equal SID B.

The test source was restored byte-for-byte. The patch is retained for reproduction, not adoption as a regression preserving disclosure. No full suite, Clippy or native claim is added by this diagnostic.

## Contract question before native integration

ADR0017 requires protected identifiers to be absent from immediate responses. Its privacy tests recursively inspect the whole value, but their completed() helper supplies empty output. The reproduced output route is therefore outside those tests. An identifier could escape without appearing in any metadata field.

More broadly, when a child identity is missing or mismatches the configured pin, raw child output is not established as output from the authorized build identity. Returning it may expose more than an identifier. This is a latent interface/contract gap; Native still refuses MechanismUnavailable, so no production owner-data disclosure is claimed.

Request an Opus architecture clarification of the safe output-release condition. Preserve ordinary failed-command, timeout and cancellation output when its identity is valid; decide explicitly what to return for mismatched or inadmissible identity and missing/wrong containment. Use a trust gate, not regex/SID string scrubbing or a blanket filter over legitimate command output. Keep response keys/schema3 and truthful reason/cleanup precedence. Captured payload should never become audit storage or a forensic cache.

Required regression cases should exercise both streams with nonempty synthetic payloads through real dispatch and the MCP envelope, including identity mismatch, missing/inadmissible observation and matching positive controls. Existing metadata-only privacy tests remain necessary. This report does not choose a new policy or alter the accepted contract.

## Limits

The only identifier used is the existing synthetic SID B from ADR0017. No actual process, service, pipe, privileged constructor, SCM/LSA/restore/account/admin/VM operation, publication or workflow dispatch. All source restored; no runtime change is committed. Installed/native/dedicated M2 remains **unverified on real hardware** and incomplete.
