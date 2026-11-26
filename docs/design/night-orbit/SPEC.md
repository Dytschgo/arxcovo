# Night Orbit R4: build spec for the mockup pages

> **Note:** mock fixture — fields marked `x` / `◇` are not in the API; do not ship them.

Mockups only, fake data. Nothing goes into the product. Mockup files live in `docs/design/night-orbit/`.
Follow `docs/dev-scratch.md` for task-owned `TMP`/`TEMP` paths on D:; never use C: for scratch. No real names, hostnames, IPs or secrets.

## Read first
- `night-orbit.css` (this folder): shared tokens and components. **Link it** (`<link rel="stylesheet" href="night-orbit.css">`) and use its classes; add only page-specific CSS inline. Don't redefine the tokens.
- `_shell.html` (this folder): the exact logo SVG, the icon sprite and the top bar. Copy them verbatim into every page (login uses the logo only).
- The approved look: settled R4 pages `board.html` and `host.html`, reference screenshots `shots/board.jpg` and `shots/host.jpg`, and the shared tokens in `night-orbit.css` (superseding the earlier R2 scratch drafts).
- The council findings folded into R4: summarized in `R4-CHANGES.md`.
- Real data contract (read-only, from the repository root): `docs/dashboard-api.md`, `ui/src/api/types.ts`, current views in `ui/src/app/views/`, `docs/KICKOFF.md`, `docs/ROADMAP.md`, `AGENTS.md`. **Every field you show must exist in `types.ts`, or carry the quiet `◇` API mark (`.apimark`) explained once in the page footer.** The only chip kept is the dashed `needs API: cpu history` next to a CPU trace.

## Owner decisions (2026-11-26)
1. **Board is the v1 Hosts view; migration is not implemented in v1.** VM cards are not draggable by default. A clearly labelled switch "Preview migration (roadmap)" on the board turns on drag + the Move menu so the owner can see the future flow. Default off.
2. **CPU history: the design asks for it.** Keep traces/time window in the design, but every history element carries a dashed chip `needs API: cpu history` (no history endpoint exists; `cpu.usedPercent` is a single nullable sample, null today). The current-value number must be shown on its own and render "usage unavailable" (hatched) when null.
3. **Owner drag asks first.** In migration preview, a drop or a Move-menu choice opens a confirm dialog: "Move <vm> to <host>? Warm migration: pre-copy, then about 3 minutes of downtime." Cancel / Move. Agents never get prompts (KICKOFF line 37), so the dialog says "Owner action" only.

