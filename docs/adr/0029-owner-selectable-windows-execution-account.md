# 0029: Owner-selectable Windows execution account

Status: owner product decision accepted, 2026-11-18; implementation and independent
source review remain pending.

The owner explicitly requested that Windows execution default to the user's own
Windows account for simplicity, with a dedicated account available as a choice.
This supersedes the earlier Windows dedicated-by-default requirement. It does
not waive VM isolation, privileged setup, native qualification, data/resource
fences, or the held PR182's exhausted-attempt rules.

## Decision

Expose an execution-account setting with `current_user` as the default when a
Windows execution configuration is supplied, and explicit `dedicated_account`
selection. Workspace, environment and resource limits still require a valid
configuration. Existing inventory-only configurations remain inventory-only.

Current-user execution uses the identity of the user-level Arxcovo process. It
does not find or impersonate an arbitrary active desktop session. Reuse the
existing Windows Job executor with actual parent/child identity checks, bounded
capture and cleanup. Service identities, elevation, non-Medium tokens and enabled
administrator tokens remain refused. In particular, LocalSystem must never become
the effective identity of this default mode.

Current-user execution shares the launching account's file and credential access;
capability and activity must say that it is not dedicated-account isolation. The
new normal mode has no development acknowledgement switch. Historical explicit
`development_execution` configurations and activity keep their original meaning;
conflicting old/new configuration is rejected.

Dedicated selection must be explicit and never fall back to current-user execution.
The protected installed-service route and its actual executor qualifications stay
separate. While that backend is unavailable, the setting reports that fact rather
than claiming a successful dedicated run. It collects no account password. The
stopped S4U work, Native refusal, source pins and native acceptance gates remain
unchanged for this optional route.

The authenticated local transport is the execution boundary. HTTP execution stays
disabled. The account mode is immutable startup configuration, not a tool-call
parameter. Current-user metadata cannot claim dedicated rights or persist the
owner's raw identity merely to rename the mode. A schema change must preserve
existing activity and identity provenance.

This decision changes the Windows host execution default only. Other OS account
policies and optional Windows Admin support remain in the full v1 scope. The
dedicated account/caps/reserves requirements for privileged native or real VM
qualification are not removed by a user-level current-account preview.

## Verification and delivery

Meaningful checks cover default and explicit modes, invalid/conflicting settings,
dedicated unavailability without fallback, service/elevated refusal before spawn,
immutable local identity, HTTP refusal, private-state/workspace separation,
truthful activity/capability and historical database preservation. Reuse the
existing execution mechanism and budgets; a drain timeout must not be presented
as observed cleanup or a clean foreground exit.

After independent source review, a bounded real user-level MCP journey can verify
inventory, `cargo --version`, correlated durable activity and observed cleanup in
a task-owned D: workspace under a permitted non-elevated token. Test fixtures do
not substitute for that production path. No owner admin, account creation, S4U,
Lab Manager, VM or private-data operation is authorized by this decision.

M2 remains OPEN and M3 UNCHECKED until their revised actual acceptance requirements
are proved. This setting is a step toward the first owner-testable version, not
completion of v1 milestones 1–6.
