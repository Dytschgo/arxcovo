// Deterministic stand-in data. Only reachable from dev and test builds; the
// marker string lets the desktop smoke prove production bundles exclude it.
import type { ActivityEntry, ActivityOutcome, ActorKind, Host, Vm } from './types';

export const STANDIN_MARKER = 'arx-standin-fixture-v1';
export const DEMO_CREDENTIALS = { username: 'owner', password: 'arxcovo-demo' } as const;
/** Fixed clock for fixtures: 2026-11-23T12:00:00Z. */
export const FIXTURE_NOW = Date.UTC(2026, 10, 23, 12, 0, 0);

const GiB = 1024 ** 3;
const TiB = 1024 ** 4;
const at = (minutesAgo: number) => new Date(FIXTURE_NOW - minutesAgo * 60_000).toISOString();
const inDays = (days: number) => new Date(FIXTURE_NOW + days * 86_400_000).toISOString();

const devTools = (names: string[]) => names.map(n => {
  const [name, version] = n.split('@');
  return { name, version: version ?? null };
});

export const FIXTURE_HOSTS: Host[] = ([
  {
    id: 'atlas', name: 'atlas', os: 'linux', osVersion: 'Ubuntu 24.04.3 LTS', arch: 'x86_64',
    cpu: { model: 'AMD Ryzen 9 7950X', cores: 16, usedPercent: 38 },
    memory: { totalBytes: 128 * GiB, usedBytes: 71 * GiB },
    disk: { totalBytes: 4 * TiB, freeBytes: 2.6 * TiB, capBytes: 2 * TiB },
    tools: devTools(['git@2.43.0', 'cargo@1.91.0', 'node@24.18.0', 'python3@3.12.3', 'qemu-system-x86_64@8.2.2', 'swtpm@0.8.0']),
    reachability: 'online', reachabilityReason: null, lastSeenAt: at(0), mode: 'always_on', agentRights: 'standard',
  },
  {
    id: 'studio', name: 'studio', os: 'macos', osVersion: 'macOS 26.1', arch: 'arm64',
    cpu: { model: 'Apple M4 Pro', cores: 14, usedPercent: 21 },
    memory: { totalBytes: 48 * GiB, usedBytes: 30 * GiB },
    disk: { totalBytes: 2 * TiB, freeBytes: 0.9 * TiB, capBytes: 0.5 * TiB },
    tools: devTools(['git@2.50.1', 'cargo@1.91.0', 'node@24.18.0', 'swift@6.2', 'xcodebuild@26.1']),
    reachability: 'online', reachabilityReason: null, lastSeenAt: at(1), mode: 'opportunistic', agentRights: 'standard',
  },
  {
    id: 'forge', name: 'forge', os: 'windows', osVersion: 'Windows 11 Pro 25H2', arch: 'x86_64',
    cpu: { model: 'Intel Core i7-13700', cores: 16, usedPercent: 84 },
    memory: { totalBytes: 64 * GiB, usedBytes: 52 * GiB },
    disk: { totalBytes: 2 * TiB, freeBytes: 0.31 * TiB, capBytes: 0.4 * TiB },
    tools: devTools(['git@2.51.0', 'cargo@1.91.0', 'node@24.18.0', 'pwsh@7.5.4']),
    reachability: 'degraded', reachabilityReason: 'Owner is active: user CPU and RAM reserve in force', lastSeenAt: at(2), mode: 'opportunistic', agentRights: 'admin',
  },
  {
    id: 'pebble', name: 'pebble', os: 'linux', osVersion: 'Debian 13', arch: 'arm64',
    cpu: { model: 'Raspberry Pi 5 (Cortex-A76)', cores: 4, usedPercent: null },
    memory: { totalBytes: 8 * GiB, usedBytes: null },
    disk: { totalBytes: 0.5 * TiB, freeBytes: 0.42 * TiB, capBytes: null },
    tools: devTools(['git@2.47.2', 'python3@3.13.5']),
    reachability: 'offline', reachabilityReason: 'No heartbeat for 2 hours', lastSeenAt: at(127), mode: 'always_on', agentRights: 'standard',
  },
] satisfies Host[]).map(h => ({ ...h, disk: { ...h.disk, totalBytes: Math.round(h.disk.totalBytes), freeBytes: Math.round(h.disk.freeBytes), capBytes: h.disk.capBytes === null ? null : Math.round(h.disk.capBytes) } }));

