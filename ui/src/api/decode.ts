// Runtime decoders for server payloads. The server is trusted for meaning but
// not for shape: anything malformed or oversized becomes ApiError('bad_response')
// instead of reaching React as `undefined`.
import {
  ACTIVITY_OUTCOMES,
  ApiError,
  VM_STATES,
  type ActivityEntry,
  type ActivityPage,
  type Arch,
  type Host,
  type OsFamily,
  type Session,
  type Snapshot,
  type Vm,
} from './types';

type Obj = Record<string, unknown>;

export const LIMITS = { hosts: 256, vms: 1024, tools: 128, snapshots: 256, activity: 50, text: 512, id: 128 } as const;

const ID = /^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$/;
// RFC 3339 date-time: seconds are required, fractions optional, offset Z or ±hh:mm.
const RFC3339 = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(\.\d{1,9})?(Z|[+-](\d{2}):(\d{2}))$/;

function daysInMonth(year: number, month: number): number {
  return new Date(Date.UTC(year, month, 0)).getUTCDate();
}

/** Calendar-checked RFC 3339 instant (no Feb 30, hour 24 or missing seconds; leap seconds refused). */
function isRfc3339(value: string): boolean {
  const m = RFC3339.exec(value);
  if (!m) return false;
  const [year, month, day, hour, minute, second] = m.slice(1, 7).map(Number);
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > daysInMonth(year, month)) return false;
  if (hour > 23 || minute > 59 || second > 59) return false;
  if (m[9] !== undefined && (Number(m[9]) > 23 || Number(m[10]) > 59)) return false;
  return !Number.isNaN(Date.parse(value));
}

function fail(path: string): never {
  throw new ApiError('bad_response', `Unexpected server response at ${path}`);
}

function obj(v: unknown, path: string): Obj {
  if (typeof v !== 'object' || v === null || Array.isArray(v)) fail(path);
  return v as Obj;
}

function str(v: unknown, path: string, max: number = LIMITS.text): string {
  if (typeof v !== 'string' || v.length > max) fail(path);
  return v;
}

function id(v: unknown, path: string): string {
  const s = str(v, path, LIMITS.id);
  if (!ID.test(s)) fail(path);
  return s;
}

function iso(v: unknown, path: string): string {
  const s = str(v, path, 40);
  if (!isRfc3339(s)) fail(path);
  return s;
}

function num(v: unknown, path: string, min = 0, max = Number.MAX_SAFE_INTEGER): number {
  if (typeof v !== 'number' || !Number.isFinite(v) || v < min || v > max) fail(path);
  return v;
}

function int(v: unknown, path: string, min = 0, max = Number.MAX_SAFE_INTEGER): number {
  const n = num(v, path, min, max);
  if (!Number.isInteger(n)) fail(path);
  return n;
}

function bool(v: unknown, path: string): boolean {
  if (typeof v !== 'boolean') fail(path);
  return v;
}

function oneOf<T extends string>(v: unknown, allowed: readonly T[], path: string): T {
  if (typeof v !== 'string' || !(allowed as readonly string[]).includes(v)) fail(path);
  return v as T;
}

function nullable<T>(v: unknown, decode: (v: unknown) => T): T | null {
  return v === null || v === undefined ? null : decode(v);
}

function arr<T>(v: unknown, path: string, max: number, decode: (v: unknown, path: string) => T): T[] {
  if (!Array.isArray(v) || v.length > max) fail(path);
  return v.map((item, i) => decode(item, `${path}[${i}]`));
}

const ARCHES: readonly Arch[] = ['x86_64', 'arm64'];
const OSES: readonly OsFamily[] = ['windows', 'linux'];

export function decodeSession(v: unknown): Session {
  const o = obj(v, 'session');
  const user = obj(o.user, 'session.user');
  const csrfToken = str(o.csrfToken, 'session.csrfToken', 256);
  if (csrfToken.length < 16) fail('session.csrfToken');
  return { user: { name: str(user.name, 'session.user.name', 64) }, csrfToken, expiresAt: iso(o.expiresAt, 'session.expiresAt') };
}

