import { describe, expect, test } from 'vitest';
import { FIXTURE_ACTIVITY, FIXTURE_HOSTS, FIXTURE_VMS } from '../api/fixtures';
import type { Host, Vm } from '../api/types';
import { JUMP_LIMIT, allocation, boardTotals, consoleClosedReason, consoleGate, groupBoard, jumpItems, outcomeCounts } from './derive';

const GiB = 1024 ** 3;
const host = (id: string, patch: Partial<Host> = {}): Host => ({ ...FIXTURE_HOSTS[0], id, name: id, ...patch });
const vm = (id: string, hostId: string, patch: Partial<Vm> = {}): Vm => ({ ...FIXTURE_VMS[0], id, name: id, hostId, ...patch });

describe('board grouping', () => {
  test('one column per host in API order, each holding only its own VMs in API order', () => {
    const hosts = [host('b'), host('a'), host('c')];
    const vms = [vm('v1', 'a'), vm('v2', 'b'), vm('v3', 'a'), vm('v4', 'c')];
    const board = groupBoard(hosts, vms);
    expect(board.columns.map(c => [c.host.id, c.vms.map(v => v.id)])).toEqual([['b', ['v2']], ['a', ['v1', 'v3']], ['c', ['v4']]]);
    expect(board.orphans).toEqual([]);
  });

  test('a host with no VMs still gets a column, and VMs on an unknown host are kept as orphans', () => {
    const board = groupBoard([host('a'), host('empty')], [vm('v1', 'a'), vm('lost', 'gone')]);
    expect(board.columns.map(c => c.vms.length)).toEqual([1, 0]);
    expect(board.orphans.map(v => v.id)).toEqual(['lost']);
  });

  test('the kind filter narrows the cards but never the allocation', () => {
    const vms = [vm('t', 'a', { kind: 'test', cpus: 2 }), vm('s', 'a', { kind: 'static', cpus: 4, expiresAt: null })];
    const board = groupBoard([host('a')], vms, 'static');
    expect(board.columns[0].vms.map(v => v.id)).toEqual(['s']);
    expect(board.columns[0].allocated.cpus).toBe(6);
    expect(groupBoard([host('a')], vms, 'test').columns[0].vms.map(v => v.id)).toEqual(['t']);
  });

  test('the fixture board puts every VM in its host column', () => {
    const board = groupBoard(FIXTURE_HOSTS, FIXTURE_VMS);
    expect(board.columns.map(c => [c.host.id, c.vms.length])).toEqual([['atlas', 3], ['harbor', 1], ['forge', 1], ['pebble', 0]]);
  });
});

describe('allocation', () => {
  test('sums vCPU, memory and disk of the VMs on a host', () => {
    const a = allocation([vm('a', 'h', { cpus: 2, memoryBytes: 4 * GiB, diskBytes: 20 * GiB }), vm('b', 'h', { cpus: 4, memoryBytes: 8 * GiB, diskBytes: 40 * GiB })]);
    expect(a).toEqual({ cpus: 6, memoryBytes: 12 * GiB, diskBytes: 60 * GiB, unknown: 0 });
  });

  test('no VMs is a real zero', () => {
    expect(allocation([])).toEqual({ cpus: 0, memoryBytes: 0, diskBytes: 0, unknown: 0 });
  });

  test('VMs that all report null size are unknown, never 0', () => {
    const a = allocation([vm('a', 'h', { cpus: null, memoryBytes: null }), vm('b', 'h', { cpus: null, memoryBytes: null })]);
    expect(a.cpus).toBeNull();
    expect(a.memoryBytes).toBeNull();
    expect(a.unknown).toBe(2);
  });

  test('a mix counts the known sizes and reports how many are missing', () => {
    const a = allocation([vm('a', 'h', { cpus: 2, memoryBytes: null }), vm('b', 'h', { cpus: null, memoryBytes: 2 * GiB }), vm('c', 'h', { cpus: 1, memoryBytes: GiB })]);
    expect(a).toMatchObject({ cpus: 3, memoryBytes: 3 * GiB, unknown: 2 });
  });
});

