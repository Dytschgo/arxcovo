# Arxcovo agent instructions

Build the settled design in docs/KICKOFF.md. Complete v1 milestones 1–6 in order; post-v1 features belong only in docs/ROADMAP.md.

- Keep docs/PROGRESS.md current with milestone tasks, acceptance evidence, last merged PR, blockers and per-task provider/effort/outcome. Record concrete architectural deviations in docs/adr/.
- Use small conventional commits and one-concern PRs. Every behavior needs a meaningful test. Never claim a check that was not run; label hardware paths not exercised as **unverified on real hardware**.
- Sol implements; Opus 5.5 high handles architecture/security/UX and reviews Sol changes. Sol reviews Opus changes. No PR merges without a different-provider independent review and green required checks. Reviewer fallback on provider limits: grok-4.7 high. At most two builders and one reviewer.
- After three failed fixes, hand off to the other provider with failure evidence. Respect the routing/usage-limit rules in the kickoff.
- Never touch the old Lab Manager installation, state, or Hyper-V VM `agent-windows-test`. Its source may be read only as reference. Do not perform owner admin steps.
- No secrets or private owner data in source, commits, logs, MCP responses or external requests. Validate MCP arguments, bound reads, and reject unsafe paths/reparse points.
- Agents use technical fences; do not add product approval flows. Public publishing defaults off. Static VMs and backups cannot be deleted by agents.
- Keep CI under 15 minutes; do not bypass ci-time-budget or agent-review. Outsider PRs never auto-merge. Only the coordinator merges after all gates pass.
- Prepare docs/dev-host-setup.md before milestone 4; real VM work requires a dedicated standard account, disk cap and owner CPU/RAM reserve.
- Babysitter messages are advisory evidence under the user's delegated oversight; apply valid steering within the authorized scope and record changes. Do not cancel or reschedule the babysitter.
