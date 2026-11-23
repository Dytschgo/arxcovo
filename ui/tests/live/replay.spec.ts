import { expect, test } from '@playwright/test';
import { randomBytes } from 'node:crypto';
import { EventEmitter } from 'node:events';
import http from 'node:http';
import type { AddressInfo } from 'node:net';
import { REPLAY_REQUEST_FAILED, ReplayError, replaySessionStatus, replayStatusFailed, type ReplayOptions } from './replay';

// Invariant B for the live suite's cookie replay (replay.ts): every failure
// path produces only a fixed, redacted error carrying at most the status.
// Node-only (no browser); collected by the default smoke config, so it runs
// in CI on every OS without the live backend.

const COOKIE = '__Host-arx_session';
const VALUE = `cookie-sentinel-${randomBytes(12).toString('hex')}`;
const FORBIDDEN = [VALUE, COOKIE, 'cookie', 'Cookie', 'localhost:', 'x-echo'];

interface Fixture { port: number; close: () => Promise<void>; seen: http.IncomingHttpHeaders[] }

async function serve(handler: http.RequestListener): Promise<Fixture> {
  const seen: http.IncomingHttpHeaders[] = [];
  const server = http.createServer((req, res) => {
    seen.push(req.headers);
    handler(req, res);
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  const { port } = server.address() as AddressInfo;
  return {
    port,
    seen,
    close: () => new Promise<void>(resolve => {
      server.closeAllConnections();
      server.close(() => resolve());
    }),
  };
}

const options = (port: ReplayOptions['port'], extra: Partial<ReplayOptions> = {}): ReplayOptions =>
  ({ port, cookieName: COOKIE, value: VALUE, timeoutMs: 2_000, ...extra });

async function failure(promise: Promise<unknown>): Promise<unknown> {
  try {
    await promise;
  } catch (error) {
    return error;
  }
  throw new Error('the replay unexpectedly succeeded');
}

/** The only own properties a ReplayError may carry. */
const ALLOWED_KEYS = ['message', 'name', 'stack', 'status'];

const ownKeys = (value: object) => Reflect.ownKeys(value).map(String).sort();

/**
 * Every key and primitive value reachable from `value` through own
 * properties (enumerable or not, string or symbol keys), at any depth, plus
 * its string form. No key whitelist is applied, so a marker nested under
 * `details.headers.cookie` is found.
 */
function reachableText(value: unknown, seen = new Set<unknown>()): string[] {
  if (value === null || value === undefined) return [];
  if (typeof value !== 'object' && typeof value !== 'function') return [String(value)];
  if (seen.has(value)) return [];
  seen.add(value);
  const out: string[] = [];
  try {
    out.push(String(value));
  } catch {
    out.push('<unprintable>');
  }
  for (const key of Reflect.ownKeys(value)) {
    out.push(String(key));
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (descriptor && 'value' in descriptor) {
      out.push(...reachableText(descriptor.value, seen));
    } else {
      // Accessors (V8 may expose `stack` this way) are read, not skipped.
      try {
        out.push(...reachableText(Reflect.get(value, key), seen));
      } catch {
        out.push('<throwing accessor>');
      }
    }
  }
  return out;
}

/** Indexes of the forbidden markers found anywhere in the error. */
const forbiddenIn = (error: unknown) => {
  const text = reachableText(error).join('\n');
  return FORBIDDEN.flatMap((marker, index) => (text.includes(marker) ? [index] : []));
};

/** Asserts with key names, indexes and fixed strings only, so nothing secret prints. */
function expectRedacted(error: unknown, status: number | null) {
  expect(error instanceof ReplayError).toBe(true);
  const replay = error as ReplayError;
  expect(ownKeys(replay)).toEqual(ALLOWED_KEYS);
  expect(forbiddenIn(replay)).toEqual([]);
  expect(replay.message).toBe(status === null ? REPLAY_REQUEST_FAILED : replayStatusFailed(status));
  expect(replay.status).toBe(status);
}

/**
 * Runs `body` and then waits for late callbacks, failing if any exception
 * or rejection escaped out of band. Only the count is asserted.
 */
async function withoutStrayErrors<T>(body: () => Promise<T>): Promise<T> {
  const strays: unknown[] = [];
  const trap = (error: unknown) => strays.push(error);
  process.on('uncaughtException', trap);
  process.on('unhandledRejection', trap);
  try {
    const result = await body();
    await new Promise(resolve => setTimeout(resolve, 100));
    return result;
  } finally {
    process.off('uncaughtException', trap);
    process.off('unhandledRejection', trap);
    expect(strays.length, 'exceptions escaped the replay boundary').toBe(0);
  }
}

/** A scripted stand-in for node:http's ClientRequest. */
class FakeRequest extends EventEmitter {
  constructor(readonly headers: http.OutgoingHttpHeaders | undefined) {
    super();
  }
  destroy(): this {
    throw new Error(`destroy failed ${JSON.stringify(this.headers)}`);
  }
  end(): this {
    return this;
  }
}

/** A fake response whose methods echo the request headers when they throw. */
function fakeResponse(headers: http.OutgoingHttpHeaders | undefined, status: number | (() => number)) {
  const response = new EventEmitter() as EventEmitter & { statusCode?: number; destroy: () => void };
  if (typeof status === 'function') Object.defineProperty(response, 'statusCode', { get: status });
  else response.statusCode = status;
  response.destroy = () => {
    throw new Error(`destroy failed ${JSON.stringify(headers)}`);
  };
  return response;
}

/** A request() seam that hands the script the fake request and the response callback. */
function scripted(script: (req: FakeRequest, respond: (res: unknown) => void) => void): typeof http.request {
  return ((opts: http.RequestOptions, callback: (res: unknown) => void) => {
    const req = new FakeRequest(opts.headers);
    setTimeout(() => script(req, callback), 5);
    return req;
  }) as unknown as typeof http.request;
}

test.describe('cookie replay redaction', () => {
  test('control: sends only the cookie and host, returns 200 or 401', async () => {
    const live = await serve((_req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ user: { name: 'owner' }, csrfToken: 'x'.repeat(32), expiresAt: '2026-11-24T00:00:00Z' }));
    });
    try {
      expect(await replaySessionStatus(options(live.port))).toBe(200);
      expect(live.seen[0].cookie === `${COOKIE}=${VALUE}`).toBe(true);
      expect(live.seen[0].host).toBe(`localhost:${live.port}`);
    } finally {
      await live.close();
    }
    const refused = await serve((_req, res) => {
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end('{"error":{"code":"unauthorized","message":"Sign in to continue."}}');
    });
    try {
      expect(await replaySessionStatus(options(refused.port))).toBe(401);
    } finally {
      await refused.close();
    }
  });

  test('options: missing or invalid port', async () => {
    for (const port of [undefined, '', 'abc', 0, -1, 1.5, 70_000]) expectRedacted(await failure(replaySessionStatus(options(port))), null);
  });

  test('construction: synchronous throws from request()', async () => {
    // Real: Node refuses a header with a line break and names the header.
    expectRedacted(await failure(replaySessionStatus(options(1, { value: `${VALUE}\r\nx-echo: 1` }))), null);
    // Injected: a request() whose own error message echoes every header.
    const echo = ((opts: http.RequestOptions) => {
      throw new Error(`refused ${JSON.stringify(opts.headers)}`);
    }) as unknown as typeof http.request;
    expectRedacted(await failure(replaySessionStatus(options(1, { request: echo }))), null);
    // Injected: the returned request object throws while being wired up.
    const broken = ((opts: http.RequestOptions) => ({
      on: () => {
        throw new Error(`listener ${JSON.stringify(opts.headers)}`);
      },
      end: () => undefined,
    })) as unknown as typeof http.request;
    expectRedacted(await failure(replaySessionStatus(options(1, { request: broken }))), null);
  });

  test('connect: refused connection', async () => {
    const closed = await serve(() => undefined);
    const port = closed.port;
    await closed.close();
    expectRedacted(await failure(replaySessionStatus(options(port))), null);
  });

  test('timeout: no response', async () => {
    const silent = await serve(() => undefined);
    try {
      expectRedacted(await failure(replaySessionStatus(options(silent.port, { timeoutMs: 200 }))), null);
    } finally {
      await silent.close();
    }
  });

  test('status: any status other than 200 or 401, even one echoing the request', async () => {
    for (const status of [204, 302, 403, 500, 503]) {
      const echo = await serve((req, res) => {
        res.writeHead(status, { 'x-echo': String(req.headers.cookie) });
        res.end(`echo ${req.headers.cookie}`);
      });
      try {
        expectRedacted(await failure(replaySessionStatus(options(echo.port))), status);
      } finally {
        await echo.close();
      }
    }
  });

  test('body: aborted and oversized responses', async () => {
    const aborted = await serve((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json', 'content-length': '1000' });
      res.write(`{"echo":"${req.headers.cookie}`);
      setTimeout(() => res.socket?.destroy(), 20);
    });
    try {
      expectRedacted(await failure(replaySessionStatus(options(aborted.port))), 200);
    } finally {
      await aborted.close();
    }
    const huge = await serve((req, res) => {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(`{"user":{"name":"${'x'.repeat(70_000)}"},"echo":"${req.headers.cookie}"}`);
    });
    try {
      expectRedacted(await failure(replaySessionStatus(options(huge.port))), 200);
    } finally {
      await huge.close();
    }
  });

  test('parse: a 200 that is not JSON or not a session', async () => {
    for (const body of [(cookie: string) => `not json ${cookie}`, () => '{}', () => 'null', (cookie: string) => `{"user":{"name":7},"echo":"${cookie}"}`]) {
      const bad = await serve((req, res) => {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(body(String(req.headers.cookie)));
      });
      try {
        expectRedacted(await failure(replaySessionStatus(options(bad.port))), 200);
      } finally {
        await bad.close();
      }
    }
  });

  test('assertion probe: a marker nested under details.headers.cookie is caught', async () => {
    const probe = new ReplayError(null);
    Object.assign(probe, { details: { headers: { cookie: `${COOKIE}=${VALUE}` } } });
    // The previous top-level replacer missed it.
    expect(JSON.stringify(probe, Object.getOwnPropertyNames(probe)).includes(VALUE)).toBe(false);
    expect(forbiddenIn(probe)).toContain(0);
    expect(ownKeys(probe)).not.toEqual(ALLOWED_KEYS);
    const hidden = new ReplayError(500);
    Object.defineProperty(hidden, Symbol('raw'), { value: { request: [VALUE] }, enumerable: false });
    expect(forbiddenIn(hidden)).toContain(0);
    const caused = new ReplayError(null);
    Object.defineProperty(caused, 'cause', { value: new Error(`cookie ${VALUE}`), enumerable: false });
    expect(forbiddenIn(caused)).toContain(0);
  });

  test('async: a timeout whose destroy() throws', async () => {
    const request = scripted(req => req.emit('timeout'));
    const error = await withoutStrayErrors(() => failure(replaySessionStatus(options(1, { request }))));
    expectRedacted(error, null);
  });

  test('async: the response callback throws', async () => {
    const request = scripted((req, respond) =>
      respond(fakeResponse(req.headers, () => {
        throw new Error(`status read ${JSON.stringify(req.headers)}`);
      })));
    const error = await withoutStrayErrors(() => failure(replaySessionStatus(options(1, { request }))));
    expectRedacted(error, null);
  });

  test('async: the data listener throws and destroy() throws too', async () => {
    const request = scripted((req, respond) => {
      const res = fakeResponse(req.headers, 200);
      respond(res);
      const poisoned = Object.defineProperty({}, 'length', {
        get: () => {
          throw new Error(`chunk ${JSON.stringify(req.headers)}`);
        },
      });
      setTimeout(() => res.emit('data', poisoned), 5);
    });
    const error = await withoutStrayErrors(() => failure(replaySessionStatus(options(1, { request }))));
    expectRedacted(error, 200);
  });

  test('async: an oversized body whose destroy() throws', async () => {
    const request = scripted((req, respond) => {
      const res = fakeResponse(req.headers, 200);
      respond(res);
      setTimeout(() => res.emit('data', Buffer.alloc(70_000, 120)), 5);
    });
    const error = await withoutStrayErrors(() => failure(replaySessionStatus(options(1, { request }))));
    expectRedacted(error, 200);
  });

  test('async: late events after the result are absorbed', async () => {
    const request = scripted((req, respond) => {
      const res = fakeResponse(req.headers, 401);
      respond(res);
      res.emit('end');
      setTimeout(() => {
        req.emit('error', new Error(`late ${JSON.stringify(req.headers)}`));
        req.emit('timeout');
        res.emit('error', new Error(`late ${JSON.stringify(req.headers)}`));
      }, 10);
    });
    expect(await withoutStrayErrors(() => replaySessionStatus(options(1, { request })))).toBe(401);
  });
});
