/* global URLSearchParams, document, location, window */
/* mock fixture — fields marked x / ◇ are not in the API; do not ship them.
   Night Orbit R4: one shared fixture, formatter and migration checker for every mockup page.
   Shapes follow ui/src/api/types.ts (Host, Vm, ActivityEntry). Fields that types.ts lacks live under `x`
   (x = needs an API addition) so pages can mark them. Mock data only. */
(function () {
  const GiB = 1024 ** 3, TiB = 1024 ** 4, MIN = 60e3, H = 3600e3, D = 24 * H;
  const NOW = Date.now();
  const at = ms => new Date(NOW + ms).toISOString();

  /** Host (types.ts) + x: fields not in the API yet. */
  const HOSTS = [
    { id: 'forge', name: 'forge', os: 'windows', osVersion: 'Windows 11 Pro 24H2', arch: 'x86_64',
      cpu: { model: 'Ryzen 9 7950X', cores: 16, usedPercent: 23 }, memory: { totalBytes: 64 * GiB, usedBytes: 26 * GiB },
      disk: { totalBytes: 2 * TiB, freeBytes: 812 * GiB, capBytes: 200 * GiB },
      tools: [['git', '2.47'], ['node', '22.11'], ['cargo', '1.86'], ['python', '3.13'], ['pwsh', '7.5'], ['qemu', '9.1']].map(([name, version]) => ({ name, version })),
      reachability: 'online', reachabilityReason: null, lastSeenAt: at(-20e3), mode: 'opportunistic', agentRights: 'standard',
      x: { pins: [], cpuHistory: true } },
    { id: 'atlas', name: 'atlas', os: 'linux', osVersion: 'Ubuntu 24.04 LTS', arch: 'x86_64',
      cpu: { model: 'Xeon E-2388G', cores: 8, usedPercent: 61 }, memory: { totalBytes: 32 * GiB, usedBytes: 21 * GiB },
      disk: { totalBytes: 1 * TiB, freeBytes: 640 * GiB, capBytes: 400 * GiB },
      tools: [['git', '2.47'], ['docker', '27.3'], ['compose', '2.30'], ['qemu', '9.1'], ['node', '22.11'], ['cargo', '1.86'], ['python', '3.12'], ['rg', '14.1'], ['make', null], ['jq', '1.7'], ['curl', '8.5']].map(([name, version]) => ({ name, version })),
      reachability: 'online', reachabilityReason: null, lastSeenAt: at(-5e3), mode: 'always_on', agentRights: 'standard',
      x: { pins: [], reserveBytes: 50 * GiB, cpuHistory: true } },
    { id: 'halcyon', name: 'halcyon', os: 'windows', osVersion: 'Windows Server 2025', arch: 'x86_64',
      cpu: { model: 'EPYC 4344P', cores: 8, usedPercent: 48 }, memory: { totalBytes: 64 * GiB, usedBytes: 38 * GiB },
      disk: { totalBytes: 2 * TiB, freeBytes: 1229 * GiB, capBytes: 120 * GiB },
      tools: [['git', '2.47'], ['pwsh', '7.5'], ['cargo', '1.86'], ['qemu', '9.1']].map(([name, version]) => ({ name, version })),
      reachability: 'online', reachabilityReason: null, lastSeenAt: at(-60e3), mode: 'always_on', agentRights: 'standard',
      x: { pins: [], cpuHistory: true } },
    { id: 'ember', name: 'ember', os: 'linux', osVersion: 'Ubuntu 24.04 LTS', arch: 'x86_64',
      cpu: { model: 'Core i7-12700', cores: 12, usedPercent: 7 }, memory: { totalBytes: 64 * GiB, usedBytes: 9 * GiB },
      disk: { totalBytes: 2 * TiB, freeBytes: 1536 * GiB, capBytes: 300 * GiB },
      tools: [['git', '2.43'], ['docker', '27.3'], ['qemu', '8.2'], ['python', '3.12']].map(([name, version]) => ({ name, version })),
      reachability: 'online', reachabilityReason: null, lastSeenAt: at(-10e3), mode: 'opportunistic', agentRights: 'standard',
      x: { pins: [], cpuHistory: true } },
    { id: 'pine', name: 'pine', os: 'linux', osVersion: 'Debian 12', arch: 'arm64',
      cpu: { model: 'Cortex-A76', cores: 4, usedPercent: 34 }, memory: { totalBytes: 8 * GiB, usedBytes: 5.1 * GiB },
      disk: { totalBytes: 256 * GiB, freeBytes: 21 * GiB, capBytes: 120 * GiB },
      tools: [['git', '2.39'], ['node', '18.19'], ['python', '3.11']].map(([name, version]) => ({ name, version })),
      reachability: 'degraded', reachabilityReason: 'Disk 92% full, below the 50 GiB reserve', lastSeenAt: at(-8e3), mode: 'always_on', agentRights: 'standard',
      x: { pins: [], cpuHistory: true } },
    { id: 'drift', name: 'drift', os: 'linux', osVersion: 'Fedora 41', arch: 'arm64',
      cpu: { model: 'Ampere Altra', cores: 32, usedPercent: null }, memory: { totalBytes: 128 * GiB, usedBytes: null },
      disk: { totalBytes: 1 * TiB, freeBytes: 410 * GiB, capBytes: null },
      tools: [['git', '2.47'], ['podman', '5.2'], ['qemu', '9.1']].map(([name, version]) => ({ name, version })),
      reachability: 'offline', reachabilityReason: 'No heartbeat for 3 h', lastSeenAt: at(-3 * H), mode: 'opportunistic', agentRights: 'admin',
      x: { pins: ['files-static'], cpuHistory: false } },
  ];

  const snap = (name, ago, gib) => ({ id: 'snap-' + name, name, createdAt: at(-ago), sizeBytes: gib == null ? null : gib * GiB });

  /** Vm (types.ts) + x: fields not in the API yet. */
  const VMS = [
    { id: 't-win11-a3f2', name: 't-win11-a3f2', kind: 'test', template: { id: 'win11-base', version: 7 }, guestOs: 'windows', guestOsVersion: 'Windows 11 24H2', arch: 'x86_64', hostId: 'forge', state: 'running', ready: true, expiresAt: at(2 * H + 14 * MIN), cpus: 4, memoryBytes: 8 * GiB, diskBytes: 64 * GiB, snapshots: [snap('pre-update', D, 9.4)] },
    { id: 't-win11-9d10', name: 't-win11-9d10', kind: 'test', template: { id: 'win11-base', version: 7 }, guestOs: 'windows', guestOsVersion: 'Windows 11 24H2', arch: 'x86_64', hostId: 'forge', state: 'creating', ready: false, expiresAt: at(8 * H), cpus: 4, memoryBytes: 8 * GiB, diskBytes: 64 * GiB, snapshots: [] },
    { id: 'web-static-01', name: 'web-static-01', kind: 'static', template: { id: 'ubuntu-web', version: 3 }, guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04', arch: 'x86_64', hostId: 'atlas', state: 'running', ready: true, expiresAt: null, cpus: 2, memoryBytes: 4 * GiB, diskBytes: 40 * GiB,
      snapshots: [snap('nightly-2026-11-26', 18 * MIN, 3.2), snap('nightly-2026-11-25', D, 3.1), snap('pre-tunnel', 2 * D, null), snap('nightly-2026-11-24', 2 * D + H, 3.0)],
      x: { health: { state: 'passing', check: 'GET http://localhost:80/ → 200 every 60 s', lastAt: at(-44e3) }, backup: { lastAt: at(-6 * H), policy: '7 daily + 4 weekly, copied to another host' }, publish: { on: true, hostname: 'demo.example.net', by: 'owner', at: at(-2 * H) } } },
    { id: 't-ubuntu-77c1', name: 't-ubuntu-77c1', kind: 'test', template: { id: 'ubuntu-base', version: 4 }, guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04', arch: 'x86_64', hostId: 'atlas', state: 'running', ready: true, expiresAt: at(5 * H + 2 * MIN), cpus: 2, memoryBytes: 4 * GiB, diskBytes: 20 * GiB, snapshots: [] },
    { id: 't-ubuntu-61aa', name: 't-ubuntu-61aa', kind: 'test', template: { id: 'ubuntu-base', version: 4 }, guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04', arch: 'x86_64', hostId: 'atlas', state: 'snapshotting', ready: true, expiresAt: at(47 * MIN), cpus: 2, memoryBytes: 4 * GiB, diskBytes: 20 * GiB, snapshots: [snap('deps-installed', 5 * H, 2.1), snap('base', 6 * H, 1.8)] },
    { id: 't-win11-c71e', name: 't-win11-c71e', kind: 'test', template: { id: 'win11-base', version: 7 }, guestOs: 'windows', guestOsVersion: 'Windows 11 24H2', arch: 'x86_64', hostId: 'halcyon', state: 'running', ready: false, expiresAt: at(3 * H + 10 * MIN), cpus: 4, memoryBytes: 8 * GiB, diskBytes: 64 * GiB, snapshots: [] },
    { id: 't-debian-7aa0', name: 't-debian-7aa0', kind: 'test', template: { id: 'debian-base', version: 2 }, guestOs: 'linux', guestOsVersion: 'Debian 12', arch: 'arm64', hostId: 'pine', state: 'running', ready: true, expiresAt: at(6 * H + 30 * MIN), cpus: 1, memoryBytes: 1 * GiB, diskBytes: 12 * GiB, snapshots: [] },
    { id: 't-debian-0be4', name: 't-debian-0be4', kind: 'test', template: { id: 'debian-base', version: 2 }, guestOs: 'linux', guestOsVersion: 'Debian 12', arch: 'arm64', hostId: 'pine', state: 'failed', ready: false, expiresAt: at(40 * MIN), cpus: 2, memoryBytes: 2 * GiB, diskBytes: 16 * GiB, snapshots: [] },
    { id: 'files-static', name: 'files-static', kind: 'static', template: { id: 'ubuntu-files', version: 1 }, guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04', arch: 'arm64', hostId: 'drift', state: 'stopped', ready: false, expiresAt: null, cpus: 4, memoryBytes: 8 * GiB, diskBytes: 80 * GiB,
      snapshots: [snap('nightly-2026-11-25', D, 6.2)],
      x: { health: { state: 'unknown', check: 'TCP 445 every 60 s', lastAt: null }, backup: { lastAt: at(-D), policy: '7 daily + 4 weekly, copied to another host' }, publish: { on: false, hostname: null } } },
  ];

  /** ActivityEntry (types.ts), newest first. */
  let id = 4820;
  const act = (ago, tool, outcome, kind, name, hostId, vmId, summary, exitCode = null, dur = null) => ({
    id: id--, tool, outcome, actor: { kind, name }, hostId, vmId, summary, exitCode,
    startedAt: at(-ago), finishedAt: outcome === 'started' ? null : at(-ago + (dur ?? 2e3)) });
  const ACTIVITY = [
    act(2 * MIN, 'run_command', 'succeeded', 'agent', 'builder-opus', 'forge', 't-win11-a3f2', 'cargo test --workspace', 0, 48e3),
    act(3 * MIN, 'create_vm', 'started', 'agent', 'builder-sol', 'forge', 't-win11-9d10', 'from win11-base v7'),
    act(11 * MIN, 'run_command', 'succeeded', 'agent', 'web-neon', 'atlas', 't-ubuntu-77c1', 'docker compose up -d', 0, 9e3),
    act(18 * MIN, 'snapshot_vm', 'succeeded', 'system', 'system', 'atlas', 'web-static-01', 'nightly-2026-11-26'),
    act(26 * MIN, 'run_command', 'failed', 'agent', 'web-neon', 'pine', 't-debian-0be4', 'npm ci', 1, 31e3),
    act(41 * MIN, 'destroy_vm', 'succeeded', 'system', 'system', 'atlas', 't-ubuntu-3c09', 'expired after 8 h'),
    act(58 * MIN, 'delete_vm', 'refused', 'agent', 'builder-sol', 'atlas', 'web-static-01', "static VMs can't be deleted by agents"),
    act(62 * MIN, 'read_file', 'rejected', 'agent', 'review-grok', 'halcyon', null, 'path outside the workspace'),
    act(2 * H, 'publish_tunnel', 'succeeded', 'owner', 'owner', 'atlas', 'web-static-01', 'public URL on demo.example.net'),
    act(3 * H, 'run_command', 'interrupted', 'agent', 'builder-opus', 'halcyon', 't-win11-c71e', 'cargo build --release (session lost)', null),
    act(3 * H + 5 * MIN, 'run_command', 'succeeded', 'agent', 'builder-sol', 'forge', 't-win11-a3f2', 'cargo fmt --check', 0),
    act(4 * H, 'create_vm', 'succeeded', 'agent', 'builder-opus', 'atlas', 't-ubuntu-61aa', 'from ubuntu-base v4'),
    act(4 * H + 20 * MIN, 'run_command', 'failed', 'agent', 'review-grok', 'halcyon', 't-win11-c71e', 'pwsh -File ci.ps1', 1),
    act(5 * H, 'snapshot_vm', 'succeeded', 'agent', 'builder-sol', 'atlas', 't-ubuntu-61aa', 'deps-installed'),
    act(5 * H + 30 * MIN, 'run_command', 'succeeded', 'agent', 'web-neon', 'atlas', 't-ubuntu-77c1', 'git pull --ff-only', 0),
    act(6 * H, 'destroy_vm', 'succeeded', 'system', 'system', 'forge', 't-win11-5e21', 'expired after 8 h'),
    act(6 * H + 10 * MIN, 'create_vm', 'succeeded', 'agent', 'web-neon', 'pine', 't-debian-7aa0', 'from debian-base v2'),
    act(7 * H, 'run_command', 'succeeded', 'owner', 'owner', 'atlas', null, 'git --version', 0),
    act(7 * H + 40 * MIN, 'read_file', 'rejected', 'agent', 'web-neon', 'pine', null, 'path outside the workspace'),
    act(8 * H, 'create_vm', 'failed', 'agent', 'builder-opus', 'pine', 't-debian-0be4', 'guest agent never answered (boot timeout)'),
    act(8 * H + 15 * MIN, 'run_command', 'refused', 'agent', 'builder-sol', 'drift', 'files-static', 'host drift is offline'),
    act(9 * H, 'snapshot_vm', 'succeeded', 'agent', 'builder-opus', 'forge', 't-win11-a3f2', 'pre-update'),
  ];
  /** Read-only tools, shown only with include_reads. */
  const READS = [
    act(1 * MIN, 'list_hosts', 'succeeded', 'agent', 'builder-opus', null, null, '6 hosts'),
    act(15 * MIN, 'get_vm', 'succeeded', 'agent', 'web-neon', 'atlas', 't-ubuntu-77c1', 't-ubuntu-77c1'),
    act(70 * MIN, 'list_vms', 'succeeded', 'agent', 'review-grok', null, null, '9 VMs'),
  ];

  /* ---------- helpers ---------- */
  const host = id => HOSTS.find(h => h.id === id) || null;
  const vm = id => VMS.find(v => v.id === id) || null;
  const vmsOn = hostId => VMS.filter(v => v.hostId === hostId);
  const allocated = hostId => vmsOn(hostId).reduce((a, v) => ({ cpus: a.cpus + (v.cpus ?? 0), memoryBytes: a.memoryBytes + (v.memoryBytes ?? 0), diskBytes: a.diskBytes + v.diskBytes, unknown: a.unknown || v.cpus == null || v.memoryBytes == null }), { cpus: 0, memoryBytes: 0, diskBytes: 0, unknown: false });
  const hypervisor = os => (os === 'windows' ? 'WHPX' : 'KVM');
  const OS_LABEL = { windows: 'Windows', linux: 'Linux' };
  const OS_COLOR = { windows: 'var(--win)', linux: 'var(--lin)' };
  const RESERVE = 50 * GiB; // owner disk reserve; per host value is not in the API yet

  /** Identical to ui/src/app/format.ts formatBytes (1024 base; 1 decimal below 10, none at 10+ or for bytes). */
  function fmtBytes(b) {
    if (b == null) return null;
    const u = ['B', 'KiB', 'MiB', 'GiB', 'TiB', 'PiB']; let i = 0, n = b;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return n.toFixed(i === 0 || n >= 10 ? 0 : 1) + ' ' + u[i];
  }
  function fmtAgo(iso) {
    if (!iso) return null;
    const s = Math.max(0, (Date.now() - Date.parse(iso)) / 1000);
    if (s < 45) return 'just now'; if (s < 3600) return Math.round(s / 60) + ' m ago';
    if (s < 86400) return Math.round(s / 3600) + ' h ago'; return Math.round(s / 86400) + ' d ago';
  }
  function fmtLeft(iso) {
    if (!iso) return 'never';
    const s = Math.max(0, Math.floor((Date.parse(iso) - Date.now()) / 1000));
    const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
    return h ? `${h}h ${String(m).padStart(2, '0')}m` : m ? `${m}m ${String(sec).padStart(2, '0')}s` : `${sec}s`;
  }

  /**
   * The one migration checker (roadmap preview only). Order:
   * home → VM state → source reachable → static on Linux → same hypervisor → arch →
   * target reachable (reachabilityReason) → opportunistic pin (static) → VM disk cap → disk reserve → vCPU → RAM.
   * Permanent incompatibilities come before temporary ones so the reason shown is the one that will not go away.
   * Returns { ok, reason, note }.
   */
  function canMove(v, h) {
    const src = host(v.hostId);
    if (v.hostId === h.id) return { ok: false, home: true, reason: 'Current host' };
    if (v.state !== 'running' && v.state !== 'stopped') return { ok: false, reason: `Only running or stopped VMs move (this one is ${v.state})` };
    if (src && src.reachability === 'offline') return { ok: false, reason: `Source host offline: ${src.reachabilityReason}` };
    if (v.kind === 'static' && h.os !== 'linux') return { ok: false, reason: 'Static VMs run on Linux hosts only' };
    if (src && hypervisor(src.os) !== hypervisor(h.os)) return { ok: false, reason: "Cross-hypervisor moves aren't supported yet" };
    if (v.arch !== h.arch) return { ok: false, reason: `Arch differs: ${h.arch} host, ${v.arch} VM` };
    if (h.reachability !== 'online') return { ok: false, reason: h.reachabilityReason || 'Host not online' };
    if (v.kind === 'static' && h.mode === 'opportunistic' && !h.x.pins.includes(v.id)) return { ok: false, reason: 'Opportunistic host: static VMs need a pin' };
    const a = allocated(h.id);
    if (h.disk.capBytes != null && a.diskBytes + v.diskBytes > h.disk.capBytes) return { ok: false, reason: `Needs ${fmtBytes(v.diskBytes)} of VM disk, ${fmtBytes(h.disk.capBytes - a.diskBytes)} left under the ${fmtBytes(h.disk.capBytes)} cap` };
    if (h.disk.freeBytes - v.diskBytes < RESERVE) return { ok: false, reason: `Would leave less than the ${fmtBytes(RESERVE)} disk reserve` };
    if (v.cpus != null && a.cpus + v.cpus > h.cpu.cores) return { ok: false, reason: `Needs ${v.cpus} vCPU, ${h.cpu.cores - a.cpus} free` };
    if (v.memoryBytes != null && a.memoryBytes + v.memoryBytes > h.memory.totalBytes) return { ok: false, reason: `Needs ${fmtBytes(v.memoryBytes)} RAM, ${fmtBytes(h.memory.totalBytes - a.memoryBytes)} free` };
    const note = h.mode === 'opportunistic' ? 'Opportunistic host: test VMs may be moved away while its owner is active' : null;
    return { ok: true, reason: 'Warm migration: pre-copy, then about 3 minutes of downtime', note };
  }

  const totals = () => ({
    hosts: HOSTS.length,
    online: HOSTS.filter(h => h.reachability === 'online').length,
    degraded: HOSTS.filter(h => h.reachability === 'degraded').length,
    offline: HOSTS.filter(h => h.reachability === 'offline').length,
    vms: VMS.length,
    running: VMS.filter(v => v.state === 'running').length,
    test: VMS.filter(v => v.kind === 'test').length,
    static: VMS.filter(v => v.kind === 'static').length,
  });

  const params = new URLSearchParams(location.search);
  const param = n => params.get(n);
  const demo = params.has('demo');
  if (demo) document.documentElement.classList.add('demo');

  /* ---------- shell behaviour: `/` focuses search, jump list, sync state ---------- */
  function initShell() {
    const input = document.querySelector('.search input');
    document.addEventListener('keydown', e => {
      if (e.key === '/' && input && document.activeElement !== input && !/^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName || '')) { e.preventDefault(); input.focus(); }
    });
    if (input) {
      const wrap = input.closest('.search'); let list = null, items = [], sel = -1;
      const close = () => { list?.remove(); list = null; sel = -1; input.setAttribute('aria-expanded', 'false'); };
      input.setAttribute('role', 'combobox'); input.setAttribute('aria-expanded', 'false'); input.setAttribute('aria-autocomplete', 'list');
      const render = () => {
        const q = input.value.trim().toLowerCase(); if (!q) return close();
        items = [
          ...HOSTS.filter(h => h.id.includes(q)).map(h => ({ href: `host.html?id=${h.id}`, label: h.id, kind: 'host' })),
          ...VMS.filter(v => v.id.includes(q)).map(v => ({ href: `vm.html?id=${v.id}`, label: v.id, kind: v.kind + ' VM' })),
          ...HOSTS.flatMap(h => h.tools.filter(t => t.name.includes(q)).map(t => ({ href: `terminal.html?host=${h.id}&tool=${t.name}`, label: `${t.name} on ${h.id}`, kind: 'tool' }))),
        ].slice(0, 8);
        if (!list) { list = document.createElement('div'); list.className = 'jump'; list.id = 'jump'; list.setAttribute('role', 'listbox'); wrap.appendChild(list); input.setAttribute('aria-controls', 'jump'); }
        input.setAttribute('aria-expanded', 'true');
        list.innerHTML = items.length ? items.map((it, i) => `<a role="option" id="jump-${i}" href="${it.href}" aria-selected="${i === sel}">${it.label}<small>${it.kind}</small></a>`).join('') : `<p>No host, VM or tool matches “${input.value.replace(/[<&]/g, '')}”</p>`;
        input.setAttribute('aria-activedescendant', sel >= 0 ? `jump-${sel}` : '');
      };
      input.addEventListener('input', () => { sel = -1; render(); });
      input.addEventListener('keydown', e => {
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { if (!items.length) return; e.preventDefault(); sel = (sel + (e.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length; render(); }
        else if (e.key === 'Enter' && items.length) { e.preventDefault(); location.href = items[Math.max(0, sel)].href; }
        else if (e.key === 'Escape') { close(); }
      });
      document.addEventListener('pointerdown', e => { if (list && !wrap.contains(e.target)) close(); });
    }
    if (params.has('stale')) {
      const s = document.querySelector('.sync');
      if (s) { s.classList.add('stale'); s.innerHTML = '<span class="pulse"></span>served by <b>atlas</b> · stale, last synced 2 m ago'; }
    }
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', initShell); else initShell();

  window.ARX = { GiB, TiB, NOW, HOSTS, VMS, ACTIVITY, READS, host, vm, vmsOn, allocated, hypervisor, OS_LABEL, OS_COLOR, RESERVE, fmtBytes, fmtAgo, fmtLeft, canMove, totals, param, demo };
})();