describe('board totals', () => {
  test('count hosts by reachability and VMs by state and kind', () => {
    expect(boardTotals(FIXTURE_HOSTS, FIXTURE_VMS)).toEqual({ hosts: 4, online: 2, degraded: 1, offline: 1, vms: 5, running: 3, test: 4, static: 1 });
  });

  test('an empty cluster totals zero everywhere', () => {
    expect(boardTotals([], [])).toEqual({ hosts: 0, online: 0, degraded: 0, offline: 0, vms: 0, running: 0, test: 0, static: 0 });
  });

  test('VMs count even when their host is not in the host list', () => {
    expect(boardTotals([host('a')], [vm('v1', 'a'), vm('lost', 'gone')])).toMatchObject({ hosts: 1, vms: 2, test: 2 });
  });

  test('only running counts as running', () => {
    const states = (['creating', 'running', 'stopped', 'snapshotting', 'migrating', 'failed', 'destroying'] as const).map((state, i) => vm(`v${i}`, 'a', { state }));
    expect(boardTotals([], states).running).toBe(1);
  });
});

describe('search jump list', () => {
  test('an empty or blank query lists nothing', () => {
    expect(jumpItems('', FIXTURE_HOSTS, FIXTURE_VMS)).toEqual([]);
    expect(jumpItems('   ', FIXTURE_HOSTS, FIXTURE_VMS)).toEqual([]);
  });

  test('matches hosts, then VMs, then tools, case-insensitively, with their routes', () => {
    const items = jumpItems('  AtLaS ', FIXTURE_HOSTS, FIXTURE_VMS);
    expect(items).toEqual([{ key: 'h:atlas', label: 'atlas', kind: 'host', route: { name: 'host', id: 'atlas' } }]);
    const ubuntu = jumpItems('ubuntu', FIXTURE_HOSTS, FIXTURE_VMS);
    expect(ubuntu.map(i => [i.label, i.kind])).toEqual([['ubuntu-build-02', 'test VM'], ['ubuntu-arm-03', 'test VM']]);
    expect(jumpItems('web', FIXTURE_HOSTS, FIXTURE_VMS)[0]).toMatchObject({ label: 'web-static', kind: 'static VM', route: { name: 'vm', id: 'web-static' } });
  });

  test('a tool jumps to the terminal on each reachable host that has it, never an offline one', () => {
    const items = jumpItems('python3', FIXTURE_HOSTS, FIXTURE_VMS);
    // pebble also has python3 but is offline.
    expect(items.map(i => [i.label, i.route])).toEqual([
      ['python3 on atlas', { name: 'terminal', hostId: 'atlas' }],
      ['python3 on harbor', { name: 'terminal', hostId: 'harbor' }],
    ]);
    expect(items.every(i => i.kind === 'tool')).toBe(true);
  });

  test('a VM matches by id as well as by name', () => {
    const items = jumpItems('id-7', FIXTURE_HOSTS, [vm('id-7f', 'atlas', { name: 'friendly' })]);
    expect(items.map(i => i.label)).toEqual(['friendly']);
  });

  test('lists at most the limit, keeping the order', () => {
    const many = Array.from({ length: 12 }, (_, i) => vm(`box-${i}`, 'atlas'));
    const items = jumpItems('box', FIXTURE_HOSTS, many);
    expect(items).toHaveLength(JUMP_LIMIT);
    expect(items[0].label).toBe('box-0');
    expect(jumpItems('box', FIXTURE_HOSTS, many, 3).map(i => i.label)).toEqual(['box-0', 'box-1', 'box-2']);
  });

  test('no match lists nothing', () => {
    expect(jumpItems('zzz-nothing', FIXTURE_HOSTS, FIXTURE_VMS)).toEqual([]);
  });
});

