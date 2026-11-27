// Deterministic stand-in adapter for dev and test builds. It implements the
// same ArxApi contract as HttpApi so views cannot tell them apart, except via
// `kind`, which drives the always-visible "demo data" banner.
import { Emitter } from './emitter';
import {
  DEMO_CREDENTIALS,
  FIXTURE_ACTIVITY,
  FIXTURE_HOSTS,
  FIXTURE_NOW,
  FIXTURE_VMS,
  STANDIN_MARKER,
  TERMINAL_SCRIPTS,
  fixtureCpuMinute,
  isReadTool,
} from './fixtures';
import { activitySearch } from './http';
import {
  ApiError,
  type ActivityEntry,
  type ActivityPage,
  type ActivityQuery,
  type ArxApi,
  type ConsoleFrame,
  type ConsoleStatus,
  type ConsoleStream,
  type Credentials,
  CPU_HISTORY_RANGES,
  type CpuHistory,
  type CpuHistoryRange,
  type Host,
  type Session,
  type TerminalCommand,
  type TerminalEvent,
  type TerminalSession,
  type TerminalStatus,
  type Vm,
} from './types';

export type Schedule = (fn: () => void, ms: number) => () => void;

const defaultSchedule: Schedule = (fn, ms) => {
  const t = setTimeout(fn, ms);
  return () => clearTimeout(t);
};

export interface StandinOptions {
  /** Artificial latency so loading states are visible in dev. */
  latencyMs?: number;
  /** Delay between streamed terminal lines and console frames. */
  streamMs?: number;
  schedule?: Schedule;
  hosts?: Host[];
  vms?: Vm[];
  activity?: ActivityEntry[];
  /**
   * One minute's mean CPU for a host, `ago` minutes before the last complete
   * minute, or null when not sampled. Defaults to the fixture profiles.
   */
  cpuMinute?: (hostId: string, ago: number) => number | null;
  /** Force a method to reject, for error-state tests. */
  failures?: Partial<Record<'logout' | 'listHosts' | 'getHost' | 'getCpuHistory' | 'listCpuHistory' | 'listActivity' | 'listVms' | 'getVm' | 'openTerminal' | 'openConsole', ApiError>>;
}

const clone = <T,>(v: T): T => structuredClone(v);

/**
 * Builds a CPU history the way docs/design/night-orbit/CPU-HISTORY-API.md
 * says arxd does: buckets aligned in UTC, each the mean of the minutes present
 * (rounded to 0.1), and null when fewer than half of its minutes are present.
 */
export function standinCpuHistory(hostId: string, range: CpuHistoryRange, minute: (hostId: string, ago: number) => number | null): CpuHistory {
  const { bucketSeconds, points } = CPU_HISTORY_RANGES[range];
  const width = bucketSeconds * 1000;
  const lastMinuteEnd = Math.floor(FIXTURE_NOW / 60_000) * 60_000;
  const end = Math.ceil(lastMinuteEnd / width) * width;
  const from = end - points * width;
  const perBucket = bucketSeconds / 60;
  const samples: (number | null)[] = [];
  for (let i = 0; i < points; i += 1) {
    let sum = 0;
    let present = 0;
    for (let k = 0; k < perBucket; k += 1) {
      const start = from + i * width + k * 60_000;
      if (start >= lastMinuteEnd) break; // not yet sampled: the partial last bucket
      const value = minute(hostId, (lastMinuteEnd - start) / 60_000 - 1);
      if (value === null) continue;
      sum += value;
      present += 1;
    }
    samples.push(present * 2 < perBucket ? null : Math.round((sum / present) * 10) / 10);
  }
  return { hostId, range, bucketSeconds, from: new Date(from).toISOString(), to: new Date(end).toISOString(), samples };
}

export class StandinApi implements ArxApi {
  readonly kind = 'standin' as const;
  readonly marker = STANDIN_MARKER;
  private session: Session | null = null;
  /** Open streams, closed by the "server" when the session ends. */
  private readonly streams = new Set<{ serverClose(sessionEnded: boolean, message: string): void }>();
  private readonly opts: Required<Omit<StandinOptions, 'failures'>> & Pick<StandinOptions, 'failures'>;

