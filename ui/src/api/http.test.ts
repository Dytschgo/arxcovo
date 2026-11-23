import { describe, expect, test } from 'vitest';
import { FIXTURE_HOSTS, FIXTURE_VMS } from './fixtures';
import { activitySearch, CSRF_HEADER, decodeFrameMeta, decodeTerminalMessage, HttpApi, readBounded } from './http';
import type { ConsoleFrame, TerminalEvent } from './types';

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
    });
    const api = new HttpApi({ fetch: fetchImpl });
    await api.login({ username: 'owner', password: 'pw' });
    await expect(api.logout()).rejects.toMatchObject({ code: 'unavailable', message: 'arxd is restarting' });
    await api.logout();
    const deletes = calls.filter(c => c.init.method === 'DELETE');
    expect(deletes).toHaveLength(2);
    expect(deletes.every(c => header(c.init, CSRF_HEADER) === SESSION.csrfToken)).toBe(true);
    await api.logout();
    expect(calls.filter(c => c.init.method === 'DELETE')).toHaveLength(2);
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