## Product rules the mocks must obey
- Windows hosts run **TEST VMs only**. Static VMs are Linux guests on Linux hosts in v1 (KICKOFF line 27, ADR 0058).
- macOS hosts and guests are roadmap: **no macOS host in the v1 data.** At most a "Join a host" ghost column/tile.
- Agents can't delete static VMs or backups. Public publishing defaults **off**; turning it on is an owner action.
- Terminal is **not a shell**: the bounded `run_command` executor, one discovered tool + args + optional timeout (1–3600 s), one command at a time, cancel, stdout/stderr chunks, exit code may be null, output may be truncated. 503 / disabled when the host is offline. Label it "Run command" or "Terminal (run a tool)".
- Console is **view-only frames** in v1 (1–2 fps screendumps, PNG/JPEG), states connecting / streaming / closed / error, Reconnect. Interactive VNC is later.
- There is **no create-VM API** for the owner. No "New test VM" button.
- No IP address or uptime on hosts (not API fields). Use last seen, mode, agent rights, OS + version, arch, cpu model + cores, memory, disk + cap.
- `VmState` = creating | running | stopped | snapshotting | migrating | failed | destroying. `ActivityOutcome` = started | succeeded | failed | rejected | refused | interrupted. Render all of them (icon + word).
- Activity rows: tool (mono), summary, actor kind + name, host, VM, exit code when present, outcome word, time.
- Nullable numbers render hatched with a reason, never 0.
- Migration rules for the preview: only `running` or `stopped` VMs move; same hypervisor family only ("Cross-hypervisor moves aren't supported yet" for Windows↔Linux hosts); arch must match; static VMs only to Linux hosts and not to an opportunistic host unless pinned; target online; VM disk must fit in free disk after the reserve and cap; vCPU/RAM must fit; refusal text comes from the rule that failed (or the host's `reachabilityReason`).

## Accessibility
Text at `--ink4` or brighter (never `--deco` for text). Every control reachable by keyboard; menus use roving focus with ArrowUp/ArrowDown, include disabled items as `aria-disabled="true"` with their reason in the accessible name, and return focus to the trigger on Escape. Sliders set `aria-valuemin/max/now/valuetext`. `prefers-reduced-motion` = finished state. Must not overflow at 1280 px; the board scrolls horizontally with a visible scrollbar if columns don't fit.

## Shared fake data
The fixture is `arx-data.js` and nothing else: 6 hosts (forge, atlas, halcyon, ember, pine, drift) and 9 VMs, shapes from `types.ts`, API-less fields under `x`. Totals come from `ARX.totals()` (6 hosts, 4 online, 1 degraded, 1 offline, 9 VMs, 5 running, 7 test, 2 static). Activity is `ARX.ACTIVITY` (+ `ARX.READS` with include reads). Never hard-code a count or copy rows into a page.

## Visual rules (unchanged from Night Orbit)
Pure black, hairlines, tiles #070707 radius 18, white is the only UI accent, OS colour only for data (Windows `--win`, Linux `--lin`), status always icon + word, one main number per tile, Geist + Geist Mono, entrance rise with 70 ms stagger, numbers count up, traces draw in. Keep it calm: less text, more icons.

## Deliverable checks
Open each page in Chromium via the repository's Playwright installation (with `TMP`/`TEMP` and `PLAYWRIGHT_BROWSERS_PATH` on D: per `docs/dev-scratch.md`) at 1440 and 1280: no console errors, no page overflow, everything settled by ~3 s. Exercise every interactive control you built. Report what you checked.

## R4 additions (also in R4-CHANGES.md)
- Fixture, formatter and migration rules live in `arx-data.js` only. 6 hosts, 9 VMs.
- Fonts are self-hosted (`fonts/`, SIL OFL); the product CSP is `font-src 'self'`.
- Sizes render through 1024-based units (GiB/TiB), like the product `formatBytes`.
- No demo control ships: they render only with `?demo`.

## API wishlist (fields the design shows with ◇ or a needs-API chip)
| Need | Used by | Notes |
|---|---|---|
| CPU history series | board traces, host CPU chart + window, avg/peak | Concrete proposal: `CPU-HISTORY-API.md`. Owner asked for it (2026-11-26). Until it lands, only the current sample (or "usage unavailable") ships; traces, window and avg/peak stay design-only behind their chip. |
| ~~Real CPU sampling~~ | every "CPU now" | Done in #362 (background sampler, 1 s). Null still renders "usage unavailable". |
| Activity time range | Activity page | `ActivityQuery` has only a cursor; a time slider needs `since`/`until`. |
| Static VM health, backups, publish state | board/host/VM cards, VM detail | Not in `Vm`. Publishing defaults off; turning it on is an owner action. |
| Disk reserve per host, opportunistic pins | host disk tile, migration rules | Not in `Host`. |
| Join code | board "Join a host" | No endpoint in the dashboard API. |
| Migration + drain | board preview, VM detail | Roadmap; no endpoint. Owner drag asks first; agents never get prompts. |
| Host remote desktop | host detail | KICKOFF remote access; not in the v1 API. |
| Notifications settings | none drawn | KICKOFF; not in the v1 API. |

## Owner decisions, continued
- 2026-11-26: **no light mode.** Night Orbit is dark only; the current light/dark/system switch goes away.