  constructor(options: StandinOptions = {}) {
    this.opts = {
      latencyMs: options.latencyMs ?? 0,
      streamMs: options.streamMs ?? 0,
      schedule: options.schedule ?? defaultSchedule,
      hosts: options.hosts ?? FIXTURE_HOSTS,
      vms: options.vms ?? FIXTURE_VMS,
      activity: options.activity ?? FIXTURE_ACTIVITY,
      cpuMinute: options.cpuMinute ?? fixtureCpuMinute,
      failures: options.failures,
    };
  }

  private delay<T>(value: () => T): Promise<T> {
    return new Promise((resolve, reject) => {
      this.opts.schedule(() => {
        try {
          resolve(value());
        } catch (error) {
          reject(error);
        }
      }, this.opts.latencyMs);
    });
  }

  /**
   * Simulates the server revoking the session (logout, expiry, revocation):
   * the session is gone and every open stream is closed with the
   * session-ended code, as docs/dashboard-api.md requires of arxd.
   */
  endSession(message = 'Your session ended.'): void {
    this.session = null;
    for (const stream of [...this.streams]) stream.serverClose(true, message);
    this.streams.clear();
  }

  private track<T extends { serverClose(sessionEnded: boolean, message: string): void; onClosed(fn: () => void): void }>(stream: T): T {
    this.streams.add(stream);
    stream.onClosed(() => this.streams.delete(stream));
    return stream;
  }

  private guard(method: keyof NonNullable<StandinOptions['failures']>): void {
    if (!this.session) throw new ApiError('unauthorized', 'Your session has ended. Sign in again.', 401);
    const failure = this.opts.failures?.[method];
    if (failure) throw failure;
  }

  getSession(): Promise<Session | null> {
    return this.delay(() => (this.session ? clone(this.session) : null));
  }

  login(credentials: Credentials): Promise<Session> {
    return this.delay(() => {
      if (credentials.username !== DEMO_CREDENTIALS.username || credentials.password !== DEMO_CREDENTIALS.password) {
        throw new ApiError('unauthorized', 'Wrong username or password.', 401);
      }
      this.session = { user: { name: 'owner' }, csrfToken: 'standin-csrf-token-0001', expiresAt: new Date(FIXTURE_NOW + 12 * 3_600_000).toISOString() };
      return clone(this.session);
    });
  }

  logout(): Promise<void> {
    return this.delay(() => {
      const failure = this.opts.failures?.logout;
      if (failure) {
        this.opts.failures = { ...this.opts.failures, logout: undefined };
        throw failure; // fails once, so a retry can succeed
      }
      this.endSession('Signed out.');
    });
  }

  listHosts(): Promise<Host[]> {
    return this.delay(() => {
      this.guard('listHosts');
      return clone(this.opts.hosts);
    });
  }

  getHost(id: string): Promise<Host> {
    return this.delay(() => {
      this.guard('getHost');
      const host = this.opts.hosts.find(h => h.id === id);
      if (!host) throw new ApiError('not_found', `No host named ${id}.`, 404);
      return clone(host);
    });
  }

  getCpuHistory(hostId: string, range: CpuHistoryRange): Promise<CpuHistory> {
    return this.delay(() => {
      this.guard('getCpuHistory');
      if (!Object.hasOwn(CPU_HISTORY_RANGES, range)) throw new ApiError('invalid_request', 'Invalid CPU history range.');
      if (!this.opts.hosts.some(host => host.id === hostId)) throw new ApiError('not_found', `No host named ${hostId}.`, 404);
      return standinCpuHistory(hostId, range, this.opts.cpuMinute);
    });
  }

  listCpuHistory(range: '24h'): Promise<CpuHistory[]> {
    return this.delay(() => {
      this.guard('listCpuHistory');
      if (range !== '24h') throw new ApiError('invalid_request', 'The CPU history batch requires a 24h range.');
      return this.opts.hosts.map(host => standinCpuHistory(host.id, range, this.opts.cpuMinute));
    });
  }

  listActivity(query: ActivityQuery = {}): Promise<ActivityPage> {
    return this.delay(() => {
      this.guard('listActivity');
      activitySearch(query); // same validation as the HTTP adapter
      const limit = query.limit ?? 20;
      const matches = this.opts.activity.filter(e =>
        (query.beforeId === undefined || e.id < query.beforeId)
        && (query.includeReads || !isReadTool(e.tool))
        && (!query.outcome || e.outcome === query.outcome)
        && (!query.tool || e.tool === query.tool)
        && (!query.hostId || e.hostId === query.hostId));
      const entries = matches.slice(0, limit);
      return { entries: clone(entries), nextBeforeId: matches.length > limit ? entries[entries.length - 1].id : null };
    });
  }

