// Pure view logic for the Night Orbit views: the hosts board, the search jump
// list and the console's closed state. Everything is computed from API data
// only (types.ts); a value the API sends as null stays null so the view can
// draw it hatched instead of as 0.
import { ACTIVITY_OUTCOMES, type ActivityEntry, type ActivityOutcome, type Host, type Vm, type VmKind } from '../api/types';
import type { Route } from './router';

// --- Hosts board -------------------------------------------------------------

/** What the VMs on one host ask of it. `null` means every VM's size is unknown. */
export interface Allocation {
  cpus: number | null;
  memoryBytes: number | null;
  diskBytes: number;
  /** VMs whose vCPU or memory is null: their share is missing from the sums. */
  unknown: number;
}

export function allocation(vms: readonly Vm[]): Allocation {
  const sum = (values: (number | null)[]) => {
    const known = values.filter((v): v is number => v !== null);
    // No VMs is a real 0; VMs that all report null are unknown, never 0.
    return values.length > 0 && known.length === 0 ? null : known.reduce((a, b) => a + b, 0);
  };
  return {
    cpus: sum(vms.map(v => v.cpus)),
    memoryBytes: sum(vms.map(v => v.memoryBytes)),
    diskBytes: vms.reduce((a, v) => a + v.diskBytes, 0),
    unknown: vms.filter(v => v.cpus === null || v.memoryBytes === null).length,
  };
}

export interface BoardColumn {
  host: Host;
  /** The host's VMs after the kind filter, in API order. */
  vms: Vm[];
  /** Allocation of every VM on the host, whatever the filter shows. */
  allocated: Allocation;
}

export interface Board {
  columns: BoardColumn[];
  /** VMs whose host is not in the host list (for example a host that left). */
  orphans: Vm[];
}

/** One column per host, in API order, holding that host's VMs. */
export function groupBoard(hosts: readonly Host[], vms: readonly Vm[], kind: VmKind | 'all' = 'all'): Board {
  const byHost = new Map<string, Vm[]>(hosts.map(h => [h.id, []]));
  const orphans: Vm[] = [];
  for (const vm of vms) (byHost.get(vm.hostId) ?? orphans).push(vm);
  const shown = (list: Vm[]) => (kind === 'all' ? list : list.filter(v => v.kind === kind));
  return {
    columns: hosts.map(host => {
      const all = byHost.get(host.id)!;
      return { host, vms: shown(all), allocated: allocation(all) };
    }),
    orphans: shown(orphans),
  };
}

export interface BoardTotals {
  hosts: number;
  online: number;
  degraded: number;
  offline: number;
  vms: number;
  running: number;
  test: number;
  static: number;
}

/** Header totals for the board: counts over every host and VM the API returned. */
export function boardTotals(hosts: readonly Host[], vms: readonly Vm[]): BoardTotals {
  const count = <T,>(list: readonly T[], test: (item: T) => boolean) => list.filter(test).length;
  return {
    hosts: hosts.length,
    online: count(hosts, h => h.reachability === 'online'),
    degraded: count(hosts, h => h.reachability === 'degraded'),
    offline: count(hosts, h => h.reachability === 'offline'),
    vms: vms.length,
    running: count(vms, v => v.state === 'running'),
    test: count(vms, v => v.kind === 'test'),
    static: count(vms, v => v.kind === 'static'),
  };
}

// --- Search jump list -------------------------------------------------------

export type JumpKind = 'host' | 'test VM' | 'static VM' | 'tool';

export interface JumpItem {
  key: string;
  label: string;
  kind: JumpKind;
  route: Route;
}

export const JUMP_LIMIT = 8;

/**
 * Hosts, VMs and tools whose name (or id) contains the query, case-insensitive,
 * in that order and at most `limit`. A tool jumps to the terminal on its host;
 * offline hosts can't run commands, so their tools are left out.
 */
export function jumpItems(query: string, hosts: readonly Host[], vms: readonly Vm[], limit = JUMP_LIMIT): JumpItem[] {
  const q = query.trim().toLowerCase();
  if (q === '') return [];
  const hit = (...values: string[]) => values.some(v => v.toLowerCase().includes(q));
  const items: JumpItem[] = [
    ...hosts.filter(h => hit(h.name, h.id)).map(h => ({ key: `h:${h.id}`, label: h.name, kind: 'host' as const, route: { name: 'host', id: h.id } as Route })),
    ...vms.filter(v => hit(v.name, v.id)).map(v => ({ key: `v:${v.id}`, label: v.name, kind: `${v.kind} VM` as JumpKind, route: { name: 'vm', id: v.id } as Route })),
    ...hosts.filter(h => h.reachability !== 'offline').flatMap(h => h.tools.filter(t => hit(t.name)).map(t => ({
      key: `t:${h.id}:${t.name}`, label: `${t.name} on ${h.name}`, kind: 'tool' as const, route: { name: 'terminal', hostId: h.id } as Route,
    }))),
  ];
  return items.slice(0, limit);
}

// --- Console ----------------------------------------------------------------

/**
 * Why the console opens straight to "closed" without asking the server, or
 * null when it should connect. An offline host comes first: nothing on it can
 * answer. A failed or stopped VM has no picture to show.
 */
export function consoleClosedReason(vm: Vm, host: Host | undefined): string | null {
  if (host?.reachability === 'offline') {
    return `${host.name} is offline${host.reachabilityReason ? ` (${host.reachabilityReason})` : ''}, so its VMs have no console.`;
  }
  if (vm.state === 'failed') return `${vm.name} failed, so there is no picture to show.`;
  if (vm.state === 'stopped') return `${vm.name} is stopped; start it to see its console.`;
  return null;
}

// --- Activity -----------------------------------------------------------------

/** How many of the loaded rows ended in each outcome, in `ACTIVITY_OUTCOMES` order. */
export function outcomeCounts(entries: readonly ActivityEntry[]): { outcome: ActivityOutcome; count: number }[] {
  return ACTIVITY_OUTCOMES.map(outcome => ({ outcome, count: entries.filter(e => e.outcome === outcome).length }));
}
