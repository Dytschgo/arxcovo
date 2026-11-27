import { describe, expect, test } from 'vitest';
import { FIXTURE_HOSTS, FIXTURE_VMS } from './fixtures';
import { activitySearch, CSRF_HEADER, decodeFrameMeta, decodeTerminalMessage, HttpApi, readBounded } from './http';
import { ApiError, isApiError } from './types';
import type { ConsoleFrame, ConsoleStream, TerminalEvent, TerminalSession } from './types';

const SESSION = { user: { name: 'owner' }, csrfToken: 'csrf-token-0123456789', expiresAt: '2026-11-23T20:00:00Z' };

type Call = { url: string; init: RequestInit };

function fakeFetch(routes: Record<string, (init: RequestInit) => Response | Promise<Response>>) {
  const calls: Call[] = [];
  const fetchImpl = (async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = String(input);
    calls.push({ url, init });
    const key = `${init.method ?? 'GET'} ${url.split('?')[0]}`;
    const route = routes[key];
    if (!route) return json(404, { error: { code: 'not_found', message: 'No route' } });
    return route(init);
  }) as typeof fetch;
  return { fetchImpl, calls };
}

function json(status: number, body: unknown): Response {
  return new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
}

const header = (init: RequestInit, name: string) => (init.headers as Record<string, string>)[name];

