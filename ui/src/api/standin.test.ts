import { describe, expect, test } from 'vitest';
import { DEMO_CREDENTIALS, FIXTURE_ACTIVITY, FIXTURE_VMS, isReadTool } from './fixtures';
import { consoleSvg, StandinApi } from './standin';
import { manualClock } from './testing';
import { ApiError, type ConsoleFrame, type ConsoleStatus, type TerminalEvent } from './types';

async function signedIn(options: ConstructorParameters<typeof StandinApi>[0] = {}) {
  const api = new StandinApi(options);
  await api.login(DEMO_CREDENTIALS);
  return api;
}

describe('stand-in session', () => {
  test('data calls need a session, wrong passwords are refused, logout ends it', async () => {
    const api = new StandinApi();
    expect(await api.getSession()).toBeNull();
    await expect(api.listHosts()).rejects.toMatchObject({ code: 'unauthorized' });
    await expect(api.login({ username: 'owner', password: 'nope' })).rejects.toMatchObject({ code: 'unauthorized' });
    const session = await api.login(DEMO_CREDENTIALS);
    expect(session.user.name).toBe('owner');
    expect(await api.getSession()).toEqual(session);
    expect(await api.listHosts()).toHaveLength(4);
    await api.logout();
    await expect(api.listVms()).rejects.toMatchObject({ code: 'unauthorized' });
  });

  test('returned data are copies, so callers cannot mutate the fixtures', async () => {
    const api = await signedIn();
    const [first] = await api.listHosts();
    first.name = 'mutated';
    expect((await api.getHost('atlas')).name).toBe('atlas');
  });

  test('unknown ids are not_found and injected failures surface', async () => {
    const api = await signedIn({ failures: { listVms: new ApiError('unavailable', 'down', 503) } });
    await expect(api.getHost('nope')).rejects.toMatchObject({ code: 'not_found' });
    await expect(api.listVms()).rejects.toMatchObject({ code: 'unavailable' });
  });
});

describe('stand-in CPU history', () => {
  test('is session-bound and answers for its host inventory only', async () => {
    await expect(new StandinApi().listCpuHistory('24h')).rejects.toMatchObject({ code: 'unauthorized' });
    const api = await signedIn();
    await expect(api.getCpuHistory('missing', '1h')).rejects.toMatchObject({ code: 'not_found' });
    const batch = await api.listCpuHistory('24h');
    expect(batch.map(series => series.hostId)).toEqual((await api.listHosts()).map(host => host.id));
    expect(batch.every(series => series.range === '24h' && series.bucketSeconds === 600 && series.samples.length === 144)).toBe(true);
  });

  test('buckets are UTC-aligned and sized per range, with values from 0 to 100 at one decimal', async () => {
    const api = await signedIn();
    for (const [range, bucketSeconds, points] of [['1h', 60, 60], ['6h', 300, 72], ['24h', 600, 144]] as const) {
      const history = await api.getCpuHistory('harbor', range);
      expect(history).toMatchObject({ hostId: 'harbor', range, bucketSeconds });
      expect(history.samples).toHaveLength(points);
      expect(Date.parse(history.from) % (bucketSeconds * 1000)).toBe(0);
      expect(Date.parse(history.to) - Date.parse(history.from)).toBe(points * bucketSeconds * 1000);
      // harbor has no gaps: every bucket is sampled.
      for (const sample of history.samples) {
        expect(sample).not.toBeNull();
        expect(sample! >= 0 && sample! <= 100 && Math.round(sample! * 10) === sample! * 10).toBe(true);
      }
    }
  });

  test('missing minutes stay null (never 0): atlas has restart gaps, pebble has no buffer at all', async () => {
    const api = await signedIn();
    const atlas1h = await api.getCpuHistory('atlas', '1h');
    // Minutes 18 to 23 ago are missing: the six 1-minute buckets before the last 18.
    expect(atlas1h.samples.slice(60 - 24, 60 - 18)).toEqual(Array<null>(6).fill(null));
    expect(atlas1h.samples.filter(s => s === null)).toHaveLength(6);
    const atlas24h = await api.getCpuHistory('atlas', '24h');
    expect(atlas24h.samples.some(s => s === null)).toBe(true);
    expect(atlas24h.samples.some(s => s !== null)).toBe(true);
    expect(atlas24h.samples).not.toContain(0);
    const pebble = await api.getCpuHistory('pebble', '6h');
    expect(pebble.samples).toEqual(Array<null>(72).fill(null));
  });

  test('a bucket with fewer than half of its minutes present is null; otherwise it is the mean of those present', async () => {
    // Only every third minute sampled: 2 of 5 per 5-minute bucket is under half.
    const sparse = await signedIn({ cpuMinute: (_h, ago) => (ago % 3 === 0 ? 50 : null) });
    expect((await sparse.getCpuHistory('atlas', '6h')).samples.every(s => s === null)).toBe(true);
    // Three of five present at 10, 20, 30 averages to 20.
    const half = await signedIn({ cpuMinute: (_h, ago) => ([10, 20, 30][ago % 5] ?? null) });
    expect((await half.getCpuHistory('atlas', '6h')).samples.every(s => s === 20)).toBe(true);
  });
});

