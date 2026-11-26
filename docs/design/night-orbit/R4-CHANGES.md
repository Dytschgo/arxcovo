# Night Orbit R4: what changes from R3 (read with SPEC.md)

R4 = R3 pages + the council round-2 fixes folded into this package.
Where this file and SPEC.md disagree, **this file wins**.

## Shared foundation (already done, use it, don't fork it)
- `arx-data.js`: the ONE fixture (`ARX.HOSTS`, `ARX.VMS`, `ARX.ACTIVITY`, `ARX.READS`), helpers (`host(id)`, `vm(id)`, `vmsOn`, `allocated`, `totals()`), the 1024-based `fmtBytes` (GiB/TiB, same as the product's `formatBytes`), `fmtAgo`, `fmtLeft` (absolute-instant countdown, keeps running when stopped), **the one migration checker `canMove(vm, host)`**, `param(name)`, `demo`. Shapes follow `types.ts`; fields the API lacks sit under `x` (health, backup, publish, pins, reserve, cpu history). Load it with `<script src="arx-data.js"></script>` (in the shell). Delete every page-local copy of hosts/VMs/activity/rules/byte maths and read from `ARX` instead. Page-specific extras (e.g. a CPU series generator, extra older activity rows for paging) may stay local but must be built from `ARX` data.
- The fleet changed: **6 hosts** (new `ember`: Ubuntu x86_64, opportunistic, online, empty), halcyon's VM disk cap is 120 GiB, drift's disk totals are last-known numbers (only CPU/memory used are null), **9 VMs** (new `t-debian-7aa0` running on pine). Totals come from `ARX.totals()`: 6 hosts, 4 online, 1 degraded, 1 offline, 9 VMs, 5 running, 7 test, 2 static.
- `night-orbit.css`: self-hosted Geist (no Google Fonts anywhere), visible white focus ring on menu items, `.jump` search list, `.sync.stale`, `.apimark`, and demo gating: anything with a `data-demo` attribute is hidden unless the URL has `?demo` (use class `demo-flex` too if it must be flex).
- `_shell.html`: no Google Fonts links, loads `arx-data.js`, sign-out goes to `login.html?state=signedout`. `/`, the search jump list (hosts, VMs, tools; arrows + Enter) and `?stale` (sync shows "stale, last synced 2 m ago") are wired by `arx-data.js`; don't re-implement them.

## Rules for every page
1. **Routing is real.** `host.html?id=<host>`, `vm.html?id=<vm>`, `activity.html?host=<id>&vm=<id>&outcome=<o>`, `terminal.html?host=<id>&tool=<name>`, `vm.html?id=<vm>#console` (the console section has `id="console"`). One query name per page as listed; every link in every page uses these. Unknown id → a calm "Not found" state with a link back.
2. **No demo controls on the settled surface.** Every demo switch (host offline, long id, tool count, console live/stopped/error, terminal failure/4401, login states, migration failure) gets `data-demo` and lives in one footer strip labelled "Demo". Without `?demo` nothing of it renders.
3. **API honesty, quietly.** One line per page footer: `Mock data · ◇ = field the API doesn't have yet`. On the surface, a field the API lacks gets only the tiny `.apimark` "◇" after its label, not a chip. Exception kept: the one dashed `needs API: cpu history` chip next to a CPU trace (owner decision). No disabled controls that exist only to carry a chip (remove Activity's disabled time-range slider; it is now in SPEC's API wishlist).
4. **Sizes** always via `ARX.fmtBytes` (GiB/TiB).
5. **Every VmState and ActivityOutcome** renders with icon + word. **Actor** always shows its kind word: `agent · builder-sol`, `owner`, `system`.
6. Keep everything that already works in R3 (look, motion, keyboard, ARIA, 1280 px fit, reduced motion).

## Migration preview (board + VM detail)
- Both use `ARX.canMove` and nothing else; the board's drag labels, Move menu, drain dialog and VM detail's "Compatible hosts" must show identical reasons.
- Success path exists (t-ubuntu-77c1 → ember, t-win11-c71e → forge with the opportunistic note shown in the confirm dialog).
- **Failure path:** a `data-demo` switch "Next move fails at cutover" → the move runs pre-copy, then fails: card returns to its source, state word "Migration failed · VM still on <source>", ticker/activity line `migrate_vm failed`, and a Retry. Cancel during pre-copy stays as is (outcome interrupted).
- Under reduced motion the pre-copy timer completes instantly (no stuck 0 %).

## Owner-facing open items recorded in SPEC (don't build UI for them)
Light theme (not planned: the owner decided dark only on 2026-11-26, ADR 0073), notifications settings (not in v1 API), host remote desktop (not in v1 API; one disabled "Remote desktop ◇" action on host detail is OK), agents-may-publish cluster setting (◇).