  listVms(): Promise<Vm[]> {
    return this.delay(() => {
      this.guard('listVms');
      return clone(this.opts.vms);
    });
  }

  getVm(id: string): Promise<Vm> {
    return this.delay(() => {
      this.guard('getVm');
      const vm = this.opts.vms.find(v => v.id === id);
      if (!vm) throw new ApiError('not_found', `No VM named ${id}.`, 404);
      return clone(vm);
    });
  }

  openTerminal(hostId: string): Promise<TerminalSession> {
    return this.delay(() => {
      this.guard('openTerminal');
      const host = this.opts.hosts.find(h => h.id === hostId);
      if (!host) throw new ApiError('not_found', `No host named ${hostId}.`, 404);
      if (host.reachability === 'offline') throw new ApiError('unavailable', `${host.name} is offline.`, 503);
      return this.track(new StandinTerminal(host, this.opts.schedule, this.opts.streamMs));
    });
  }

  openConsole(vmId: string): Promise<ConsoleStream> {
    return this.delay(() => {
      this.guard('openConsole');
      const vm = this.opts.vms.find(v => v.id === vmId);
      if (!vm) throw new ApiError('not_found', `No VM named ${vmId}.`, 404);
      return this.track(new StandinConsole(vm, this.opts.schedule, Math.max(this.opts.streamMs, 1) * 20));
    });
  }
}

class StandinTerminal implements TerminalSession {
  private current: TerminalStatus = 'connecting';
  private readonly events = new Emitter<[TerminalEvent]>();
  private seq = 0;
  private cancelTimers: (() => void)[] = [];
  private activeId: string | null = null;
  private closedHook: (() => void) | null = null;

  constructor(private readonly host: Host, private readonly schedule: Schedule, private readonly streamMs: number) {
    // A 1 ms handshake: callers subscribe before the first event, as with a socket.
    this.cancelTimers.push(schedule(() => this.setStatus('ready'), 1));
  }

  get status(): TerminalStatus {
    return this.current;
  }

  private setStatus(status: TerminalStatus): void {
    if (this.current === status) return;
    this.current = status;
    this.events.emit({ type: 'status', status });
  }

  onEvent(listener: (event: TerminalEvent) => void): () => void {
    return this.events.on(listener);
  }

  run(command: TerminalCommand): void {
    if (this.current !== 'ready') throw new ApiError('invalid_request', 'The terminal is not ready for a command.');
    const commandId = `cmd-${++this.seq}`;
    this.activeId = commandId;
    this.setStatus('running');
    this.events.emit({ type: 'started', commandId, tool: command.tool, args: [...command.args] });
    const available = this.host.tools.some(t => t.name === command.tool);
    const script = TERMINAL_SCRIPTS[command.tool];
    const steps: (() => void)[] = [];
    if (!available) {
      steps.push(() => this.events.emit({ type: 'output', commandId, stream: 'stderr', data: `${command.tool}: not an available tool on ${this.host.name}` }));
      steps.push(() => this.finish(commandId, null, 'refused'));
    } else if (!script) {
      steps.push(() => this.events.emit({ type: 'output', commandId, stream: 'stdout', data: `${command.tool} ${command.args.join(' ')}`.trim() + ' (stand-in: no scripted output)' }));
      steps.push(() => this.finish(commandId, 0, 'succeeded'));
    } else {
      for (const line of script.lines) steps.push(() => this.events.emit({ type: 'output', commandId, stream: 'stdout', data: line }));
      for (const line of script.stderr ?? []) steps.push(() => this.events.emit({ type: 'output', commandId, stream: 'stderr', data: line }));
      steps.push(() => this.finish(commandId, script.exitCode, script.exitCode === 0 ? 'succeeded' : 'failed'));
    }
    steps.forEach((step, i) => this.cancelTimers.push(this.schedule(step, this.streamMs * (i + 1))));
  }

  private finish(commandId: string, exitCode: number | null, outcome: 'succeeded' | 'failed' | 'refused' | 'interrupted'): void {
    if (this.activeId !== commandId) return;
    this.activeId = null;
    this.events.emit({ type: 'exit', commandId, exitCode, outcome, truncated: false });
    this.setStatus('ready');
  }

