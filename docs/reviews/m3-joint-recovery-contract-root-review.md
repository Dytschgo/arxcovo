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

## Recheck of 0e1febcd

Root accepts the revised seven-scenario plan for this test-only API proof.
The scenario-independent checker consumes real results and saved expectations;
each counterfactual must both succeed in the intended engine operation and
trigger exactly its designated checker violation. Setup, convergence and cleanup
failures cannot count as successful negative controls. This closes the supported
runtime evidence-route blocker through ordinary Ubuntu/macOS CI.

These are omission-equivalent runtime controls, not compiled source mutations.
The report and future PR must preserve that distinction. Each of the seven
fixtures runs once, sequentially, under the one shared 75-second entry ceiling,
including a reserved cleanup window. No Windows engine execution is granted.
Root accepts this expanded work inside the existing ceiling, not seven renewed
budgets. Actual runtime fit and headroom remain unknown until the first normal
hosted run; a natural red stops work without rerun or tuning.

Keep the parked Box owned until the scenario's decision point. Positive and the
five request-substitution scenarios drop it before recovery; ResumeFuture alone
resumes it. No constructed joint entry, fabricated result or expected panic.

The bounded implementation allowlist is accepted, conditional on fresh green
exact main and independent source review before publication. This authorizes
only the new test, cfg(test) binding, guard and stated documentation changes.
Production L2 remains unauthorized. Actual recheck: read revised brief only;
no build, test, runtime probe or network action ran.
