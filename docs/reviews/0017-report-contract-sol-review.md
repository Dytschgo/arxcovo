# ADR 0017 independent Sol review

Reviewer: OpenAI / gpt-6.1-sol / medium, coordinator, 2026-11-11. This is independent review of the Opus-written proposed contract, not an implementation review. Base `4abf276a15883e8e55ff53df26a0641c62b43efd`; exact reviewed head `86b0a40bba154a5cb9ed408bd9e0c2e433b73479`. No native backend exists and no production leak is claimed.

Verdict: **changes required before implementation**. The typed evidence, immutable pin, precedence and containment checks address the five reproduced reports coherently. Two contract issues need architecture correction; neither warrants a new approval flow or probe.

## Standards

No blocking standards finding. The API and outcome matrix are concrete, bounded and compatible with schema 3. The brief has real-dispatch and durable-row checks, positive controls, stop causes and source mutations. The native-fact versus interface-consistency limitation is explicit. Preserving the separate N3 completion helper will keep the eventual merge small.

The same-provider architecture credit is not claimed: Opus wrote this ADR and Sol is reviewing it. Original ADR0009 credit remains with its retained earlier review.

## Spec P1: process-creation boundary is contradictory (medium, fix before build)

Decision 3 and the Report contract say pre-spawn variants assert that no process object was created. The normative parenthesis then narrows Unix to a forked child **that reached exec**. Those are different boundaries: a child can exist after fork while ID dropping, containment joining or the identity channel fails before exec. The proposed text permits two incompatible classifications of that case.

Trace: Unix child exists; pre-exec identity read/protocol fails; no target exec is confirmed. Reading the parenthesis literally lets the adapter return `Failed { after_spawn: false }`, row F, without a containment/cleanup claim or quarantine. Reading decision 3 gives row G, cleanup_unconfirmed and quarantine. This is an ambiguity in the proposed contract, not evidence that the existing Unix library leaks a child.

Correction required: define the lifecycle boundary once (Windows creation and Unix fork/child creation, not target exec), including an unknown creation outcome. State exactly when a native spawn error can prove that no child exists, versus when it requires post-spawn cleanup/evidence. Do not let a missing observation downgrade a potentially-created child to a pre-spawn report. Add a required negative test for a created-but-not-execed child with no observation and no confirmed cleanup; no native experiment is needed for the pure phase contract.

## Spec P2: mismatched-principal response disclosure conflicts with the privacy fence (high, fix before build)

The privacy paragraph explicitly allows a non-build principal's SID/UID to be stored and returned under row I. It argues that the build account can already resolve those identifiers, so this adds no capability. Capability to look something up is not authorization to disclose private owner data through MCP. AGENTS/KICKOFF prohibit private owner data in MCP responses; ADR0009's response privacy text permits only the observed build identity, while its mismatch table says observed fields as observed. The proposed ADR needs to resolve that existing inconsistency explicitly, rather than treating it as no deviation.

Contract counterexample (synthetic, not a native probe): pin account A; backend observes a syntactically valid account B SID, where B could be the owner or another local principal. Row I persists B and the response emits both `observed_identity.id = B` and `metadata.observed_id = B`. Activity history can emit the same ID later. There is no evidence that B is public or approved for disclosure. Passing the syntax validator does not settle that privacy question.

Correction required: make unknown/mismatched principal identifiers safe for both immediate MCP and later activity responses, including the nested metadata copy. Preserve a truthful identity-mismatch outcome without substituting the pin as an observation. Explicitly decide what local audit storage may retain, which interfaces can read it, and whether existing ADR0009 wording must be marked as amended. Do not add product approvals, hashes as a new identity vocabulary, schema migration, or native lookup. Add synthetic tests using distinct pin/observed identifiers that prove the protected identifier cannot appear in any unauthorized response path. If the design can prove these identifiers are allowed under the actual privacy fence, state the exact authority rather than assuming that lookup ability grants disclosure permission.

## Checks actually performed

- Read the full ADR and implementation brief at the pinned Git objects, the five-case probe, current executor/Metadata/store and service-config source, relevant Application/MCP test callers, ADR0009's identity/cleanup and response-privacy sections, and KICKOFF routing/fences.
- Inspected the existing Unix pre_exec sequence: it performs containment and identity work before exec. No Unix function was run.
- Checked primary Microsoft documentation for [elevation types](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ne-winnt-token_elevation_type), [elevation Boolean](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-token_elevation), [deny-only group attributes](https://learn.microsoft.com/en-us/windows/win32/secauthz/sid-attributes-in-an-access-token), and [Job accounting](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_basic_accounting_information). These support the stated Windows enum/group and ActiveProcesses caveats. They do not prove the proposed adapter's future behavior.
- No Rust build, test suite, native runtime, manager or identity API call. The findings above are contract traces, not claimed runtime reproductions. Report encoding/whitespace checked locally.

## Limits and next step

Opus architecture correction of P1/P2, then a fresh Sol recheck before implementation. N3, E1 and development behavior are outside this review. No source/workflow/PROGRESS edit, push, PR, status, merge, experiment or owner/admin/SCM/LSA/account/VM operation. Installed/native/dedicated M2 remains **unverified on real hardware**.
