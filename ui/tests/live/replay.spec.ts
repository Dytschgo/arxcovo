import { expect, test } from '@playwright/test';
import { randomBytes } from 'node:crypto';
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

/** Asserts with booleans and fixed strings only, so nothing secret prints. */
function expectRedacted(error: unknown, status: number | null) {
  expect(error instanceof ReplayError).toBe(true);
  const replay = error as ReplayError;
  const text = [replay.message, String(replay), replay.stack ?? '', JSON.stringify(replay, Object.getOwnPropertyNames(replay))].join('\n');
  for (const secret of FORBIDDEN) expect(text.includes(secret), `error text contains a forbidden marker #${FORBIDDEN.indexOf(secret)}`).toBe(false);
  expect(replay.message).toBe(status === null ? REPLAY_REQUEST_FAILED : replayStatusFailed(status));
  expect(replay.status).toBe(status);
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
});