describe('console closed state', () => {
  const online = host('h1', { reachability: 'online', reachabilityReason: null });
  const offline = host('h2', { reachability: 'offline', reachabilityReason: 'No heartbeat for 2 hours' });

  test('a running VM on a reachable host connects', () => {
    expect(consoleClosedReason(vm('v', 'h1', { state: 'running' }), online)).toBeNull();
    expect(consoleClosedReason(vm('v', 'h1', { state: 'running' }), host('h3', { reachability: 'degraded', reachabilityReason: 'busy' }))).toBeNull();
  });

  test('busy states still connect: creating, snapshotting, migrating, destroying', () => {
    for (const state of ['creating', 'snapshotting', 'migrating', 'destroying'] as const) {
      expect(consoleClosedReason(vm('v', 'h1', { state }), online), state).toBeNull();
    }
  });

  test('a stopped or failed VM is closed with its reason', () => {
    expect(consoleClosedReason(vm('box', 'h1', { state: 'stopped' }), online)).toBe('box is stopped; start it to see its console.');
    expect(consoleClosedReason(vm('box', 'h1', { state: 'failed' }), online)).toBe('box failed, so there is no picture to show.');
  });

  test('a VM on an offline host is closed with the host reason, before its own state', () => {
    expect(consoleClosedReason(vm('box', 'h2', { state: 'running' }), offline)).toBe('h2 is offline (No heartbeat for 2 hours), so its VMs have no console.');
    expect(consoleClosedReason(vm('box', 'h2', { state: 'stopped' }), offline)).toContain('h2 is offline');
    expect(consoleClosedReason(vm('box', 'h2'), { ...offline, reachabilityReason: null })).toBe('h2 is offline, so its VMs have no console.');
  });

  test('an unknown host does not close a running VM', () => {
    expect(consoleClosedReason(vm('box', 'gone', { state: 'running' }), undefined)).toBeNull();
  });
});

describe('outcome counts', () => {
  test('counts each outcome in the loaded rows, in the fixed order', () => {
    const counts = outcomeCounts(FIXTURE_ACTIVITY.slice(0, 12));
    expect(counts.map(c => c.outcome)).toEqual(['started', 'succeeded', 'failed', 'rejected', 'refused', 'interrupted']);
    expect(counts.reduce((a, c) => a + c.count, 0)).toBe(12);
    expect(Object.fromEntries(counts.map(c => [c.outcome, c.count]))).toEqual({ started: 1, succeeded: 6, failed: 2, rejected: 1, refused: 1, interrupted: 1 });
  });
});

describe('console gate (when a live console may ask for a ticket)', () => {
  const online = host('h1', { reachability: 'online', reachabilityReason: null });
  const offline = host('h2', { reachability: 'offline', reachabilityReason: 'gone' });

  test('a stopped or failed VM is closed at once, whatever the host list is doing', () => {
    for (const state of ['stopped', 'failed'] as const) {
      for (const hostState of [{ status: 'pending' }, { status: 'unavailable' }, { status: 'ready', host: online }] as const) {
        expect(consoleGate(vm('v', 'h1', { state }), hostState).status, `${state} ${hostState.status}`).toBe('closed');
      }
    }
  });

  test('while the host list loads, a running VM waits', () => {
    expect(consoleGate(vm('v', 'h1'), { status: 'pending' })).toEqual({ status: 'checking' });
  });

  test('an offline host closes with its reason; a reachable or unknown host, or a failed list, connects', () => {
    expect(consoleGate(vm('box', 'h2'), { status: 'ready', host: offline })).toEqual({ status: 'closed', reason: 'h2 is offline (gone), so its VMs have no console.' });
    expect(consoleGate(vm('v', 'h1'), { status: 'ready', host: online })).toEqual({ status: 'connect' });
    expect(consoleGate(vm('v', 'gone'), { status: 'ready', host: undefined })).toEqual({ status: 'connect' });
    expect(consoleGate(vm('v', 'h1'), { status: 'unavailable' })).toEqual({ status: 'connect' });
  });
});