  cancel(): void {
    if (!this.activeId) return;
    const id = this.activeId;
    this.cancelTimers.forEach(c => c());
    this.cancelTimers = [];
    this.finish(id, 130, 'interrupted');
  }

  onClosed(fn: () => void): void {
    this.closedHook = fn;
  }

  /** Server-initiated close; a running command is cancelled first. */
  serverClose(sessionEnded: boolean, message: string): void {
    if (this.current === 'closed') return;
    this.cancelTimers.forEach(c => c());
    this.cancelTimers = [];
    this.activeId = null;
    this.events.emit({ type: 'closed', reason: sessionEnded ? 'session_ended' : 'server', message: sessionEnded ? 'Your session ended, so the server closed the terminal.' : message });
    this.setStatus('closed');
    this.closedHook?.();
  }

  close(): void {
    this.cancelTimers.forEach(c => c());
    this.cancelTimers = [];
    this.activeId = null;
    this.setStatus('closed');
    this.events.clear();
    this.closedHook?.();
  }
}

const escapeXml = (s: string) => s.replace(/[<>&'"]/g, c => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', "'": '&apos;', '"': '&quot;' })[c]!);

export function consoleSvg(vm: Vm, seq: number): string {
  const lines = vm.guestOs === 'windows'
    ? ['Windows Boot Manager', `Starting ${vm.guestOsVersion}...`, '', `${vm.name} login screen ready`]
    : [`${vm.guestOsVersion} ${vm.name} ttyS0`, '', `${vm.name} login: _`, `[  ${(seq * 0.98 + 3.2).toFixed(2)}] arx-guest: ready`];
  const text = lines.map((l, i) => `<text x="16" y="${32 + i * 22}">${escapeXml(l)}</text>`).join('');
  return `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="400" viewBox="0 0 640 400"><rect width="640" height="400" fill="#0b0f14"/><g fill="#c8d3df" font-family="monospace" font-size="16">${text}</g><text x="624" y="388" fill="#5b6b7c" font-family="monospace" font-size="12" text-anchor="end">stand-in frame ${seq}</text></svg>`;
}

class StandinConsole implements ConsoleStream {
  private current: ConsoleStatus = 'connecting';
  private readonly frames = new Emitter<[ConsoleFrame]>();
  private readonly statuses = new Emitter<[ConsoleStatus, string | undefined, boolean]>();
  private seq = 0;
  private stop: (() => void) | null = null;
  private closedHook: (() => void) | null = null;

  constructor(private readonly vm: Vm, private readonly schedule: Schedule, private readonly intervalMs: number) {
    this.stop = schedule(() => this.start(), 1);
  }

  get status(): ConsoleStatus {
    return this.current;
  }

  private setStatus(status: ConsoleStatus, reason?: string, sessionEnded = false): void {
    if (this.current === status) return;
    this.current = status;
    this.statuses.emit(status, reason, sessionEnded);
  }

  private start(): void {
    if (this.vm.state !== 'running' && this.vm.state !== 'creating') {
      this.setStatus('closed', `${this.vm.name} is ${this.vm.state}; start it to see its console.`);
      this.closedHook?.();
      return;
    }
    this.setStatus('streaming');
    this.tick();
  }

  private tick(): void {
    this.seq += 1;
    const svg = consoleSvg(this.vm, this.seq);
    this.frames.emit({ seq: this.seq, width: 640, height: 400, mime: 'image/svg+xml', data: new Blob([svg], { type: 'image/svg+xml' }), capturedAt: new Date(FIXTURE_NOW + this.seq * 1000).toISOString() });
    this.stop = this.schedule(() => this.tick(), this.intervalMs);
  }

  onFrame(listener: (frame: ConsoleFrame) => void): () => void {
    return this.frames.on(listener);
  }

  onStatus(listener: (status: ConsoleStatus, reason?: string, sessionEnded?: boolean) => void): () => void {
    return this.statuses.on(listener);
  }

  onClosed(fn: () => void): void {
    this.closedHook = fn;
  }

  serverClose(sessionEnded: boolean, message: string): void {
    if (this.current === 'closed') return;
    this.stop?.();
    this.stop = null;
    this.setStatus('closed', sessionEnded ? 'Your session ended, so the server closed the console.' : message, sessionEnded);
    this.closedHook?.();
  }

  close(): void {
    this.stop?.();
    this.stop = null;
    this.setStatus('closed');
    this.frames.clear();
    this.statuses.clear();
    this.closedHook?.();
  }
}
