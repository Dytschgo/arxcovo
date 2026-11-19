# Path to the first owner-testable v1

Audit: 2026-11-19, refreshed at main `05ba5b87` after PR189–PR194 merged. This is
an implementation and evidence map, not a new acceptance definition. KICKOFF
milestones1–6 remain the target. [KICKOFF](KICKOFF.md) defines the complete requirements;
[PROGRESS](PROGRESS.md) preserves per-task evidence and original failures.

| Milestone | Current evidence | What still blocks owner acceptance |
| --- | --- | --- |
| 1: setup | Repository, workspace and required CI/review gates exist. Exact-main first CI passed on `95b4b651` (44616170358). | Keep gates green on every delivery; a passing source review cannot replace CI. |
| 2: single host | Authenticated local MCP, inventory, activity and Windows current-user command path are in merged source. Backend preview is prepared. | Actual Claude Code journey remains unrun; owner OAuth wait remains. Launcher observation, native F/G/PA6 and remaining closure-ledger items are incomplete. Dedicated installed execution has no acceptance. |
| 3: multiple hosts | Transport, RPC, enrollment/storage components have reviewed source and bounded tests. | Retained-owner handoff (PR189) and SQLite identity proof (PR190) are merged; bound store construction is not yet wired. Live startup/custody/admission and real-network three-container acceptance remain incomplete. |
| 4: Windows/Linux test VMs | Settled specification and owner setup checklist exist. | Not implemented/accepted; requires earlier milestone gates, owner-provisioned host limits and tools, and user-supplied installation media. Agents perform no admin setup. |
| 5: script vault | Settled specification exists. | Not implemented/accepted; requires real throwaway-VM validation and rejection of a broken script version. |
| 6: dashboard | React/Electron scaffold and smoke tests exist. | Host/VM/activity/terminal/console management views and their end-to-end acceptance/screenshots are not implemented. |

The preview is a backend slice. Opening Electron does not provide host or VM
management. A passing MCP mock or SQLite component test does not demonstrate the
required owner journey, native cleanup or cluster survival.

## Immediate route and limits

1. PR189 and PR190 merged after the shared Windows rename defect was fixed on
   main (PR191). The Windows founder deadline and e7 timeout causes remain
   unknown; test-only traces (PR192, PR193) and a scheduling mitigation (PR194)
   are on main. EngineSol3/3 is exhausted; historicalOpus0/2 is unchanged.
2. M2's already-prepared actual-Claude harness still awaits secure owner OAuth;
   do not read saved login state or send another credential request merely to
   refresh this document. [Windows account selection](windows-execution-accounts.md)
   distinguishes the current-user foreground preview from installed-service setup.
   Optional dedicated native acceptance requires the owner-created Standard
   account, enforced D: disk cap and CPU/RAM reserves in [host setup](dev-host-setup.md).
   Linux A7/A11 sudo/NNP and A12 remain dedicated-Linux-host pending.
3. Finish bound store construction, custody, founding, joining, admission and
   startup in the ordered slices of the current M3 plan before attempting the
   [three-container acceptance](briefs/m3-three-container-acceptance.md).
   This must use real network transport, not mem_net or direct three-voter
   initialization as acceptance. Container acceptance must remain labelled as
   such; real multi-host hardware stays unverified.
4. Complete milestones4–6 in order after the earlier acceptance gates. The
   [ADR0023 exception](adr/0023-parallel-m3-with-open-m2-hardware-gaps.md) permits
   independent M3 work beside open M2 gaps; it does not authorize skipping to M4
   or declaring v1 complete with a scaffold dashboard.

## Evidence inspected for this audit

- Main daemon and stdio dispatch: `crates/arxd/src/main.rs`; tool registration:
  `crates/arxd/src/mcp.rs`; both match the recorded main8bd12b7f Git objects.
- UI: `ui/src/App.tsx` is the scaffold view. Desktop start and development commands
  match `desktop/package.json` and root `package.json`.
- [PR188 main CI](evidence/m3-pr188-main-first-ci.md) records the last merged-main
  first-attempt evidence; this audit did not rerun CI or claim a fresh health read.
- [PR190 corrected first CI](evidence/m3-pr190-corrected-first-ci.md) records the
  failed required gates and independently assessed native unknowns.
- Owner package status and M2 gaps were checked against PROGRESS's current-state
  and closure-plan sections. No package, daemon, credentials or VM was opened.

Documentation inspection supplies no runtime acceptance. M2 OPEN / M3 UNCHECKED;
**unverified on real hardware**.
