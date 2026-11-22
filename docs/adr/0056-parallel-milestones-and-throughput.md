# 0056: Parallel M4–M6 and throughput rules

Status: accepted. Owner instruction of 2026-11-23, relayed by the babysitter. This extends [ADR 0023](0023-parallel-m3-with-open-m2-hardware-gaps.md).

## Decision

The owner asked to "do everything possible to speed up this project", without fast mode, by parallelizing work and giving agents bigger chunks. Strange bugs that do not hold up a feature go to a later release.

- **Parallel milestones.** M4 (Windows host and test VMs), M5 (script vault and templates) and M6 (dashboard) proceed in parallel. ADR 0023 already permitted parallel work while blocked acceptance stays visible; this extends that to M4–M6.
  - M6 builds the host list, activity feed, terminal and login now. It builds the VM views and the console viewer against typed stand-in data until M4's VM APIs land.
  - M5 builds its non-VM parts now: versioned scripts per OS, the MCP tools, and the template tool inventory. Its acceptance test needs a throwaway VM, so it waits for M4.
  - A milestone is marked complete only when its own KICKOFF acceptance check passes. No acceptance is inferred from fixtures or from another milestone's progress.
- **Capacity.** At most three builders and two reviewers at a time, up from two and one. Every PR still needs an independent reviewer separate from its builder, plus green required checks. Only the coordinator merges.
- **Bigger slices.** Builders get whole coherent features or views per PR. PRs stay one-concern and reviewable. Coordinator PROGRESS updates are batched, with fewer docs-only checkpoint PRs and no extra evidence ceremony.
- **No fast mode.** Effort stays as routed: Sol 6.2 high for engineering, architecture, security and reviews; Claude Opus 5.5 high for UI and as the stuck fallback.
- **Bug triage by product impact.**
  - An intermittent or odd failure that does not block a v1 feature or its acceptance check, or that has only marginal user impact, is recorded in `docs/ROADMAP.md` under "Known issues / later release" with evidence, if it cannot be fixed quickly.
  - If such a failure reddens `main`, that single case is quarantined honestly through the existing route instead of a long diagnosis.
  - Correctness, security and data-loss bugs are still fixed before merge.

## Unchanged

- CI stays under 15 minutes, and `ci-time-budget` and `agent-review` are never bypassed.
- Scratch stays on D:.
- No access to the old Lab Manager or the `agent-windows-test` VM.
- No owner admin steps; the M4 host setup remains the owner's.
- No secrets.
- Honest **unverified on real hardware** labels.
- On a usage limit: commit WIP and schedule a resume.
- The stop rule after three counted failures.
