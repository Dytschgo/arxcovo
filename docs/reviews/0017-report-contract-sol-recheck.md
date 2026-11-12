# ADR 0017 corrected-contract Sol recheck

Reviewer: OpenAI / gpt-6.1-sol / medium, coordinator, 2026-11-11. Exact incoming contract head `1d78d74d4feafefcee53024aafab9c2da4815c96`, compared with prior reviewed `86b0a40b` and findings at `f8a02af`. This is independent review of Opus architecture; no native implementation or milestone acceptance.

**PASS for local implementation**, with the consistency edits below applied. No blocking findings remain on either standards or originating-spec axes. Production remains MechanismUnavailable; normal independent implementation review and hosted gates remain mandatory.

## P1 resolved

One child-creation boundary now applies before fork/clone/spawn or CreateProcess, not target exec. Only NotInvoked or documented ProvenNone map to pre-creation failure; Created and Unknown quarantine with unconfirmed cleanup unless actual containment/cleanup evidence supports a post-spawn evidence report. Missing observation cannot downgrade a child to pre-spawn. The tests distinguish created-not-execed, unknown outcome, not-called and cancellation-before-call. The revised rule is conservative and explicitly amends ADR0009; no undocumented no-child guarantee is asserted.

## P2 resolved

The full observation computes the match result first. Each differing SID/UID/GID is withheld everywhere, including persistence, and matching fields come from the observation rather than the pin. Both immediate metadata copies and subsequent activity history read the same sanitized value. Debug redacts raw observation identifiers. Tests cover Windows and Unix separate mismatches, cleanup precedence, durable rows, byte inspection and MCP run/history/inventory paths. No separate forensic cache, lookup capability argument, approval flow, identity hash or schema addition remains.

## Small consistency corrections applied by the coordinator

These preserve the normative rules, not new architecture choices:

- Qualified the decision-summary quarantine sentence to failed runs; successful created runs still follow row N without quarantine.
- A failed Job assignment does not establish containment; token-read/resume failures may follow a successful assignment, so their cleanup classification depends on real evidence rather than an assertion that every child is outside the Job.
- The privacy test checklist now retains a matching UID or GID when only its partner differs. The prior blanket both-NULL assertion contradicted the accepted per-field rule and its own negative-case table.
- The database privacy check scans a remaining protected TRUNCATE journal if present. It no longer requires deleting that persistent sidecar, which conflicts with the service store design.

Implementation must be based on the reviewed N3 completion helper when available and preserve quarantine-before-stopping priority. Any new report API transition must keep real dispatch/durable-row and caller-disconnect/cancellation regressions. No native backend, owner integration or development behavior change is approved by this contract.

## Evidence and limits

Read the full corrected ADR/brief, disposition, prior Sol findings, existing executor/store/config/MCP paths, and relevant ADR0009/0007 requirements. Compared outcome, identifier and phase tables against all mandatory cases and mutation requirements. The prior primary-source Windows enum/group/accounting checks remain applicable; no new external API guarantee is introduced. No Rust tests/build or native operation ran for this documentation review. UTF-8/LF, local links and whitespace were checked. The small Sol-authored consistency edits above must be included in the later Opus implementation-review scope; no independent credit is claimed for them here.

No push, new PR, GitHub write, experiment, owner/admin, SCM/LSA/account/service/VM operation. Installed/native/dedicated M2 remains **unverified on real hardware**; M2 is incomplete.