describe('stand-in activity', () => {
  test('pages walk the whole non-read feed exactly once, newest first', async () => {
    const api = await signedIn();
    const seen: number[] = [];
    let beforeId: number | undefined;
    for (let i = 0; i < 10; i += 1) {
      const page = await api.listActivity({ limit: 20, beforeId });
      seen.push(...page.entries.map(e => e.id));
      if (page.nextBeforeId === null) break;
      beforeId = page.nextBeforeId;
    }
    const expected = FIXTURE_ACTIVITY.filter(e => !isReadTool(e.tool)).map(e => e.id);
    expect(seen).toEqual(expected);
    expect([...seen].sort((a, b) => b - a)).toEqual(seen);
  });

  test('filters combine and include_reads adds read-only tools', async () => {
    const api = await signedIn();
    const failedOnForge = await api.listActivity({ outcome: 'failed', hostId: 'forge', limit: 50 });
    expect(failedOnForge.entries.length).toBeGreaterThan(0);
    expect(failedOnForge.entries.every(e => e.outcome === 'failed' && e.hostId === 'forge')).toBe(true);
    expect((await api.listActivity({ tool: 'list_hosts' })).entries).toHaveLength(0);
    expect((await api.listActivity({ tool: 'list_hosts', includeReads: true })).entries.length).toBeGreaterThan(0);
    await expect(api.listActivity({ limit: 0 })).rejects.toMatchObject({ code: 'invalid_request' });
  });
});

describe('stand-in terminal', () => {
  async function terminal(hostId: string) {
    const clock = manualClock();
    const api = new StandinApi({ schedule: clock.schedule, streamMs: 10 });
    const login = api.login(DEMO_CREDENTIALS);
    clock.advance(0);
    await login;
    const opening = api.openTerminal(hostId);
    clock.advance(0);
    const session = await opening;
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    clock.advance(1);
    return { session, events, clock, api };
  }

  test('a scripted tool streams output then exits succeeded, returning to ready', async () => {
    const { session, events, clock } = await terminal('atlas');
    expect(session.status).toBe('ready');
    session.run({ tool: 'cargo', args: ['build'] });
    expect(session.status).toBe('running');
    clock.advance(100);
    const output = events.filter(e => e.type === 'output').map(e => (e as { data: string }).data);
    expect(output).toHaveLength(3);
    expect(output[2]).toContain('Finished');
    expect(events.at(-2)).toMatchObject({ type: 'exit', exitCode: 0, outcome: 'succeeded' });
    expect(session.status).toBe('ready');
  });

  test('a tool the host lacks is refused, and cancel interrupts a running command', async () => {
    const { session, events, clock } = await terminal('harbor');
    session.run({ tool: 'pwsh', args: [] });
    clock.advance(100);
    expect(events.find(e => e.type === 'exit')).toMatchObject({ outcome: 'refused', exitCode: null });
    session.run({ tool: 'cargo', args: ['build'] });
    clock.advance(10);
    session.cancel();
    clock.advance(100);
    const exits = events.filter(e => e.type === 'exit');
    expect(exits.at(-1)).toMatchObject({ outcome: 'interrupted', exitCode: 130 });
    expect(events.filter(e => e.type === 'output' && e.commandId === 'cmd-2')).toHaveLength(1);
    expect(session.status).toBe('ready');
  });

  test('running while busy is refused and an offline host cannot open a terminal', async () => {
    const { session, api, clock } = await terminal('atlas');
    session.run({ tool: 'git', args: ['--version'] });
    expect(() => session.run({ tool: 'git', args: [] })).toThrow(/not ready/);
    const opening = api.openTerminal('pebble');
    clock.advance(0);
    await expect(opening).rejects.toMatchObject({ code: 'unavailable' });
  });
});

describe('stand-in console', () => {
  test('a running VM streams numbered SVG frames until closed', async () => {
    const clock = manualClock();
    const api = new StandinApi({ schedule: clock.schedule, streamMs: 5 });
    const login = api.login(DEMO_CREDENTIALS);
    clock.advance(0);
    await login;
    const opening = api.openConsole('ubuntu-build-02');
    clock.advance(0);
    const stream = await opening;
    const frames: ConsoleFrame[] = [];
    stream.onFrame(f => frames.push(f));
    clock.advance(250);
    expect(stream.status).toBe('streaming');
    expect(frames.map(f => f.seq)).toEqual([1, 2, 3]);
    expect(frames[0].mime).toBe('image/svg+xml');
    stream.close();
    clock.advance(1000);
    expect(frames).toHaveLength(3);
    expect(clock.pending()).toBe(0);
  });

  test('a stopped VM closes with a reason instead of streaming', async () => {
    const clock = manualClock();
    const api = new StandinApi({ schedule: clock.schedule });
    const login = api.login(DEMO_CREDENTIALS);
    clock.advance(0);
    await login;
    const opening = api.openConsole('win11-test-04');
    clock.advance(0);
    const stream = await opening;
    const statuses: [ConsoleStatus, string | undefined][] = [];
    stream.onStatus((s, r) => statuses.push([s, r]));
    clock.advance(1);
    expect(statuses).toEqual([['closed', 'win11-test-04 is stopped; start it to see its console.']]);
  });

  test('frame text is XML-escaped', () => {
    const svg = consoleSvg({ ...FIXTURE_VMS[1], name: '<script>' }, 1);
    expect(svg).not.toContain('<script>');
    expect(svg).toContain('&lt;script&gt;');
  });
});