export const FIXTURE_VMS: Vm[] = [
  {
    id: 'win11-ci-01', name: 'win11-ci-01', kind: 'test', template: { id: 'windows-11-x64', version: 3 }, guestOs: 'windows', guestOsVersion: 'Windows 11 Pro 25H2',
    arch: 'x86_64', hostId: 'atlas', state: 'running', ready: true, expiresAt: inDays(5), cpus: 4, memoryBytes: 8 * GiB, diskBytes: 64 * GiB,
    snapshots: [{ id: 'snap-clean', name: 'clean-install', createdAt: at(2880), sizeBytes: 3 * GiB }],
  },
  {
    id: 'ubuntu-build-02', name: 'ubuntu-build-02', kind: 'test', template: { id: 'ubuntu-24-04-x64', version: 7 }, guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04.3 LTS',
    arch: 'x86_64', hostId: 'atlas', state: 'running', ready: true, expiresAt: inDays(6), cpus: 8, memoryBytes: 16 * GiB, diskBytes: 80 * GiB,
    snapshots: [
      { id: 'snap-deps', name: 'deps-installed', createdAt: at(600), sizeBytes: 2 * GiB },
      { id: 'snap-pre-test', name: 'before-integration-tests', createdAt: at(45), sizeBytes: Math.round(0.4 * GiB) },
    ],
  },
  {
    id: 'macos-dev', name: 'macos-dev', kind: 'static', template: { id: 'macos-26-arm64', version: 1 }, guestOs: 'macos', guestOsVersion: 'macOS 26.1',
    arch: 'arm64', hostId: 'studio', state: 'running', ready: true, expiresAt: null, cpus: 6, memoryBytes: 16 * GiB, diskBytes: 120 * GiB,
    snapshots: [{ id: 'nightly-1122', name: 'nightly 2026-11-22', createdAt: at(720), sizeBytes: 9 * GiB }],
  },
  {
    id: 'ubuntu-arm-03', name: 'ubuntu-arm-03', kind: 'test', template: { id: 'ubuntu-24-04-arm64', version: 2 }, guestOs: 'linux', guestOsVersion: 'Ubuntu 24.04.3 LTS',
    arch: 'arm64', hostId: 'studio', state: 'creating', ready: false, expiresAt: inDays(7), cpus: 4, memoryBytes: 8 * GiB, diskBytes: 40 * GiB, snapshots: [],
  },
  {
    id: 'win11-test-04', name: 'win11-test-04', kind: 'test', template: { id: 'windows-11-x64', version: 3 }, guestOs: 'windows', guestOsVersion: 'Windows 11 Pro 25H2',
    arch: 'x86_64', hostId: 'forge', state: 'stopped', ready: false, expiresAt: inDays(1), cpus: 4, memoryBytes: 8 * GiB, diskBytes: 64 * GiB, snapshots: [],
  },
];

const ACTORS: { kind: ActorKind; name: string }[] = [
  { kind: 'agent', name: 'claude-code-1' },
  { kind: 'agent', name: 'codex-ci' },
  { kind: 'owner', name: 'owner' },
  { kind: 'system', name: 'arxd' },
];

const SCRIPT: { tool: string; outcome: ActivityOutcome; summary: string; host?: string; vm?: string; exit?: number }[] = [
  { tool: 'run_command', outcome: 'succeeded', summary: 'cargo test --workspace', host: 'atlas', exit: 0 },
  { tool: 'list_hosts', outcome: 'succeeded', summary: 'Listed 4 hosts' },
  { tool: 'run_command', outcome: 'failed', summary: 'npm run build', host: 'forge', exit: 1 },
  { tool: 'snapshot_create', outcome: 'succeeded', summary: 'Snapshot before-integration-tests', host: 'atlas', vm: 'ubuntu-build-02' },
  { tool: 'discover_tools', outcome: 'succeeded', summary: 'Found 5 tools', host: 'studio' },
  { tool: 'run_command', outcome: 'refused', summary: 'Tool not available on host: docker', host: 'pebble' },
  { tool: 'vm_create', outcome: 'started', summary: 'Linked clone from ubuntu-24-04-arm64 v2', host: 'studio', vm: 'ubuntu-arm-03' },
  { tool: 'list_activity', outcome: 'succeeded', summary: 'Read 20 activity entries' },
  { tool: 'run_command', outcome: 'interrupted', summary: 'cargo build --release (cancelled)', host: 'atlas', exit: 130 },
  { tool: 'vm_stop', outcome: 'succeeded', summary: 'Stopped win11-test-04', host: 'forge', vm: 'win11-test-04' },
  { tool: 'run_command', outcome: 'rejected', summary: 'Arguments exceed the 128 item limit', host: 'atlas' },
  { tool: 'host_heartbeat', outcome: 'failed', summary: 'Heartbeat missed: pebble', host: 'pebble' },
];

const READ_TOOLS = new Set(['list_hosts', 'list_activity']);
export const isReadTool = (tool: string) => READ_TOOLS.has(tool);

export const FIXTURE_ACTIVITY: ActivityEntry[] = Array.from({ length: 64 }, (_, i) => {
  const id = 64 - i;
  const s = SCRIPT[i % SCRIPT.length];
  const actor = s.tool === 'host_heartbeat' ? ACTORS[3] : ACTORS[i % 3];
  const started = 7 * i + 3;
  return {
    id,
    tool: s.tool,
    outcome: s.outcome,
    actor,
    hostId: s.host ?? null,
    vmId: s.vm ?? null,
    summary: s.summary,
    exitCode: s.exit ?? null,
    startedAt: at(started),
    finishedAt: s.outcome === 'started' ? null : at(started - 1),
  };
});

export const TERMINAL_SCRIPTS: Record<string, { lines: string[]; exitCode: number; stderr?: string[] }> = {
  git: { lines: ['git version 2.43.0'], exitCode: 0 },
  node: { lines: ['v24.18.0'], exitCode: 0 },
  python3: { lines: ['Python 3.12.3'], exitCode: 0 },
  cargo: {
    lines: ['   Compiling arx-guest v0.1.0', '   Compiling arxd v0.1.0', '    Finished `dev` profile [unoptimized + debuginfo] target(s) in 41.07s'],
    exitCode: 0,
  },
  pwsh: { lines: ['PowerShell 7.5.4'], exitCode: 0 },
  swift: { lines: ['Swift version 6.2'], exitCode: 0, stderr: ['warning: using the stand-in toolchain'] },
};
