# Night Orbit: implementation handoff

From the design-exploration thread, 2026-11-26. **Owner decisions:** Night Orbit is the dashboard design language; no light mode; the board is the v1 Hosts view with migration **not** shipped; CPU history is requested from the API; the owner's own move asks for confirmation (agents never get prompts). The owner asked the design thread to "do what's best" about implementation. This file is that recommendation; the coordinator schedules and routes it under AGENTS.md.

Design package: `docs/design/night-orbit/` (`index.html`, `SPEC.md`, `R4-CHANGES.md`, `arx-data.js`, `night-orbit.css`, `fonts/`, `shots/`, `CPU-HISTORY-API.md`). Council: Grok 4.7 and Muse Wonder, three rounds, 7 → 8 → 9/10, both items from the last round fixed; the resulting changes are captured in `R4-CHANGES.md`.

## Recommendation
1. **Move the package into the repo first** (docs-only PR): `docs/design/night-orbit/` with the mockups, `SPEC.md`, the fonts licence, and an ADR "Dashboard design language: Night Orbit" recording the owner decisions (dark only, board as Hosts, migration preview not shipped, confirm-on-move for the owner). Local scratch copies can be cleaned once merged.
2. **Restyle the existing M6 UI in two Claude UI PRs** (AGENTS routing: UI work with Claude; independent reviewer separate from the builder):
   - **PR A: foundation.** Tokens from `night-orbit.css` into `ui/src/styles.css`; self-host Geist + Geist Mono under `ui/public/fonts` (CSP `font-src 'self'` already holds); the O3 logo and top bar from `_shell.html`; remove the theme switch and `app/theme.ts` (no light mode); shared components (`ui.tsx`): status icon + word for every `VmState`/`ActivityOutcome`, kind marker + word, arch badge, hatched null, VM card.
   - **PR B: views.** `HostsView` → board (one column per host, VM cards, computed header totals, join entry only if an API exists, otherwise omit); `HostDetailView` → bento; `VmsView` → table; `VmDetailView` + `ConsoleView` → VM detail with console tile; `TerminalView`, `ActivityView`, `LoginView` restyled. Search jump list over loaded hosts/VMs/tools. Keep the current routes, data hooks and API types; the design was built on `types.ts`.
   - **Not shipped:** migration preview and drain (roadmap), the ◇ API-mark fields (health, backups, publish state, reserve, pins, join code) unless the API gains them, the CPU trace and time window until CPU history lands. "CPU now" already works (#362) and shows "usage unavailable" for null.
3. **CPU history (backend, Sol per routing):** implement `CPU-HISTORY-API.md` on top of the #362 sampler (per-minute ring buffer, `GET /api/v1/hosts/{id}/cpu-history`, batch endpoint, `CpuHistory` type). The UI draws traces only once it exists. If it threatens the v1 date, put it in ROADMAP; the UI degrades cleanly.
4. **Timing:** M6 code is merged and only the owner-host acceptance run is open. Landing PR A + B **before** that run means acceptance is exercised once, on the final look. The live suite (`ui/tests/live/dashboard.live.ts`) and `docs/evidence/m6/live/` screenshots must be refreshed in PR B; selectors that depend on old markup will change. If v1 timing is tight, the coordinator may instead run M6 acceptance on the current UI and land the restyle right after; the design does not change any behaviour or API.

## Acceptance for the restyle
- Every view in the current app exists in the new language (hosts board, host, VMs, VM + console, terminal, activity, login, not found, session ended).
- `npm run build`, `npm test` and the M6 live suite pass; screenshots refreshed and masked as today.
- No external requests (fonts self-hosted); no theme switch; text contrast ≥ 4.5:1 (tokens in `night-orbit.css`); `prefers-reduced-motion` shows the finished state; no horizontal page overflow at 1280 px (board scrolls inside itself).
- Nothing shows a value as 0 when the API gives null.

## Data honesty notes for builders
- The mock fixture (`arx-data.js`) is fake. Its `x` fields are not in the API; don't invent them in the product.
- Sizes through the existing `formatBytes`.
- Terminal is the bounded `run_command` session, not a shell. Console is view-only frames.