describe('session handling', () => {
  test('a missing session is null, not an error', async () => {
    const { fetchImpl } = fakeFetch({ 'GET /api/v1/session': () => json(401, { error: { code: 'unauthorized', message: 'No session' } }) });
    expect(await new HttpApi({ fetch: fetchImpl }).getSession()).toBeNull();
  });

  test('login posts credentials same-origin without following redirects, then unsafe calls carry the CSRF token', async () => {
    const { fetchImpl, calls } = fakeFetch({
      'POST /api/v1/session': () => json(200, SESSION),
      'DELETE /api/v1/session': () => new Response(null, { status: 204 }),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    expect(await api.login({ username: 'owner', password: 'pw' })).toEqual(SESSION);
    const [login] = calls;
    expect(login.init).toMatchObject({ method: 'POST', credentials: 'same-origin', redirect: 'error', cache: 'no-store' });
    expect(JSON.parse(String(login.init.body))).toEqual({ username: 'owner', password: 'pw' });
    expect(header(login.init, CSRF_HEADER)).toBeUndefined();

    await api.logout();
    expect(header(calls[1].init, CSRF_HEADER)).toBe(SESSION.csrfToken);
  });

  test('after logout or a 401 no unsafe request is sent without a fresh session', async () => {
    const { fetchImpl, calls } = fakeFetch({
      'POST /api/v1/session': () => json(200, SESSION),
      'GET /api/v1/hosts': () => json(401, {}),
      'POST /api/v1/hosts/atlas/terminal': () => json(200, {}),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    await api.login({ username: 'owner', password: 'pw' });
    await expect(api.listHosts()).rejects.toMatchObject({ code: 'unauthorized', status: 401 });
    await expect(api.openTerminal('atlas')).rejects.toMatchObject({ code: 'unauthorized' });
    expect(calls.map(c => c.url)).toEqual(['/api/v1/session', '/api/v1/hosts']);
  });

  test('the CSRF token is never written to web storage', async () => {
    const { fetchImpl } = fakeFetch({ 'POST /api/v1/session': () => json(200, SESSION) });
    await new HttpApi({ fetch: fetchImpl }).login({ username: 'owner', password: 'pw' });
    expect(JSON.stringify({ ...localStorage })).not.toContain(SESSION.csrfToken);
    expect(JSON.stringify({ ...sessionStorage })).not.toContain(SESSION.csrfToken);
  });
});

describe('errors', () => {
  test.each([
    [429, 'rate_limited'],
    [403, 'forbidden'],
    [404, 'not_found'],
    [400, 'invalid_request'],
    [503, 'unavailable'],
  ])('HTTP %i maps to %s', async (status, code) => {
    const { fetchImpl } = fakeFetch({ 'GET /api/v1/hosts': () => json(status, { error: { code: 'x', message: 'Server says no' } }) });
    await expect(new HttpApi({ fetch: fetchImpl }).listHosts()).rejects.toMatchObject({ code, status, message: 'Server says no' });
  });

  test('network failures, invalid JSON and wrong shapes are distinct errors', async () => {
    const failing = (() => Promise.reject(new TypeError('offline'))) as typeof fetch;
    await expect(new HttpApi({ fetch: failing }).listHosts()).rejects.toMatchObject({ code: 'network' });
    const html = fakeFetch({ 'GET /api/v1/hosts': () => new Response('<html>', { status: 200 }) });
    await expect(new HttpApi({ fetch: html.fetchImpl }).listHosts()).rejects.toMatchObject({ code: 'bad_response' });
    const wrong = fakeFetch({ 'GET /api/v1/hosts': () => json(200, { hosts: [{ id: 'x' }] }) });
    await expect(new HttpApi({ fetch: wrong.fetchImpl }).listHosts()).rejects.toMatchObject({ code: 'bad_response' });
  });

  test('unsafe identifiers are refused before any request', async () => {
    const { fetchImpl, calls } = fakeFetch({});
    const api = new HttpApi({ fetch: fetchImpl });
    await expect(api.getHost('../admin')).rejects.toMatchObject({ code: 'invalid_request' });
    await expect(api.getVm('a/b')).rejects.toMatchObject({ code: 'invalid_request' });
    expect(calls).toHaveLength(0);
  });
});

describe('data endpoints', () => {
  test('hosts, a host, VMs and a VM decode from their endpoints', async () => {
    const { fetchImpl } = fakeFetch({
      'GET /api/v1/hosts': () => json(200, { hosts: FIXTURE_HOSTS }),
      'GET /api/v1/hosts/atlas': () => json(200, FIXTURE_HOSTS[0]),
      'GET /api/v1/vms': () => json(200, { vms: FIXTURE_VMS }),
      'GET /api/v1/vms/ubuntu-arm-03': () => json(200, FIXTURE_VMS[3]),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    expect(await api.listHosts()).toHaveLength(FIXTURE_HOSTS.length);
    expect((await api.getHost('atlas')).arch).toBe('x86_64');
    expect(await api.listVms()).toHaveLength(FIXTURE_VMS.length);
    expect((await api.getVm('ubuntu-arm-03')).arch).toBe('arm64');
  });

  test('CPU history passes its abort signal to fetch, and an abort before the response rejects with the reason', async () => {
    const seen: (AbortSignal | null | undefined)[] = [];
    const { fetchImpl } = fakeFetch({
      'GET /api/v1/hosts/atlas/cpu-history': init => {
        seen.push(init.signal);
        // Like fetch: never answers, rejects once the signal aborts.
        return new Promise<Response>((_, reject) => init.signal!.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError'))));
      },
      'GET /api/v1/hosts/cpu-history': init => {
        seen.push(init.signal);
        return new Promise<Response>((_, reject) => init.signal!.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError'))));
      },
    });
    const api = new HttpApi({ fetch: fetchImpl });
    const timeout = new ApiError('unavailable', 'CPU history did not arrive in time.');
    const one = new AbortController();
    const single = api.getCpuHistory('atlas', '1h', { signal: one.signal });
    const batchController = new AbortController();
    const batch = api.listCpuHistory('24h', { signal: batchController.signal });
    one.abort(timeout);
    batchController.abort();
    await expect(single).rejects.toBe(timeout);
    await expect(batch).rejects.toMatchObject({ code: 'network', message: 'The request was cancelled.' });
    expect(seen).toEqual([one.signal, batchController.signal]);
  });

  test('an abort ends a stalled CPU history body read instead of waiting for it', async () => {
    let cancelled = false;
    const stalled = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('{"hostId":"atl'));
      },
      cancel() {
        cancelled = true;
      },
    });
    const { fetchImpl } = fakeFetch({ 'GET /api/v1/hosts/atlas/cpu-history': () => new Response(stalled, { status: 200 }) });
    const api = new HttpApi({ fetch: fetchImpl });
    const controller = new AbortController();
    const pending = api.getCpuHistory('atlas', '24h', { signal: controller.signal });
    await new Promise(resolve => setTimeout(resolve, 10));
    const reason = new ApiError('unavailable', 'CPU history did not arrive in time.');
    controller.abort(reason);
    await expect(pending).rejects.toBe(reason);
    expect(cancelled).toBe(true);
  });

  test('a request without a signal sends none', async () => {
    const series = { hostId: 'atlas', range: '1h', bucketSeconds: 60, from: '2026-11-26T09:20:00Z', to: '2026-11-26T10:20:00Z', samples: Array<null>(60).fill(null) };
    const { fetchImpl, calls } = fakeFetch({ 'GET /api/v1/hosts/atlas/cpu-history': () => json(200, series) });
    await new HttpApi({ fetch: fetchImpl }).getCpuHistory('atlas', '1h');
    expect('signal' in calls[0].init).toBe(false);
  });

  test('CPU history uses the host and batch endpoints and checks the returned identity', async () => {
    const series = {
      hostId: 'atlas', range: '24h', bucketSeconds: 600,
      from: '2026-11-25T10:20:00Z', to: '2026-11-26T10:20:00Z',
      samples: Array<null>(144).fill(null),
    };
    const { fetchImpl, calls } = fakeFetch({
      'GET /api/v1/hosts/atlas/cpu-history': () => json(200, series),
      'GET /api/v1/hosts/harbor/cpu-history': () => json(200, series),
      'GET /api/v1/hosts/cpu-history': () => json(200, { series: [series] }),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    expect(await api.getCpuHistory('atlas', '24h')).toEqual(series);
    expect(await api.listCpuHistory('24h')).toEqual([series]);
    await expect(api.getCpuHistory('harbor', '24h')).rejects.toMatchObject({ code: 'bad_response' });
    await expect(api.getCpuHistory('../atlas', '24h')).rejects.toMatchObject({ code: 'invalid_request' });
    expect(calls.map(call => call.url)).toEqual([
      '/api/v1/hosts/atlas/cpu-history?range=24h',
      '/api/v1/hosts/cpu-history?range=24h',
      '/api/v1/hosts/harbor/cpu-history?range=24h',
    ]);
  });

  test('activity query parameters are validated and encoded', () => {
    expect(activitySearch()).toBe('limit=20');
    expect(activitySearch({ limit: 5, beforeId: 40, outcome: 'failed', tool: 'run_command', hostId: 'atlas', includeReads: true }))
      .toBe('limit=5&before_id=40&outcome=failed&tool=run_command&host_id=atlas&include_reads=true');
    expect(() => activitySearch({ limit: 51 })).toThrow();
    expect(() => activitySearch({ beforeId: 0 })).toThrow();
    expect(() => activitySearch({ tool: 'run;rm' })).toThrow();
  });

  test('an older catalog can leave VM resources and snapshot time unknown', async () => {
    const vm = {
      ...FIXTURE_VMS[1],
      cpus: null,
      memoryBytes: null,
      snapshots: FIXTURE_VMS[1].snapshots.map(snapshot => ({ ...snapshot, createdAt: null })),
    };
    const { fetchImpl } = fakeFetch({ ['GET /api/v1/vms/' + vm.id]: () => json(200, vm) });
    expect(await new HttpApi({ fetch: fetchImpl }).getVm(vm.id)).toEqual(vm);
  });
});

class FakeSocket extends EventTarget {
  static last: FakeSocket | null = null;
  sent: string[] = [];
  binaryType = 'blob';
  closed = false;
  constructor(readonly url: string) {
    super();
    FakeSocket.last = this;
  }
  send(data: string) {
    this.sent.push(data);
  }
  close() {
    this.closed = true;
    this.dispatchEvent(new Event('close'));
  }
  open() {
    this.dispatchEvent(new Event('open'));
  }
  receive(data: unknown) {
    this.dispatchEvent(new MessageEvent('message', { data }));
  }
}

async function streamingApi(path: string) {
  const { fetchImpl, calls } = fakeFetch({
    'POST /api/v1/session': () => json(200, SESSION),
    [`POST ${path}`]: () => json(200, { streamPath: '/api/v1/streams/s-1', ticket: 'ticket-0123456789abcdef' }),
  });
  const api = new HttpApi({ fetch: fetchImpl, WebSocket: FakeSocket as unknown as new (url: string) => WebSocket, pageUrl: 'https://atlas.tail.ts.net:7443/#/terminal' });
  await api.login({ username: 'owner', password: 'pw' });
  return { api, calls };
}

describe('terminal stream', () => {
  test('a ticket is fetched with CSRF, sent as the first message on a wss:// socket, and never put in the URL', async () => {
    const { api, calls } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    expect(header(calls[1].init, CSRF_HEADER)).toBe(SESSION.csrfToken);
    const ws = FakeSocket.last!;
    expect(ws.url).toBe('wss://atlas.tail.ts.net:7443/api/v1/streams/s-1');
    ws.open();
    expect(JSON.parse(ws.sent[0])).toEqual({ type: 'auth', ticket: 'ticket-0123456789abcdef' });

    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    ws.receive(JSON.stringify({ type: 'ready' }));
    expect(session.status).toBe('ready');
    session.run({ tool: 'git', args: ['--version'] });
    expect(JSON.parse(ws.sent[1])).toEqual({ type: 'run', tool: 'git', args: ['--version'] });
    ws.receive(JSON.stringify({ type: 'started', commandId: 'c1', tool: 'git', args: ['--version'] }));
    ws.receive(JSON.stringify({ type: 'output', commandId: 'c1', stream: 'stdout', data: 'git version 2.43.0' }));
    ws.receive(JSON.stringify({ type: 'exit', commandId: 'c1', exitCode: 0, outcome: 'succeeded', truncated: false }));
    expect(events.filter(e => e.type !== 'status')).toEqual([
      { type: 'started', commandId: 'c1', tool: 'git', args: ['--version'] },
      { type: 'output', commandId: 'c1', stream: 'stdout', data: 'git version 2.43.0' },
      { type: 'exit', commandId: 'c1', exitCode: 0, outcome: 'succeeded', truncated: false },
    ]);
    expect(session.status).toBe('ready');
  });

  test('a malformed server message ends the session with an error', async () => {
    const { api } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    FakeSocket.last!.receive('{"type":"output","stream":"stdin"}');
    expect(session.status).toBe('error');
    expect(events.some(e => e.type === 'error')).toBe(true);
    expect(FakeSocket.last!.closed).toBe(true);
  });

  test('message decoding rejects oversized or unknown messages', () => {
    expect(decodeTerminalMessage('x'.repeat(200_000))).toBeNull();
    expect(decodeTerminalMessage(JSON.stringify({ type: 'shell' }))).toBeNull();
    expect(decodeTerminalMessage(JSON.stringify({ type: 'exit', commandId: 'c', exitCode: 1.5, outcome: 'failed' }))).toBeNull();
  });

  test('a stream path outside /api/v1/streams is refused', async () => {
    const { fetchImpl } = fakeFetch({
      'POST /api/v1/session': () => json(200, SESSION),
      'POST /api/v1/hosts/atlas/terminal': () => json(200, { streamPath: '//evil.example/x', ticket: 'ticket-0123456789abcdef' }),
    });
    const api = new HttpApi({ fetch: fetchImpl, WebSocket: FakeSocket as unknown as new (url: string) => WebSocket });
    await api.login({ username: 'owner', password: 'pw' });
    await expect(api.openTerminal('atlas')).rejects.toMatchObject({ code: 'bad_response' });
  });
});

describe('console stream', () => {
  test('frame metadata followed by binary image data yields one frame', async () => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const frames: ConsoleFrame[] = [];
    stream.onFrame(f => frames.push(f));
    const ws = FakeSocket.last!;
    ws.receive(JSON.stringify({ type: 'frame', seq: 7, width: 1024, height: 768, mime: 'image/png', capturedAt: '2026-11-23T12:00:07Z' }));
    expect(stream.status).toBe('streaming');
    ws.receive(new Blob([new Uint8Array([137, 80, 78, 71])]));
    expect(frames).toHaveLength(1);
    expect(frames[0]).toMatchObject({ seq: 7, width: 1024, height: 768, mime: 'image/png' });
    expect(frames[0].data.type).toBe('image/png');
    ws.receive(new Blob([new Uint8Array([1])]));
    expect(frames).toHaveLength(1);
  });

  test('a server-sent SVG frame is refused', async () => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    FakeSocket.last!.receive(JSON.stringify({ type: 'frame', seq: 1, width: 1, height: 1, mime: 'image/svg+xml', capturedAt: '2026-11-23T12:00:07Z' }));
    expect(stream.status).toBe('error');
  });
});

function serverClose(ws: FakeSocket, code: number, reason = '') {
  ws.dispatchEvent(Object.assign(new Event('close'), { code, reason }));
}

describe('sign-out confirmation', () => {
  test('a failed DELETE rejects, keeps the CSRF token, and a retry sends DELETE again', async () => {
    let attempts = 0;
    const { fetchImpl, calls } = fakeFetch({
      'POST /api/v1/session': () => json(200, SESSION),
      'DELETE /api/v1/session': () => (++attempts === 1 ? json(503, { error: { code: 'unavailable', message: 'arxd is restarting' } }) : new Response(null, { status: 204 })),
      'GET /api/v1/session': () => json(401, {}),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    await api.login({ username: 'owner', password: 'pw' });
    await expect(api.logout()).rejects.toMatchObject({ code: 'unavailable', message: 'arxd is restarting' });
    await api.logout();
    const deletes = calls.filter(c => c.init.method === 'DELETE');
    expect(deletes).toHaveLength(2);
    expect(deletes.every(c => header(c.init, CSRF_HEADER) === SESSION.csrfToken)).toBe(true);
    // Without a token, sign-out asks the server; its 401 confirms nothing is left.
    await api.logout();
    expect(calls.filter(c => c.init.method === 'DELETE')).toHaveLength(2);
    expect(calls.at(-1)).toMatchObject({ url: '/api/v1/session', init: { method: 'GET' } });
  });

  test('a DELETE answered 401 counts as signed out: the session was already gone', async () => {
    const { fetchImpl } = fakeFetch({
      'POST /api/v1/session': () => json(200, SESSION),
      'DELETE /api/v1/session': () => json(401, {}),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    await api.login({ username: 'owner', password: 'pw' });
    await expect(api.logout()).resolves.toBeUndefined();
  });
});

describe('round 4: a stream 4401 is bound to the session it was opened under', () => {
  const TOKENS = ['csrf-token-session-a-0123', 'csrf-token-session-b-4567'];

  function twoSessionApi(path: string, live: () => boolean = () => false) {
    let logins = 0;
    const { fetchImpl, calls } = fakeFetch({
      'POST /api/v1/session': () => json(200, { ...SESSION, csrfToken: TOKENS[logins++] }),
      'GET /api/v1/session': () => (live() ? json(200, { ...SESSION, csrfToken: TOKENS[logins - 1] }) : json(401, {})),
      'DELETE /api/v1/session': () => new Response(null, { status: 204 }),
      [`POST ${path}`]: () => json(200, { streamPath: '/api/v1/streams/s-1', ticket: 'ticket-0123456789abcdef' }),
    });
    const api = new HttpApi({ fetch: fetchImpl, WebSocket: FakeSocket as unknown as new (url: string) => WebSocket, pageUrl: 'https://atlas.tail.ts.net:7443/' });
    return { api, calls };
  }

  /** A browser fires one close event; a disposed socket's may arrive late. */
  function quietClose(ws: FakeSocket) {
    ws.close = () => {
      ws.closed = true;
    };
  }

  const kinds = [
    ['console', '/api/v1/vms/web-static/console', (api: HttpApi) => api.openConsole('web-static')],
    ['terminal', '/api/v1/hosts/atlas/terminal', (api: HttpApi) => api.openTerminal('atlas')],
  ] as const;

  test.each(kinds)('a late 4401 from a disposed %s of session A keeps session B', async (_kind, path, open) => {
    const { api, calls } = twoSessionApi(path);
    await api.login({ username: 'owner', password: 'pw' });
    const stream = await open(api);
    const old = FakeSocket.last!;
    quietClose(old);
    await api.logout();
    stream.close();
    await api.login({ username: 'owner', password: 'pw' });
    serverClose(old, 4401);
    await open(api);
    await api.logout();
    const tickets = calls.filter(c => c.url === path);
    expect(tickets.map(c => header(c.init, CSRF_HEADER))).toEqual(TOKENS);
    const deletes = calls.filter(c => c.init.method === 'DELETE');
    expect(deletes.map(c => header(c.init, CSRF_HEADER))).toEqual(TOKENS);
  });

  test('a still-open console of session A only closes on its late 4401', async () => {
    const { api, calls } = twoSessionApi('/api/v1/vms/web-static/console');
    await api.login({ username: 'owner', password: 'pw' });
    const stream = await api.openConsole('web-static');
    const statuses: [string, string | undefined, boolean | undefined][] = [];
    stream.onStatus((s, r, ended) => statuses.push([s, r, ended]));
    await api.logout();
    await api.login({ username: 'owner', password: 'pw' });
    serverClose(FakeSocket.last!, 4401);
    expect(statuses).toEqual([['closed', 'The console closed because the session it was opened under ended.', false]]);
    await api.logout();
    expect(header(calls.at(-1)!.init, CSRF_HEADER)).toBe(TOKENS[1]);
  });

  test('a still-open terminal of session A only closes on its late 4401', async () => {
    const { api } = twoSessionApi('/api/v1/hosts/atlas/terminal');
    await api.login({ username: 'owner', password: 'pw' });
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    await api.logout();
    await api.login({ username: 'owner', password: 'pw' });
    serverClose(FakeSocket.last!, 4401);
    expect(events.filter(e => e.type === 'closed')).toEqual([{ type: 'closed', reason: 'server', message: 'The terminal closed because the session it was opened under ended.' }]);
    await expect(api.openTerminal('atlas')).resolves.toBeDefined();
  });

  test.each(kinds)('a 4401 for the current session still ends it (%s)', async (kind, path, open) => {
    const { api, calls } = twoSessionApi(path);
    await api.login({ username: 'owner', password: 'pw' });
    const stream = await open(api);
    const ended: boolean[] = [];
    if (kind === 'console') (stream as ConsoleStream).onStatus((_s, _r, e) => ended.push(e === true));
    else (stream as TerminalSession).onEvent(e => {
      if (e.type === 'closed') ended.push(e.reason === 'session_ended');
    });
    serverClose(FakeSocket.last!, 4401);
    expect(ended).toEqual([true]);
    const before = calls.length;
    await expect(open(api)).rejects.toMatchObject({ code: 'unauthorized' });
    expect(calls).toHaveLength(before);
  });

  test('sign-out without a token re-reads the session and revokes one that is still live', async () => {
    let live = true;
    const { api, calls } = twoSessionApi('/api/v1/vms/web-static/console', () => live);
    await api.login({ username: 'owner', password: 'pw' });
    await api.openConsole('web-static');
    serverClose(FakeSocket.last!, 4401);
    await api.logout();
    expect(calls.slice(-2).map(c => `${c.init.method} ${c.url}`)).toEqual(['GET /api/v1/session', 'DELETE /api/v1/session']);
    expect(header(calls.at(-1)!.init, CSRF_HEADER)).toBe(TOKENS[0]);
    // Once the server reports no session, sign-out resolves without DELETE.
    live = false;
    await api.logout();
    expect(calls.at(-1)).toMatchObject({ url: '/api/v1/session', init: { method: 'GET' } });
    expect(calls.filter(c => c.init.method === 'DELETE')).toHaveLength(1);
  });

  test('sign-out without a token rejects when the session cannot be re-read', async () => {
    const { fetchImpl } = fakeFetch({ 'GET /api/v1/session': () => json(503, { error: { code: 'unavailable', message: 'arxd is restarting' } }) });
    await expect(new HttpApi({ fetch: fetchImpl }).logout()).rejects.toMatchObject({ code: 'unavailable' });
  });
});

describe('round 5: an HTTP 401 is bound to the session it was sent under', () => {
  /**
   * A fake arxd with one live session at a time. Ticket requests can be held
   * and answered later.
   */
  function racingApi() {
    let live = 0;
    let logins = 0;
    let hold = 0;
    const held: ((response: Response) => void)[] = [];
    const trace: string[] = [];
    const token = () => `csrf-token-session-${live}-0123`;
    const fetchImpl = (async (input: RequestInfo | URL, init: RequestInit = {}) => {
      const url = String(input);
      const method = init.method ?? 'GET';
      const csrf = (init.headers as Record<string, string>)[CSRF_HEADER];
      if (url === '/api/v1/session') {
        trace.push(method === 'DELETE' ? `DELETE ${csrf}` : method);
        if (method === 'POST') {
          live = ++logins;
          return json(200, { ...SESSION, csrfToken: token() });
        }
        if (method === 'DELETE') {
          if (!live) return json(401, {});
          if (csrf !== token()) return json(403, {});
          live = 0;
          return new Response(null, { status: 204 });
        }
        if (!live) return json(401, {});
        return json(200, { ...SESSION, csrfToken: token() });
      }
      trace.push(`TICKET ${csrf}`);
      if (hold > 0) {
        hold -= 1;
        return new Promise<Response>(resolve => held.push(resolve));
      }
      if (!live) return json(401, {});
      if (csrf !== token()) return json(403, {});
      return json(200, { streamPath: '/api/v1/streams/s-1', ticket: 'ticket-0123456789abcdef' });
    }) as typeof fetch;
    const api = new HttpApi({ fetch: fetchImpl, WebSocket: FakeSocket as unknown as new (url: string) => WebSocket, pageUrl: 'https://atlas.tail.ts.net:7443/' });
    return {
      api,
      trace,
      held,
      live: () => live,
      hold: (n: number) => {
        hold = n;
      },
    };
  }

  const creds = { username: 'owner', password: 'pw' };
  const failure = (p: Promise<unknown>) => p.then(() => 'resolved', (e: unknown) => (isApiError(e) ? e.code : 'thrown'));
  const kinds = [
    ['console', (api: HttpApi) => api.openConsole('web-static')],
    ['terminal', (api: HttpApi) => api.openTerminal('atlas')],
  ] as const;

  test.each(kinds)('a delayed %s ticket 401 from session A leaves session B usable', async (_kind, open) => {
    const f = racingApi();
    await f.api.login(creds);
    f.hold(1);
    const old = failure(open(f.api));
    await f.api.logout();
    await f.api.login(creds);
    await open(f.api);
    f.held[0](json(401, {}));
    // The old open fails without telling the view to end session B.
    expect(await old).toBe('unavailable');
    await open(f.api);
    expect(f.trace.filter(t => t.startsWith('TICKET')).slice(-2)).toEqual(['TICKET csrf-token-session-2-0123', 'TICKET csrf-token-session-2-0123']);
    await f.api.logout();
    expect(f.live()).toBe(0);
    expect(f.trace.at(-1)).toBe('DELETE csrf-token-session-2-0123');
  });

  test.each(kinds)('the stale-401 sign-out interleaving still revokes session B (%s)', async (_kind, open) => {
    const f = racingApi();
    await f.api.login(creds);
    f.hold(2);
    const old1 = failure(open(f.api));
    const old2 = failure(open(f.api));
    await f.api.logout();
    await f.api.login(creds);
    f.held[0](json(401, {}));
    expect(await old1).toBe('unavailable');
    // Neither stale 401 cleared B's token, so sign-out needs no recovery GET
    // and revokes B. (A stale 401 during recovery is the next test.)
    f.held[1](json(401, {}));
    expect(await old2).toBe('unavailable');
    await f.api.logout();
    expect(f.trace).not.toContain('GET');
    expect(f.live()).toBe(0);
    expect(f.trace.at(-1)).toBe('DELETE csrf-token-session-2-0123');
  });

  test.each(kinds)('recovery GET finds B, a stale 401 lands before the DELETE, and the DELETE still revokes B (%s)', async (_kind, open) => {
    const f = racingApi();
    await f.api.login(creds);
    f.hold(1);
    const old = failure(open(f.api));
    await f.api.logout();
    await f.api.login(creds);
    await open(f.api);
    // B's stream reports 4401 although the fake server still holds B, so
    // sign-out starts without a token and must recover it.
    serverClose(FakeSocket.last!, 4401);
    // The reviewer's interleaving: the stale 401 is handled after the recovery
    // GET restored B's token and before the DELETE is dispatched.
    const recover = f.api.getSession.bind(f.api);
    f.api.getSession = async () => {
      const session = await recover();
      f.held[0](json(401, {}));
      expect(await old).toBe('unavailable');
      return session;
    };
    await f.api.logout();
    expect(f.trace.slice(-2)).toEqual(['GET', 'DELETE csrf-token-session-2-0123']);
    expect(f.live()).toBe(0);
  });

  test('a DELETE that cannot be dispatched rejects for the retry instead of confirming', async () => {
    const f = racingApi();
    await f.api.login(creds);
    await f.api.openConsole('web-static');
    serverClose(FakeSocket.last!, 4401);
    // Simulate the token vanishing between the recovery GET and the DELETE.
    const recover = f.api.getSession.bind(f.api);
    f.api.getSession = async () => {
      const session = await recover();
      (f.api as unknown as { csrfToken: string | null }).csrfToken = null;
      return session;
    };
    await expect(f.api.logout()).rejects.toMatchObject({ code: 'unavailable', status: null });
    expect(f.trace.at(-1)).toBe('GET');
    expect(f.live()).toBe(1);
    // The retry recovers the session and revokes it.
    f.api.getSession = recover;
    await f.api.logout();
    expect(f.trace.slice(-2)).toEqual(['GET', 'DELETE csrf-token-session-1-0123']);
    expect(f.live()).toBe(0);
  });

  test.each(kinds)('an earlier-session 401 with no session held still reports unauthorized (%s)', async (_kind, open) => {
    const f = racingApi();
    await f.api.login(creds);
    f.hold(1);
    const old = open(f.api).then(() => null, (error: unknown) => error);
    await f.api.logout();
    expect(f.live()).toBe(0);
    // No replacement session: the late 401 is authoritative, so the view is
    // told the session ended (a no-op for the already-anonymous shell).
    f.held[0](json(401, {}));
    expect(await old).toMatchObject({ code: 'unauthorized', status: 401 });
    // Nothing is held, so the next unsafe request is refused locally.
    await expect(open(f.api)).rejects.toMatchObject({ code: 'unauthorized', status: null });
  });

  test('a delayed sign-out 204 does not clear a session that signed in meanwhile', async () => {
    let heldDelete: ((response: Response) => void) | undefined;
    let logins = 0;
    const trace: string[] = [];
    const fetchImpl = (async (input: RequestInfo | URL, init: RequestInit = {}) => {
      const method = init.method ?? 'GET';
      const csrf = (init.headers as Record<string, string>)[CSRF_HEADER];
      if (String(input) === '/api/v1/session') {
        if (method === 'POST') return json(200, { ...SESSION, csrfToken: `csrf-token-session-${++logins}-0123` });
        trace.push(`${method} ${csrf}`);
        return new Promise<Response>(resolve => {
          heldDelete = resolve;
        });
      }
      trace.push(`TICKET ${csrf}`);
      return json(200, { streamPath: '/api/v1/streams/s-1', ticket: 'ticket-0123456789abcdef' });
    }) as typeof fetch;
    const api = new HttpApi({ fetch: fetchImpl, WebSocket: FakeSocket as unknown as new (url: string) => WebSocket, pageUrl: 'https://atlas.tail.ts.net:7443/' });
    await api.login(creds);
    const signOut = api.logout();
    expect(trace).toEqual(['DELETE csrf-token-session-1-0123']);
    // Session B signs in while A's DELETE is still in flight, then A's 204 lands.
    const second = await api.login(creds);
    expect(second.csrfToken).toBe('csrf-token-session-2-0123');
    heldDelete!(new Response(null, { status: 204 }));
    await signOut;
    // B's token survived: an unsafe request is still sent under it.
    await api.openConsole('web-static');
    expect(trace.at(-1)).toBe(`TICKET ${second.csrfToken}`);
  });

  test('a sign-out 204 with no newer session still clears the token', async () => {
    const f = racingApi();
    await f.api.login(creds);
    await f.api.logout();
    await expect(f.api.openConsole('web-static')).rejects.toMatchObject({ code: 'unauthorized', status: null });
    expect(f.trace.filter(t => t.startsWith('TICKET'))).toEqual([]);
  });
});

describe('bounded response bodies', () => {
  test('a body without Content-Length is cut off once it exceeds 2 MiB', async () => {
    let pulled = 0;
    let cancelled = false;
    const chunk = new Uint8Array(256 * 1024).fill(0x20);
    const body = new ReadableStream<Uint8Array>({
      pull(controller) {
        pulled += 1;
        controller.enqueue(chunk);
      },
      cancel() {
        cancelled = true;
      },
    });
    const { fetchImpl } = fakeFetch({ 'GET /api/v1/hosts': () => new Response(body, { status: 200 }) });
    await expect(new HttpApi({ fetch: fetchImpl }).listHosts()).rejects.toMatchObject({ code: 'bad_response' });
    expect(cancelled).toBe(true);
    expect(pulled).toBeLessThanOrEqual(10);
  });

  test('a declared oversized Content-Length is refused before reading, and bytes, not characters, count', async () => {
    const big = fakeFetch({ 'GET /api/v1/hosts': () => new Response('{}', { status: 200, headers: { 'Content-Length': String(3 * 1024 * 1024) } }) });
    await expect(new HttpApi({ fetch: big.fetchImpl }).listHosts()).rejects.toMatchObject({ code: 'bad_response' });
    expect(await readBounded(new Response('é'.repeat(10)), 20)).toBe('é'.repeat(10));
    await expect(readBounded(new Response('é'.repeat(11)), 20)).rejects.toMatchObject({ code: 'bad_response' });
  });

  test('an oversized error body still yields the status-based error', async () => {
    const { fetchImpl } = fakeFetch({ 'GET /api/v1/hosts': () => new Response('x', { status: 503, headers: { 'Content-Length': String(9 * 1024 * 1024) } }) });
    await expect(new HttpApi({ fetch: fetchImpl }).listHosts()).rejects.toMatchObject({ code: 'unavailable', status: 503 });
  });
});

describe('server-initiated stream close', () => {
  test('the terminal reports a session-ended close (4401) and a plain server close with its reason', async () => {
    const { api } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    const ws = FakeSocket.last!;
    ws.receive(JSON.stringify({ type: 'ready' }));
    serverClose(ws, 4401);
    expect(events.find(e => e.type === 'closed')).toEqual({ type: 'closed', reason: 'session_ended', message: 'Your session ended, so the server closed the terminal.' });
    expect(session.status).toBe('closed');

    // 4401 forgets the CSRF token, so a new stream needs a new session.
    await api.login({ username: 'owner', password: 'pw' });
    const second = await api.openTerminal('atlas');
    const more: TerminalEvent[] = [];
    second.onEvent(e => more.push(e));
    serverClose(FakeSocket.last!, 1001, 'arxd is shutting down');
    expect(more.find(e => e.type === 'closed')).toEqual({ type: 'closed', reason: 'server', message: 'The server closed the terminal: arxd is shutting down' });
  });

  test('closing from the client side does not report a server close', async () => {
    const { api } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    session.close();
    expect(events.some(e => e.type === 'closed')).toBe(false);
  });

  test('the console marks a 4401 close as session-ended', async () => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const statuses: [string, string | undefined, boolean | undefined][] = [];
    stream.onStatus((s, r, ended) => statuses.push([s, r, ended]));
    serverClose(FakeSocket.last!, 4401);
    expect(statuses).toEqual([['closed', 'Your session ended, so the server closed the console.', true]]);
  });

  test('a 4401 after a "VM stopped" notice still ends the session and forgets the CSRF token', async () => {
    const { api, calls } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const statuses: [string, string | undefined, boolean | undefined][] = [];
    stream.onStatus((s, r, ended) => statuses.push([s, r, ended]));
    const ws = FakeSocket.last!;
    ws.receive(JSON.stringify({ type: 'closed', reason: 'VM stopped' }));
    serverClose(ws, 4401);
    expect(statuses).toEqual([
      ['closed', 'VM stopped', false],
      ['closed', 'Your session ended, so the server closed the console.', true],
    ]);
    expect(stream.status).toBe('closed');
    const tickets = calls.length;
    await expect(api.openConsole('web-static')).rejects.toMatchObject({ code: 'unauthorized' });
    expect(calls).toHaveLength(tickets);
  });

  test('an ordinary close after a "VM stopped" notice keeps the notice and the session', async () => {
    const { api, calls } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const statuses: [string, string | undefined, boolean | undefined][] = [];
    stream.onStatus((s, r, ended) => statuses.push([s, r, ended]));
    const ws = FakeSocket.last!;
    ws.receive(JSON.stringify({ type: 'closed', reason: 'VM stopped' }));
    serverClose(ws, 1000, 'VM stopped');
    expect(statuses).toEqual([['closed', 'VM stopped', false]]);
    const tickets = calls.length;
    await api.openConsole('web-static');
    expect(calls).toHaveLength(tickets + 1);
  });

  test('a 4401 after a client-side terminal failure still ends the session', async () => {
    const { api, calls } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    const ws = FakeSocket.last!;
    // A browser fires one close event; when the server's 4401 crossed the
    // client's own close, that event carries 4401.
    ws.close = () => {
      ws.closed = true;
    };
    ws.receive('not json');
    expect(session.status).toBe('error');
    expect(ws.closed).toBe(true);
    serverClose(ws, 4401);
    expect(events.filter(e => e.type === 'closed')).toEqual([{ type: 'closed', reason: 'session_ended', message: 'Your session ended, so the server closed the terminal.' }]);
    const tickets = calls.length;
    await expect(api.openTerminal('atlas')).rejects.toMatchObject({ code: 'unauthorized' });
    expect(calls).toHaveLength(tickets);
  });
});

describe('console frame validation', () => {
  const frame = (over: Record<string, unknown>) => JSON.stringify({ type: 'frame', seq: 1, width: 640, height: 400, mime: 'image/png', capturedAt: '2026-11-23T12:00:01Z', ...over });

  test.each([
    ['a malformed timestamp', { capturedAt: 'not a date' }],
    ['an impossible date', { capturedAt: '2026-13-45T99:00:00Z' }],
    ['a zero width', { width: 0 }],
    ['a huge height', { height: 100_000 }],
    ['a fractional sequence', { seq: 1.5 }],
  ])('%s puts the stream in an error state without emitting a frame', async (_name, over) => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const frames: ConsoleFrame[] = [];
    stream.onFrame(f => frames.push(f));
    FakeSocket.last!.receive(frame(over));
    FakeSocket.last!.receive(new Blob([new Uint8Array([1])]));
    expect(stream.status).toBe('error');
    expect(frames).toHaveLength(0);
  });

  test('sequence numbers must strictly increase', async () => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const frames: ConsoleFrame[] = [];
    stream.onFrame(f => frames.push(f));
    const ws = FakeSocket.last!;
    ws.receive(frame({ seq: 5 }));
    ws.receive(new Blob([new Uint8Array([1])]));
    ws.receive(frame({ seq: 5 }));
    expect(frames.map(f => f.seq)).toEqual([5]);
    expect(stream.status).toBe('error');
    expect(decodeFrameMeta({ seq: 6, width: 1, height: 1, mime: 'image/jpeg', capturedAt: '2026-11-23T12:00:01.5+01:00' }, 5)).not.toBeNull();
  });

  test('image data without preceding metadata is a protocol error', async () => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    FakeSocket.last!.receive(new Blob([new Uint8Array([1])]));
    expect(stream.status).toBe('error');
  });
});

describe('round 3: exact sign-out confirmation', () => {
  test.each([200, 202])('a DELETE answered %i is unconfirmed: it rejects, keeps the token and the retry sends DELETE again', async status => {
    let attempts = 0;
    const { fetchImpl, calls } = fakeFetch({
      'POST /api/v1/session': () => json(200, SESSION),
      'DELETE /api/v1/session': () => (++attempts === 1 ? new Response(status === 200 ? '{}' : null, { status }) : new Response(null, { status: 204 })),
    });
    const api = new HttpApi({ fetch: fetchImpl });
    await api.login({ username: 'owner', password: 'pw' });
    await expect(api.logout()).rejects.toMatchObject({ code: 'unavailable', status });
    await api.logout();
    const deletes = calls.filter(c => c.init.method === 'DELETE');
    expect(deletes).toHaveLength(2);
    expect(header(deletes[1].init, CSRF_HEADER)).toBe(SESSION.csrfToken);
  });
});

describe('round 3: revocation while a stream is busy', () => {
  test('a 4401 mid-command drops every later message and forgets the CSRF token', async () => {
    const { api, calls } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    const ws = FakeSocket.last!;
    ws.receive(JSON.stringify({ type: 'ready' }));
    session.run({ tool: 'cargo', args: ['build'] });
    ws.receive(JSON.stringify({ type: 'started', commandId: 'c1', tool: 'cargo', args: ['build'] }));
    ws.receive(JSON.stringify({ type: 'output', commandId: 'c1', stream: 'stdout', data: 'before close' }));
    serverClose(ws, 4401);
    ws.receive(JSON.stringify({ type: 'output', commandId: 'c1', stream: 'stdout', data: 'after close' }));
    ws.receive(JSON.stringify({ type: 'exit', commandId: 'c1', exitCode: 0, outcome: 'succeeded', truncated: false }));
    const data = events.flatMap(e => (e.type === 'output' ? [e.data] : []));
    expect(data).toEqual(['before close']);
    expect(events.some(e => e.type === 'exit')).toBe(false);
    expect(events.at(-1)).toEqual({ type: 'status', status: 'closed' });
    const before = calls.length;
    await expect(api.openTerminal('atlas')).rejects.toMatchObject({ code: 'unauthorized' });
    expect(calls).toHaveLength(before);
  });

  test('a 4401 on the console drops frames that arrive after it', async () => {
    const { api } = await streamingApi('/api/v1/vms/web-static/console');
    const stream = await api.openConsole('web-static');
    const frames: ConsoleFrame[] = [];
    stream.onFrame(f => frames.push(f));
    const ws = FakeSocket.last!;
    ws.receive(JSON.stringify({ type: 'frame', seq: 1, width: 640, height: 400, mime: 'image/png', capturedAt: '2026-11-23T12:00:01Z' }));
    serverClose(ws, 4401);
    ws.receive(new Blob([new Uint8Array([1])]));
    ws.receive(JSON.stringify({ type: 'frame', seq: 2, width: 640, height: 400, mime: 'image/png', capturedAt: '2026-11-23T12:00:02Z' }));
    ws.receive(new Blob([new Uint8Array([2])]));
    expect(frames).toHaveLength(0);
    expect(stream.status).toBe('closed');
  });

  test('a 4408 ticket rejection is reconnectable and keeps the session', async () => {
    const { api } = await streamingApi('/api/v1/hosts/atlas/terminal');
    const session = await api.openTerminal('atlas');
    const events: TerminalEvent[] = [];
    session.onEvent(e => events.push(e));
    serverClose(FakeSocket.last!, 4408);
    expect(events.find(e => e.type === 'closed')).toEqual({ type: 'closed', reason: 'server', message: 'The terminal could not be authorized (expired ticket or slow handshake). Reconnect to try again.' });
    await expect(api.openTerminal('atlas')).resolves.toBeTruthy();
  });
});

describe('round 3: strict console timestamps', () => {
  test.each(['2026-02-30T12:00:00Z', '2026-11-23T12:00Z', '2026-11-23T24:00:00Z', '2026-11-23T12:60:00Z', '2026-11-23T12:00:60Z', '2026-11-23T12:00:00+24:00', '2026-11-23 12:00:00Z', '2026-11-23T12:00:00'])('%s is refused', capturedAt => {
    expect(decodeFrameMeta({ seq: 1, width: 1, height: 1, mime: 'image/png', capturedAt }, 0)).toBeNull();
  });

  test('valid instants, including leap days, fractions and offsets, are accepted', () => {
    for (const capturedAt of ['2028-02-29T23:59:59Z', '2026-11-23T12:00:00.123456789Z', '2026-11-23T12:00:00-05:30']) {
      expect(decodeFrameMeta({ seq: 1, width: 1, height: 1, mime: 'image/png', capturedAt }, 0)).not.toBeNull();
    }
    expect(decodeFrameMeta({ seq: 1, width: 1, height: 1, mime: 'image/png', capturedAt: '2027-02-29T00:00:00Z' }, 0)).toBeNull();
  });
});
