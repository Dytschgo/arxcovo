# 0073: Dashboard design language: Night Orbit

Status: accepted (2026-11-26). Owner decisions (Dylan, 2026-11-26, dashboard-design thread). Reference package: [docs/design/night-orbit/](../design/night-orbit/).

## Context

Milestone 6 ([docs/KICKOFF.md](../KICKOFF.md), [docs/dashboard-api.md](../dashboard-api.md), [docs/m6-acceptance.md](../m6-acceptance.md)) has a working React/Vite/Electron dashboard backed by `arxd` session auth ([ADR 0061](0061-dashboard-owner-sessions.md)), command streams ([ADR 0062](0062-owner-dashboard-command-streams.md)), and live CPU/memory sampling (#362). Its initial UI was a minimal scaffold with a light/dark/system theme switch (`ui/src/app/theme.ts`, `ui/src/styles.css`) and a flat host list.

A dedicated design exploration produced the **Night Orbit** visual language (R4 package checked into [docs/design/night-orbit/](../design/night-orbit/) with mockups, tokens in `night-orbit.css`, self-hosted Geist / Geist Mono fonts under SIL OFL 1.1, reference screenshots in `shots/`, and the CPU-history API proposal in [CPU-HISTORY-API.md](../design/night-orbit/CPU-HISTORY-API.md)). On 2026-11-26 the owner settled the product decisions needed to adopt Night Orbit for the v1 dashboard.

## Decision

1. **Night Orbit is the dashboard design language.** Visual tokens, pure-black surfaces, hairlines, self-hosted Geist and Geist Mono typography (`font-src 'self'`), O3 logo, icon+word status badges, OS data-only accents, hatched nulls (never rendering `null` as `0`), and `prefers-reduced-motion` settled states follow [docs/design/night-orbit/SPEC.md](../design/night-orbit/SPEC.md) and [night-orbit.css](../design/night-orbit/night-orbit.css).
2. **No light mode: the theme switch is removed.** Night Orbit is dark-only. The light/dark/system toggle and `ui/src/app/theme.ts` are removed rather than maintaining a second palette.
3. **The host board becomes the v1 Hosts view.** One column per host with its VMs inside replaces the flat hosts list in v1, with computed header totals and horizontal board scrolling at 1280 px without page overflow.
4. **Migration and drain are not shipped in v1.** Drag-and-drop migration, Move menus, and host drain remain post-v1 work; the Night Orbit migration preview is recorded as a post-v1 design input in [docs/ROADMAP.md](../ROADMAP.md).
5. **CPU history is requested from the API.** The backend adds per-minute ring-buffer retention on top of the #362 sampler and exposes `GET /api/v1/hosts/{id}/cpu-history` and `GET /api/v1/hosts/cpu-history` per [CPU-HISTORY-API.md](../design/night-orbit/CPU-HISTORY-API.md). The UI draws traces only once that API exists and degrades cleanly without it.
6. **Future owner-initiated moves ask the owner for confirmation; agents never get prompts.** When post-v1 migration lands, an owner drop or Move-menu choice opens an explicit confirmation dialog naming the VM, target host, and warm-migration downtime. Per [docs/KICKOFF.md](../KICKOFF.md) (line 37), agents still have no approval prompts anywhere and remain governed solely by technical fences.

### Amended documents and theme-reference audit

This ADR amends the following repository documents without rewriting `docs/KICKOFF.md`:

- **[docs/KICKOFF.md](../KICKOFF.md)** (lines 35, 83–87): moves the host-board layout (one column per host with VMs inside) into v1 M6 as the Hosts view while keeping warm migration and host drain post-v1, and adopts Night Orbit (dark-only) as the M6 design language. Line 37 (no agent approval prompts; technical fences only) is unchanged.
- **[docs/dashboard-api.md](../dashboard-api.md)**: remains the v1 HTTP/WebSocket contract. Self-hosted fonts preserve its existing `font-src 'self'` CSP, and the CPU-history backend PR will extend it with the endpoints specified in [CPU-HISTORY-API.md](../design/night-orbit/CPU-HISTORY-API.md).
- **[docs/m6-acceptance.md](../m6-acceptance.md)**: the M6 live-backend Playwright suite (`ui/tests/live/dashboard.live.ts`) and committed masked screenshots (`docs/evidence/m6/live/`) will be refreshed in PR B to match the Night Orbit views.

Audit of `docs/` for theme and light-mode references (case-insensitive `git grep` on `origin/main` before this ADR):

- `"light mode"`: no matches in `docs/`.
- `"theme"`: no matches in `docs/`.
- `"prefers-color-scheme"`: no matches in `docs/`.

The existing light/dark/system switch and `light-dark()` tokens live only in `ui/src/app/theme.ts`, `ui/src/App.tsx`, `ui/src/App.test.tsx`, and `ui/src/styles.css`, which PR A removes.

### Implementation plan

Work is split into three coherent PRs following [IMPLEMENTATION-HANDOFF.md](../design/night-orbit/IMPLEMENTATION-HANDOFF.md):

1. **PR A — UI foundation (Claude UI builder):** port Night Orbit tokens from `night-orbit.css` into `ui/src/styles.css`; self-host `Geist-Variable.woff2` and `GeistMono-Variable.woff2` (with `OFL-LICENSE.txt`) under `ui/public/fonts`; adopt the O3 logo and top bar from `_shell.html`; remove the theme switch and `ui/src/app/theme.ts`; add shared components for `VmState` / `ActivityOutcome` icon+word badges, VM kind markers, arch badges, hatched nulls, and VM cards.
2. **PR B — UI views and M6 live-suite refresh (Claude UI builder):** restyle `HostsView` as the host board (without migration), `HostDetailView` as bento, `VmsView` as table, `VmDetailView` + `ConsoleView` as VM detail with view-only console tile, plus `TerminalView`, `ActivityView`, `LoginView`, and search jump list over loaded hosts/VMs/tools; refresh `ui/tests/live/dashboard.live.ts` selectors and `docs/evidence/m6/live/` masked screenshots.
3. **CPU-history backend (Sol engineering builder):** implement [CPU-HISTORY-API.md](../design/night-orbit/CPU-HISTORY-API.md) on top of the #362 sampler, update `docs/dashboard-api.md` and `ui/src/api/types.ts`, and wire traces into the UI once the endpoint lands.

## Consequences

- The checked-in design package under [docs/design/night-orbit/](../design/night-orbit/) gives UI and backend builders a single durable, repo-relative reference with no dependency on local scratch directories.
- Removing light mode simplifies token maintenance and contrast verification (all readable text tokens clear 4.5:1 on `--tile2`).
- Self-hosting Geist and Geist Mono preserves the zero-external-request posture and existing `font-src 'self'` CSP in both `arxd` and the Electron shell.
- Keeping existing routes, API types, and hooks unchanged lets PR A and PR B land independently of the CPU-history backend and keeps M6 behavior intact.
- Refreshing the M6 live suite and screenshots in PR B keeps live acceptance aligned with the final Night Orbit markup.

## Not shipped

The following items appear in the mockups under [docs/design/night-orbit/](../design/night-orbit/) for exploration or future continuity. None may be fabricated in the product. Migration preview and host drain stay post-v1. The API-dependent items ship only once the corresponding backend API exists; CPU history is being built for v1, so its traces may ship in v1 if that API lands first:

- **Migration preview and host drain:** drag-and-drop migration, Move menus, cutover flows, and drain dialogs stay post-v1 (tracked in [docs/ROADMAP.md](../ROADMAP.md)).
- **`◇` API-mark fields (`x` fields in `arx-data.js`):** static VM health checks, backups, publish state, per-host disk reserve, opportunistic host pins, and board join code — plus the wishlist items (activity time-range slider, host remote desktop, and notifications settings) — do not ship unless and until corresponding backend API fields exist.
- **CPU history traces, time window, and avg/peak summaries:** stay unshipped until the CPU-history API ([CPU-HISTORY-API.md](../design/night-orbit/CPU-HISTORY-API.md)) lands. Until then, only the current `cpu.usedPercent` sample (#362) renders, showing hatched `"usage unavailable"` when `null`.
