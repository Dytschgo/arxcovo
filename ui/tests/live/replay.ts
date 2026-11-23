import http from 'node:http';

// Replays a session cookie against GET /api/v1/session over plain loopback
// HTTP, with no cookie jar. Node's client is used instead of Playwright's
// request API, whose call log records header values.
//
// Invariant: every failure, wherever and whenever it happens, surfaces only
// as a ReplayError whose message is one of two fixed strings and carries at
// most the HTTP status. Failure paths:
//   1. options: missing or invalid port;
//   2. construction: `request()` throwing synchronously (for example an
//      invalid header character);
//   3. wiring: the request object throwing while listeners are attached;
//   4. connect: the request emitting `error` (refused, reset);
//   5. timeout: no response within `timeoutMs`, including a `destroy()`
//      that throws;
//   6. status: any status other than 200 or 401;
//   7. body: a response that errors, aborts or exceeds the size limit;
//   8. parse: a 200 whose body is not JSON or not a session;
//   9. any callback (response, data, end, error, timeout) throwing later,
//      asynchronously.
// Paths 1–3 throw inside the Promise executor; every later callback runs
// through `guard`, which turns a throw into a rejection of the same Promise
// instead of letting it escape as an uncaught exception. The single
// try/catch then discards the original error, which may describe the
// request and its headers.

export const REPLAY_REQUEST_FAILED = 'Session replay failed: request failed.';
export const replayStatusFailed = (status: number) => `Session replay failed: HTTP ${status}.`;

export class ReplayError extends Error {
  readonly status: number | null;
  constructor(status: number | null) {
    super(status === null ? REPLAY_REQUEST_FAILED : replayStatusFailed(status));
    this.name = 'ReplayError';
    this.status = status;
  }
}

export interface ReplayOptions {
  port: number | string | undefined;
  cookieName: string;
  value: string;
  timeoutMs?: number;
  /** Test seam for failure injection; defaults to node:http's request. */
  request?: typeof http.request;
}

const BODY_LIMIT = 64 * 1024;

/**
 * Resolves 200 for a live session (with a session-shaped body) or 401 for a
 * refused cookie; every other outcome rejects with a ReplayError.
 */
export async function replaySessionStatus(options: ReplayOptions): Promise<200 | 401> {
  const seen: { status: number | null } = { status: null };
  try {
    return await new Promise<200 | 401>((resolve, reject) => {
      // A holder rather than `let`s, so `fail` is safe at any moment. An
      // emitter is recorded only once its `error` listener is attached, so
      // `fail` never destroys (and so never provokes an error from) an
      // emitter that could not report it to us.
      const exchange: { req?: http.ClientRequest; res?: http.IncomingMessage; failed: boolean } = { failed: false };
      // Settle as failed once, then stop the exchange. Stopping is best
      // effort: its own errors are dropped because the outcome is decided,
      // and late events from it reach the guarded listeners, which ignore
      // them because `failed` is already set.
      const fail = () => {
        if (exchange.failed) return;
        exchange.failed = true;
        reject(new Error('replay'));
        try {
          exchange.res?.destroy();
        } catch {
          // dropped: may describe the request
        }
        try {
          exchange.req?.destroy();
        } catch {
          // dropped: may describe the request
        }
      };
      // Every callback goes through this, so nothing can throw out of band.
      const guard = <A extends unknown[]>(callback: (...args: A) => void) => (...args: A): void => {
        try {
          callback(...args);
        } catch {
          fail();
        }
      };

      const port = Number(options.port);
      if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('port');
      const send = options.request ?? http.request;
      const req = send(
        {
          host: '127.0.0.1',
          port,
          path: '/api/v1/session',
          method: 'GET',
          timeout: options.timeoutMs ?? 10_000,
          headers: { host: `localhost:${port}`, cookie: `${options.cookieName}=${options.value}` },
        },
        guard((response: http.IncomingMessage) => {
          // Every listener is attached before anything that can throw (such
          // as reading `statusCode`) runs, starting with `error`.
          const chunks: Buffer[] = [];
          let size = 0;
          response.on('error', guard(fail));
          exchange.res = response;
          response.on('aborted', guard(fail));
          response.on('data', guard((chunk: Buffer) => {
            size += chunk.length;
            if (size > BODY_LIMIT) fail();
            else chunks.push(chunk);
          }));
          response.on('end', guard(() => {
            if (seen.status === 401) return resolve(401);
            if (seen.status !== 200) return fail();
            const body: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'));
            const user = (body as { user?: { name?: unknown } } | null)?.user;
            if (typeof user?.name !== 'string') return fail();
            resolve(200);
          }));
          seen.status = response.statusCode ?? null;
        }),
      );
      // Request listeners are attached before anything else runs, `error`
      // first; nothing that can throw comes between construction and them.
      req.on('error', guard(fail));
      exchange.req = req;
      req.on('timeout', guard(fail));
      req.end();
    });
  } catch {
    throw new ReplayError(seen.status);
  }
}
