# Joint recovery proof: root review

Root Sol 6.1 medium, 2026-11-17. Reviewed Sol high brief fb4ddb58 and
cached OpenRaft call_core/management/public membership implementation.
Separate-author review under the owner's Sol routing; no cross-provider claim.

## Standards

The proposal is bounded and test-only. Root corrected shutdown ordering:
remove router-held engine handles before shutdown. Existing L1 uses this order
to release stores; leaving them in the router is not an acceptable cleanup test.

## Spec and security

The parked-future seam is supported by source: call_core enqueues independently
and the same future submits flatten only after receiving the joint response.
The observed effective joint membership is necessary evidence that the single
poll actually enqueued. Pending alone does not prove it. No constructed-joint
fallback is accepted. Actual runtime behavior remains unproven.

Root accepts the proposed 75-second entry ceiling for this new test alone,
with operation waits capped by existing five-second windows. This changes no
production timeout, existing test wait, retry, scheduling parameter or budget.
Classify the new three-node multi-voter proof under the owner's whole-fixture
Windows quarantine: 28 guarded fixture cases, original eight sampled, required
Ubuntu/macOS coverage. No live Windows engine execution or repair credit.

One blocker remains before implementation: the brief requires runtime mutation
evidence but provides no supported execution route. Local Linux is unavailable,
Windows execution is excluded, and extra mutant pushes/workflow changes are not
authorized. A test plan cannot require evidence for which it has no route.
Settle a bounded normal-CI counterfactual design that exercises equivalent
omissions and the same assertions, or explicitly leave implementation blocked.
Do not silently waive mutation evidence or treat static tokens as equivalent.

Actual work: static source/brief review only, no build/test/probe/network.
Production admission, custody, transport, owner authorization and expiry policy
remain open. M2 open, M3 unchecked; hardware **unverified on real hardware**.
