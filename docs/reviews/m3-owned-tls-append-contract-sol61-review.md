# Owned TLS Append contract review

Independent Sol6.1/high, 2026-11-18. GO for the bounded source implementation;
zero remaining Standards, Spec or Security findings. Final proposed documents
were subsequently committed as `a5162008`; coordinator changed only their status
and review links when accepting the contract.

| Reviewed document | SHA256 before status-only acceptance |
| --- | --- |
| docs/briefs/m3-owned-tls-append.md | e6ecffb35f36a99117afec73744b055bf729bb19994282c9cfc99f18d8dbdcc3 |
| docs/adr/0028-owned-tls-append-transport.md | 7eb866e8a645b5eb075d99c7d43ddfe0b693782c5970d8a2010560c758473912 |

Standards PASS: the allowlist isolates Append transport, dependency and tests.
Engine/timer/capacity/quarantine edits, synthetic PreVote grants and factory or
integration overclaims are excluded. D: verification, independent source review,
normal gates and exhausted counters remain intact.

Spec PASS: actual interfaces support the construction. The new 32-task and
four-handshake pools, absolute deadlines, framing and half-close protocol are
implementable at source level. Exact full acknowledgment is correct for this
no-prefix subset: pinned OpenRaft network/v2/network.rs:120–123 distinguishes
whole-request acknowledgment from prefix splitting, and engine_impl.rs:689–704
returns segment.last(). It is not a universal NetAppend restriction.

Index zero, consecutive indices, full LogId order and committed Append votes are
explicit. Review found the uncommitted-vote gap before code; the accepted contract
now requires refusal and a real-socket zero-dispatch test. Cancelled close retains
persistent ownership/original deadline; successful joins sever store ownership
despite retained clients. Corresponding lifecycle tests were added to the brief.
Deferred startup/factory/node-shutdown integration is accurately described.

Security PASS: actual certificate cluster/node/key binding, internal reviewed
configs, final applied-registry reauthorization, static errors and numeric
endpoint fences remain. Synthetic certificates exercise real verification while
production issuance stays unchanged.

No Append-path assertion requiring entry leader IDs at most the request vote's
leader was found. The snapshot assertion is outside this slice; no extra policy
was invented from it.

Limits: static review only. No build, dependency resolution, tests, controls or
runtime work. Half-close, feature unification and ownership require implementation
evidence. No engine, container or real-hardware acceptance granted.
