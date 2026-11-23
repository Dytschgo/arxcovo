import http from 'node:http';

// Replays a session cookie against GET /api/v1/session over plain loopback
// HTTP, with no cookie jar. Node's client is used instead of Playwright's
// request API, whose call log records header values.
//
// Invariant: every failure, wherever it happens, surfaces only as a
// ReplayError whose message is one of two fixed strings and carries at most
// the HTTP status. Failure paths, all inside the single boundary below:
//   1. options: missing or invalid port;
//   2. construction: `request()` throwing synchronously (for example an
//      invalid header character);
//   3. connect: the request emitting `error` (refused, reset);
//   4. timeout: no response within `timeoutMs`;
//   5. status: any status other than 200 or 401;
//   6. body: a response that errors, aborts or exceeds the size limit;
//   7. parse: a 200 whose body is not JSON or not a session;
//   8. anything thrown by the callbacks themselves.
// The original error is always discarded because it may describe the
// request, including its headers.

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
        res => {
          seen.status = res.statusCode ?? null;
          const chunks: Buffer[] = [];
          let size = 0;
          res.on('data', (chunk: Buffer) => {
            size += chunk.length;
            if (size > BODY_LIMIT) res.destroy(new Error('size'));
            else chunks.push(chunk);
          });
          res.on('error', () => reject(new Error('body')));
          res.on('aborted', () => reject(new Error('aborted')));
          res.on('end', () => {
            try {
              if (seen.status === 401) return resolve(401);
              if (seen.status !== 200) throw new Error('status');
              const body: unknown = JSON.parse(Buffer.concat(chunks).toString('utf8'));
              const user = (body as { user?: { name?: unknown } } | null)?.user;
              if (typeof user?.name !== 'string') throw new Error('shape');
              resolve(200);
            } catch {
              reject(new Error('parse'));
            }
          });
        },
      );
      req.on('timeout', () => req.destroy(new Error('timeout')));
      req.on('error', () => reject(new Error('request')));
      req.end();
    });
  } catch {
    throw new ReplayError(seen.status);
  }
}
