# Windows SCM contract review

Reviewer: OpenAI / gpt-6.1-sol / medium, single agent. Architecture author:
Anthropic / claude-opus-5-5 / requested high. Reviewed author range
`d6df821dcd2ec827bcf2cd3c86bea32a2c639ca4` to
`7e4cf06a5f44eb7d682e146d20238cac76383f62`, both changed documents in full.
This is a cross-provider architecture review, not an implementation review.

## Standards

No blocking finding. Scope is SCM library design and its implementation brief;
the owner provenance engine, native store and LSA rules are unchanged. The ADR
header was clarified to distinguish existing engine/store implementations from
the new unimplemented SCM amendment.

## Spec

No blocker after these local clarifications:

- Install stays demand-start. Start selects delayed auto before activation;
  stop changes back to demand only after stopped. Exact image/account/type
  matching and demand/auto classification preserve foreign-service refusal.
- Polling deadlines do not claim cancellation of synchronous native calls.
- A successfully accepted stop control is followed by polling, without repeat
  controls. Fake tests must distinguish this from a rejected control retry.
- Failure-injection tests follow the explicit accepted-error table instead of
  contradicting its absent/marked-for-delete success cases.
- Fresh install checks identity/binary before provisioning; it does not call
  the full store session constructor before its own directories/lock exist.
- The opaque deregistration proof has no Clone/Copy/Default. Consuming it before
  right removal and narrowing raw session parts remain CLI-wiring obligations.

All native assumptions and disposable evidence requirements stay open. No
service was queried, installed, started, stopped, deleted or reconfigured.
No LSA, privilege, account, elevation or ignored-fixture operation ran.

## Verification

Reviewed the source contract, registration structure and accepted ADR0014 boot
rule. Checked Microsoft's [OpenServiceW](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-openservicew),
[DeleteService](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-deleteservice)
and [ControlService](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nf-winsvc-controlservice)
documentation. Deletion remains pending while handles remain open; the proof
means deletion requested plus a stopped observation, rather than physical
database-entry disappearance. No test or hardware result is inferred from docs.

UTF-8, relative links and whitespace checks were run. No application suite was
run for these docs-only edits. Every installed SCM path remains
**unverified on real hardware**; no M2 acceptance is established.
