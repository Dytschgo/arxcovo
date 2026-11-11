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

## Disposition

Author: Anthropic / claude-opus-5-5 / high (architecture), 2026-11-11.
Base: 33f753a0fd6e55c992ebca1e6327cfd1406dfbb1. Documentation only. No
implementation, manager query, privileged operation or runtime test ran.

P2, accepted and corrected in ADR 0014. Decision 7 is new, and decisions 1, 3
and 6, the plans, the matrix and the tests are corrected:

- Install is a converging operation for a registration that is not active. A
  read-only `Expect(Installable)` runs before any mutation. It refuses Foreign
  or non-identical artifacts (`Foreign`), Unknown (`Manager`) and an Active job
  (new `Active`, `service-active`). An Active job is Linux `active`,
  `reloading`, `activating` or `deactivating`, or any loaded macOS job.
- Install then asserts boot intent off. Linux runs `daemon-reload` and then
  `disable` after publication. macOS runs `disable[Always]` before the plist.
  A final `Expect(Unloaded)` proves the postcondition. Install never records
  `enable`, `start`, `stop`, `bootstrap` or `bootout`.
- The Linux drop-in is published before the unit, and only while the unit is
  absent (`UnitAbsent`). `configure` keeps owning it afterwards.
- Interaction with start and stop: the last successful explicit command wins.
  Install never overrides an active service. It turns boot off only for a
  stopped-but-enabled registration.
- `ProvisionState` keeps nonempty state (Retained) after the existing
  rollback-header check instead of refusing it, so `uninstall` then `install`
  keeps history. Fresh and empty-partial state is provisioned as before, and
  unsafe state still refuses.
- The matrix and tests now cover running, stopped-but-enabled, stale enable
  with no artifact, a loaded job without its artifact, each partial
  publication point, retained, empty-partial and unsafe state, failures before
  and after the persistence and runtime steps of install, start and stop, and
  the model-only Linux stale-link window.

P3, accepted with no change of status. macOS Absent and state strings remain
Unknown until captured evidence pins them. Every macOS install halts with
`Manager` until then. This disposition does not clear the PR29 unloaded-job
blocker, does not authorize CLI activation and claims no M2 acceptance.

All systemd, launchd, protected-layout and installed-service behaviour remains
**unverified on real hardware**. A fresh independent Sol recheck of the
corrected four documents is required before this contract is treated as
settled for implementation.
