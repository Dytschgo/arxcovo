# 0021: Narrow omission of unsupported Ubuntu mirror-list entries

Status: proposed; explicit owner A5-scope and PR54 bundling decision pending.
Author: OpenAI / gpt-6.1-sol / medium, coordinator, 2026-11-12.

The [proposal](../reviews/ubuntu26-mirror-compatibility-proposal.md) defines
the exact input grammar, bounds, whole-plan validation, tests, mutations and
prospective path list. Independent Sol review accepted drop-only handling in
principle but required grammar, coverage and delivery corrections. The final
text corrections require independent review. No implementation is accepted.

For the indirect mirror list only, omit (never preserve, resolve or contact)
an unsupported Ubuntu-domain entry that matches the complete strict raw-token
grammar. Keep the fixed HOSTS allowlist and require at least one allowed
Ubuntu fallback. Count retained and omitted entries toward the existing cap.
Reject every other unsupported entry, including an invalid tail after an
omission. Preserve signatures, keyrings, Valid-Until, URL/source trust rules,
Kernel endpoint, priority ties and existing publication-failure semantics.
Keep the bytes-returning parser API; carry and print only a bounded omission
count on success. Refusal emits no success/count/host data.

This deliberately changes refusal into omission for that narrow class; it is
an availability tradeoff with no expanded contact trust. Exact historical
hostname and unchecked tail are unknown. Representative fixtures are not a
historical replay or proof of native behavior.

The existing frozen A5 excludes the required helper/test/contract paths.
Before implementation or hosted execution, the owner must prospectively
approve the exact additional paths in the proposal and separately permit
carrying this prerequisite in existing PR54 despite the one-concern rule.
No numerical/performance/coverage criterion or attempt count changes. If
declined, delivery stays blocked; no false speed label or gate override.
The source implementation then needs independent Opus high review and the
final head's own first-attempt measurement. No new product stack is started.

No source, system, workflow, policy, account, service, VM or experiment is
changed by this proposed ADR. M2 is incomplete. Native, installed and
dedicated paths remain **unverified on real hardware**. ExecutionFailed and
E_PROTECTION remain separate unresolved issues without retry.

## Owner disposition - 2026-11-12

The owner explicitly approved both corrected limited exceptions: the exact
A5 path expansion in the proposal and carrying the compatibility prerequisite
as a separate commit in existing PR54. This supersedes the historical pending
scope/bundling status above. Coordinator accepts the corrected technical
contract, including the explicit non-ASCII shape-fence case and paired-mutant
requirements. Independent source review is still required before push. All
speed thresholds, platform/test coverage and failed-attempt counts remain
unchanged. Performance attempt 3 is unused; no optional manual precheck is
planned. No additional native, owner, experiment or activation authority.
