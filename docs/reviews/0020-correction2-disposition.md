# ADR 0020 correction 2 disposition

Author: OpenAI / gpt-6.1-sol / medium, coordinator, 2026-11-12.
Proposal remains proposed. This is correction attempt 2 of 3, not acceptance.

Input: [independent correction-1 review](0020-correction1-sol-recheck.md),
original report commit `6bdfa141bc94c9cb84c64d8e9dd15292c7810b45`.
The original Opus design keeps its independent Sol review credit; the new
Sol-authored delta requires an independent Opus high check.

- P2: scope the bound to one immutable shared admission domain. One exclusive
  service subtree has exactly one such domain until restart. Runner, slot
  accounting, readiness and ticket allocator are bound once with no reset,
  rebind, replacement or second-runner attachment. Shared handles preserve
  the domain. T20 adds shared-latch/handle-recreation behavior and construction
  visibility/source pins. R3's fresh runners belong to distinct test subtrees.
- P6: permit only private flag plumbing from run_core through unix::run to
  Group::create; include unix.rs in E5. Preserve order, fences, cancellation,
  errors/results and original errno capture before cleanup.
- P7: W3e holds the worker before its final shutdown/admission check for the
  zero-call assertion. An already-admitted case instead checks honest cancel
  and drain behavior without claiming zero calls.

Checks: docs-only diff reviewed; git diff --check and strict UTF-8/LF passed.
No build, behavior test, mutation, native operation or GitHub action ran.
Future test requirements above are not execution evidence. CI investigations
remain parked. Stage C/D merge and contract acceptance remain prerequisites
for implementation. No activation, owner/admin/service/cgroup/VM or experiment
operation occurred. Native, installed and dedicated paths remain
**unverified on real hardware**. M2 is incomplete; ExecutionFailed and
E_PROTECTION remain unresolved without waiver or retry.
