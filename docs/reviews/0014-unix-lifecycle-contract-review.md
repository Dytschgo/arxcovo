# ADR 0014 independent contract review

Reviewer: OpenAI / gpt-6.1-sol / medium, 2026-11-11.
Pinned base: a8c98347af956e93f0ec08d121e590254b7f8616.
Pinned head: e53e4071a7d6ad6ef6533b9b77c3c110e91c558b.
Scope: all four changed documentation files, read from pinned Git objects.
No implementation, manager query, privileged operation or runtime test ran.

## Standards

No blocking repository-standard findings. Amendment pointers, evidence limits
and bounded failure rules are explicit. Whitespace checks passed. The ADR is
longer than the requested short correction because it includes the build brief
and transition matrix; this is advisory.

## Spec

P2, changes required: install/reinstall boot semantics are contradictory.
Decision 1 and the guide say install leaves the service stopped and off at boot.
The Linux install plan never disables an existing enabled unit. Both install
plans accept an already loaded Arxcovo registration; Linux never stops it, and
macOS skips disable when loaded. Therefore reinstall can leave the service
running and enabled. This also matters after a partial owner operation.
Specify reinstall behavior and exact vectors, then add running/loaded,
stopped-but-enabled and partial-install transitions to the model matrix.
Do not infer verified native manager outcomes from that model.

P3, advisory: macOS absence/query fixtures are intentionally still unpinned.
Implementation must retain Unknown until captured evidence supports absence and
state mappings. A document pass alone cannot clear PR29's functional unloaded-job
blocker or authorize CLI activation.

Validation: complete four-file diff inspected; git diff --check passed. No Rust
suite was appropriate for this docs-only assessment. Native systemd, launchd,
protected-layout and installed-service paths remain unverified on real hardware.
Standards: zero blocking findings. Spec: one P2 and one advisory.