export function decodeHost(v: unknown, path = 'host'): Host {
  const o = obj(v, path);
  const cpu = obj(o.cpu, `${path}.cpu`);
  const memory = obj(o.memory, `${path}.memory`);
  const disk = obj(o.disk, `${path}.disk`);
  return {
    id: id(o.id, `${path}.id`),
    name: str(o.name, `${path}.name`, 128),
    os: oneOf(o.os, OSES, `${path}.os`),
    osVersion: str(o.osVersion, `${path}.osVersion`, 128),
    arch: oneOf(o.arch, ARCHES, `${path}.arch`),
    cpu: {
      model: str(cpu.model, `${path}.cpu.model`, 128),
      cores: int(cpu.cores, `${path}.cpu.cores`, 1, 4096),
      usedPercent: nullable(cpu.usedPercent, x => num(x, `${path}.cpu.usedPercent`, 0, 100)),
    },
    memory: {
      totalBytes: int(memory.totalBytes, `${path}.memory.totalBytes`),
      usedBytes: nullable(memory.usedBytes, x => int(x, `${path}.memory.usedBytes`)),
    },
    disk: {
      totalBytes: int(disk.totalBytes, `${path}.disk.totalBytes`),
      freeBytes: int(disk.freeBytes, `${path}.disk.freeBytes`),
      capBytes: nullable(disk.capBytes, x => int(x, `${path}.disk.capBytes`)),
    },
    tools: arr(o.tools, `${path}.tools`, LIMITS.tools, (t, p) => {
      const to = obj(t, p);
      return { name: str(to.name, `${p}.name`, 64), version: nullable(to.version, x => str(x, `${p}.version`, 64)) };
    }),
    reachability: oneOf(o.reachability, ['online', 'degraded', 'offline'] as const, `${path}.reachability`),
    reachabilityReason: nullable(o.reachabilityReason, x => str(x, `${path}.reachabilityReason`)),
    lastSeenAt: iso(o.lastSeenAt, `${path}.lastSeenAt`),
    mode: oneOf(o.mode, ['always_on', 'opportunistic'] as const, `${path}.mode`),
    agentRights: oneOf(o.agentRights, ['standard', 'admin'] as const, `${path}.agentRights`),
  };
}

export function decodeHosts(v: unknown): Host[] {
  return arr(obj(v, 'response').hosts, 'hosts', LIMITS.hosts, decodeHost);
}

export function decodeActivityEntry(v: unknown, path = 'entry'): ActivityEntry {
  const o = obj(v, path);
  const actor = obj(o.actor, `${path}.actor`);
  return {
    id: int(o.id, `${path}.id`, 1),
    tool: str(o.tool, `${path}.tool`, 64),
    outcome: oneOf(o.outcome, ACTIVITY_OUTCOMES, `${path}.outcome`),
    actor: { kind: oneOf(actor.kind, ['agent', 'owner', 'system'] as const, `${path}.actor.kind`), name: str(actor.name, `${path}.actor.name`, 64) },
    hostId: nullable(o.hostId, x => id(x, `${path}.hostId`)),
    vmId: nullable(o.vmId, x => id(x, `${path}.vmId`)),
    summary: str(o.summary, `${path}.summary`),
    exitCode: nullable(o.exitCode, x => int(x, `${path}.exitCode`, -2147483648, 4294967295)),
    startedAt: nullable(o.startedAt, x => iso(x, `${path}.startedAt`)),
    finishedAt: nullable(o.finishedAt, x => iso(x, `${path}.finishedAt`)),
  };
}

export function decodeActivityPage(v: unknown): ActivityPage {
  const o = obj(v, 'activity');
  return {
    entries: arr(o.entries, 'activity.entries', LIMITS.activity, decodeActivityEntry),
    nextBeforeId: nullable(o.nextBeforeId, x => int(x, 'activity.nextBeforeId', 1)),
  };
}

function decodeSnapshot(v: unknown, path: string): Snapshot {
  const o = obj(v, path);
  return {
    id: id(o.id, `${path}.id`),
    name: str(o.name, `${path}.name`, 128),
    createdAt: nullable(o.createdAt, x => iso(x, `${path}.createdAt`)),
    sizeBytes: nullable(o.sizeBytes, x => int(x, `${path}.sizeBytes`)),
  };
}

export function decodeVm(v: unknown, path = 'vm'): Vm {
  const o = obj(v, path);
  const template = obj(o.template, `${path}.template`);
  const kind = oneOf(o.kind, ['test', 'static'] as const, `${path}.kind`);
  const expiresAt = nullable(o.expiresAt, x => iso(x, `${path}.expiresAt`));
  if (kind === 'test' && expiresAt === null) fail(`${path}.expiresAt`);
  return {
    id: id(o.id, `${path}.id`),
    name: str(o.name, `${path}.name`, 128),
    kind,
    template: { id: id(template.id, `${path}.template.id`), version: int(template.version, `${path}.template.version`, 1) },
    guestOs: oneOf(o.guestOs, OSES, `${path}.guestOs`),
    guestOsVersion: str(o.guestOsVersion, `${path}.guestOsVersion`, 128),
    arch: oneOf(o.arch, ARCHES, `${path}.arch`),
    hostId: id(o.hostId, `${path}.hostId`),
    state: oneOf(o.state, VM_STATES, `${path}.state`),
    ready: bool(o.ready, `${path}.ready`),
    expiresAt,
    cpus: nullable(o.cpus, x => int(x, `${path}.cpus`, 1, 1024)),
    memoryBytes: nullable(o.memoryBytes, x => int(x, `${path}.memoryBytes`)),
    diskBytes: int(o.diskBytes, `${path}.diskBytes`),
    snapshots: arr(o.snapshots, `${path}.snapshots`, LIMITS.snapshots, decodeSnapshot),
  };
}

export function decodeVms(v: unknown): Vm[] {
  return arr(obj(v, 'response').vms, 'vms', LIMITS.vms, decodeVm);
}

/** Strict RFC 3339 instant check, as used for every timestamp the server sends. */
export function isIsoInstant(value: unknown): value is string {
  return typeof value === 'string' && value.length <= 40 && isRfc3339(value);
}

export function isValidId(value: string): boolean {
  return ID.test(value);
}
