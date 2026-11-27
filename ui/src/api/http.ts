// HTTP + WebSocket adapter for the dashboard API (docs/dashboard-api.md).
// The VM decoder accepts null for metadata that older catalogs did not record.
// The session lives in an httpOnly cookie the page cannot read; the CSRF token
// lives only in this object's memory and is sent on every unsafe request.
import { decodeActivityPage, decodeCpuHistory, decodeCpuHistoryBatch, decodeHost, decodeHosts, decodeSession, decodeVm, decodeVms, isIsoInstant, isValidId } from './decode';
import { Emitter } from './emitter';
import {
  ACTIVITY_OUTCOMES,
  ApiError,
  type ActivityOutcome,
  type ActivityPage,
  type ActivityQuery,
  type ApiErrorCode,
  type ArxApi,
  type ConsoleFrame,
  type ConsoleStatus,
  type ConsoleStream,
  type Credentials,
  CPU_HISTORY_RANGES,
  type CpuHistory,
  type CpuHistoryRange,
  type ReadOptions,
  type Host,
  type Session,
  STREAM_CLOSE_CODES,
  type StreamCloseReason,
  type TerminalCommand,
  type TerminalEvent,
  type TerminalSession,
  type TerminalStatus,
  type Vm,
} from './types';

export const API_PREFIX = '/api/v1';
export const CSRF_HEADER = 'X-Arx-CSRF';
const MAX_BODY_BYTES = 2 * 1024 * 1024;
const MAX_WS_TEXT = 128 * 1024;
const MAX_FRAME_BYTES = 8 * 1024 * 1024;

type WebSocketCtor = new (url: string) => WebSocket;

export interface HttpApiOptions {
  /** Origin the dashboard was served from; '' means same origin. */
  baseUrl?: string;
  fetch?: typeof fetch;
  WebSocket?: WebSocketCtor;
  /** Used to resolve relative URLs; defaults to window.location.href. */
  pageUrl?: string;
}

const STATUS_CODES: Record<number, ApiErrorCode> = { 400: 'invalid_request', 401: 'unauthorized', 403: 'forbidden', 404: 'not_found', 422: 'invalid_request', 429: 'rate_limited' };

const DEFAULT_MESSAGES: Record<ApiErrorCode, string> = {
  unauthorized: 'Your session has ended. Sign in again.',
  forbidden: 'The server refused this request.',
  not_found: 'Not found.',
  invalid_request: 'The server rejected the request.',
  rate_limited: 'Too many attempts. Wait a moment and try again.',
  unavailable: 'arxd is unavailable right now.',
  bad_response: 'The server sent a response the dashboard does not understand.',
  network: 'Cannot reach arxd.',
};

function errorFor(status: number, body: unknown): ApiError {
  const code = STATUS_CODES[status] ?? (status >= 500 ? 'unavailable' : 'invalid_request');
  let message = DEFAULT_MESSAGES[code];
  if (typeof body === 'object' && body !== null && 'error' in body) {
    const e = (body as { error: unknown }).error;
    if (typeof e === 'object' && e !== null && typeof (e as { message?: unknown }).message === 'string') {
      const m = (e as { message: string }).message;
      if (m.length > 0 && m.length <= 200) message = m;
    }
  }
  return new ApiError(code, message, status);
}

/**
 * Reads a response body as UTF-8, counting bytes as they stream in and
 * cancelling the stream as soon as it exceeds `limit`, whether or not a
 * Content-Length header is present.
 */
/** The rejection for an aborted request: the signal's reason when it is an ApiError, else a cancellation. */
export function abortError(signal: AbortSignal): ApiError {
  return signal.reason instanceof ApiError ? signal.reason : new ApiError('network', 'The request was cancelled.');
}

/**
 * Reads at most `limit` bytes of the body. With a `signal`, an abort cancels
 * the reader (so a stalled body read ends too) and rejects with the reason.
 */
export async function readBounded(response: Response, limit: number, signal?: AbortSignal): Promise<string> {
  if (signal?.aborted) {
    void response.body?.cancel().catch(() => undefined);
    throw abortError(signal);
  }
  const declared = Number(response.headers.get('Content-Length'));
  if (Number.isFinite(declared) && declared > limit) {
    void response.body?.cancel().catch(() => undefined);
    throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response, response.status);
  }
  if (!response.body) return '';
  const reader = response.body.getReader();
  const decoder = new TextDecoder('utf-8', { fatal: true });
  let total = 0;
  let text = '';
  const onAbort = () => void reader.cancel().catch(() => undefined);
  signal?.addEventListener('abort', onAbort, { once: true });
  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (signal?.aborted) throw abortError(signal);
      if (done) break;
      total += value.byteLength;
      if (total > limit) {
        void reader.cancel().catch(() => undefined);
        throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response, response.status);
      }
      text += decoder.decode(value, { stream: true });
    }
    return text + decoder.decode();
  } catch (error) {
    if (signal?.aborted) throw abortError(signal);
    if (error instanceof ApiError) throw error;
    throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response, response.status);
  } finally {
    signal?.removeEventListener('abort', onAbort);
  }
}

function pathId(id: string): string {
  if (!isValidId(id)) throw new ApiError('invalid_request', 'Invalid identifier.');
  return encodeURIComponent(id);
}

export function activitySearch(query: ActivityQuery = {}): string {
  const params = new URLSearchParams();
  const limit = query.limit ?? 20;
  if (!Number.isInteger(limit) || limit < 1 || limit > 50) throw new ApiError('invalid_request', 'Activity limit must be 1 to 50.');
  params.set('limit', String(limit));
  if (query.beforeId !== undefined) {
    if (!Number.isInteger(query.beforeId) || query.beforeId < 1) throw new ApiError('invalid_request', 'Invalid activity cursor.');
    params.set('before_id', String(query.beforeId));
  }
  if (query.outcome !== undefined) {
    if (!ACTIVITY_OUTCOMES.includes(query.outcome)) throw new ApiError('invalid_request', 'Invalid outcome filter.');
    params.set('outcome', query.outcome);
  }
  if (query.tool) {
    if (!/^[a-z_]{1,64}$/.test(query.tool)) throw new ApiError('invalid_request', 'Invalid tool filter.');
    params.set('tool', query.tool);
  }
  if (query.hostId) params.set('host_id', pathId(query.hostId));
  if (query.includeReads) params.set('include_reads', 'true');
  return params.toString();
}

export class HttpApi implements ArxApi {
  readonly kind = 'http' as const;
  private csrfToken: string | null = null;
  /** Bumps whenever the CSRF token changes, so a stream knows its session. */
  private generation = 0;
  private readonly baseUrl: string;
  private readonly fetchImpl: typeof fetch;
  private readonly WebSocketImpl: WebSocketCtor | undefined;
  private readonly pageUrl: string;

  constructor(options: HttpApiOptions = {}) {
    this.baseUrl = options.baseUrl ?? '';
    this.fetchImpl = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
    this.WebSocketImpl = options.WebSocket ?? (globalThis.WebSocket as WebSocketCtor | undefined);
    this.pageUrl = options.pageUrl ?? (typeof window === 'undefined' ? 'http://127.0.0.1/' : window.location.href);
  }

  /**
   * `expectStatus` pins the exact success status; any other 2xx is treated as
   * unconfirmed and rejects, so callers never mistake 200/202 for 204.
   */
  private async request(method: 'GET' | 'POST' | 'DELETE', path: string, body?: unknown, expectStatus?: number, signal?: AbortSignal): Promise<unknown> {
    // The session this request is sent under, read with its CSRF token.
    const dispatched = this.generation;
    const headers: Record<string, string> = { Accept: 'application/json' };
    if (method !== 'GET') {
      if (path !== `${API_PREFIX}/session` || method !== 'POST') {
        if (!this.csrfToken) throw new ApiError('unauthorized', DEFAULT_MESSAGES.unauthorized);
        headers[CSRF_HEADER] = this.csrfToken;
      }
    }
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    let response: Response;
    try {
      response = await this.fetchImpl(`${this.baseUrl}${path}`, {
        method,
        headers,
        body: body === undefined ? undefined : JSON.stringify(body),
        credentials: 'same-origin',
        cache: 'no-store',
        redirect: 'error',
        ...(signal ? { signal } : {}),
      });
    } catch {
      if (signal?.aborted) throw abortError(signal);
      throw new ApiError('network', DEFAULT_MESSAGES.network);
    }
    let text: string;
    try {
      text = await readBounded(response, MAX_BODY_BYTES, signal);
    } catch (error) {
      // An unreadable error body still yields the status-based error below.
      if (response.ok || signal?.aborted) throw error;
      text = '';
    }
    let parsed: unknown = null;
    if (text.length > 0) {
      try {
        parsed = JSON.parse(text);
      } catch {
        if (response.ok) throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response, response.status);
      }
    }
    if (!response.ok) {
      if (response.status === 401) {
        // A 401 ends only the session the request was sent under. When a
        // newer sign-in has replaced it, the newer session is untouched and
        // the request fails without telling the view to end that session.
        if (dispatched !== this.generation && this.csrfToken !== null) {
          throw new ApiError('unavailable', 'The request was sent under an earlier session. Try again.', 401);
        }
        this.setToken(null);
      }
      throw errorFor(response.status, parsed);
    }
    if (expectStatus !== undefined && response.status !== expectStatus) {
      throw new ApiError('unavailable', `The server did not confirm the request (HTTP ${response.status}).`, response.status);
    }
    return parsed;
  }

  async getSession(): Promise<Session | null> {
    try {
      const session = decodeSession(await this.request('GET', `${API_PREFIX}/session`));
      this.setToken(session.csrfToken);
      return session;
    } catch (error) {
      if (error instanceof ApiError && error.code === 'unauthorized' && error.status === 401) return null;
      throw error;
    }
  }

  async login(credentials: Credentials): Promise<Session> {
    const session = decodeSession(await this.request('POST', `${API_PREFIX}/session`, { username: credentials.username, password: credentials.password }));
    this.setToken(session.csrfToken);
    return session;
  }

  /**
   * Revokes the session. Resolves only on exactly 204 (revoked) or 401
   * (already gone). Every other answer, including 200 and 202, rejects and
   * keeps the CSRF token so a retry sends DELETE again.
   *
   * Without a CSRF token (after a 401, a session-ended stream or an earlier
   * sign-out) it never assumes the cookie is gone: it re-reads the session
   * first. A 401 there confirms there is nothing to revoke; a live session is
   * revoked with DELETE as above, and a failure rejects.
   *
   * Only an actual HTTP 401 response confirms. A DELETE that could not be
   * sent (no token by the time it is dispatched) rejects, so the shell keeps
   * the session visible and offers the retry.
   *
   * The 204 ends only the session the DELETE was sent under. If a newer
   * sign-in completed while it was in flight, that session's token is kept.
   */
  async logout(): Promise<void> {
    if (!this.csrfToken && (await this.getSession()) === null) return;
    // The session this DELETE is sent under; request() dispatches it in this
    // same tick, so it matches the generation request() reads.
    const sent = this.generation;
    try {
      await this.request('DELETE', `${API_PREFIX}/session`, undefined, 204);
    } catch (error) {
      if (error instanceof ApiError && error.code === 'unauthorized') {
        if (error.status === 401) return;
        throw new ApiError('unavailable', 'The sign-out request could not be sent. Try again.');
      }
      throw error;
    }
    if (this.generation === sent) this.setToken(null);
  }

  private setToken(token: string | null): void {
    if (token === this.csrfToken) return;
    this.csrfToken = token;
    this.generation += 1;
  }

  /**
   * Binds a stream's 4401 to the session it is opened under. The returned
   * callback ends the local session (forgets the token) and returns true only
   * if that session is still the current one, or no session is held. A late
   * 4401 from a stream of an earlier session returns false and leaves a newer
   * session untouched.
   */
  private sessionEnder(): () => boolean {
    const opened = this.generation;
    return () => {
      if (opened !== this.generation && this.csrfToken !== null) return false;
      this.setToken(null);
      return true;
    };
  }

  async listHosts(): Promise<Host[]> {
    return decodeHosts(await this.request('GET', `${API_PREFIX}/hosts`));
  }

  async getHost(id: string): Promise<Host> {
    return decodeHost(await this.request('GET', `${API_PREFIX}/hosts/${pathId(id)}`));
  }

  async getCpuHistory(hostId: string, range: CpuHistoryRange, options: ReadOptions = {}): Promise<CpuHistory> {
    if (!Object.hasOwn(CPU_HISTORY_RANGES, range)) throw new ApiError('invalid_request', 'Invalid CPU history range.');
    const history = decodeCpuHistory(await this.request('GET', `${API_PREFIX}/hosts/${pathId(hostId)}/cpu-history?range=${range}`, undefined, undefined, options.signal));
    if (history.hostId !== hostId || history.range !== range) {
      throw new ApiError('bad_response', 'CPU history did not match the requested host and range.');
    }
    return history;
  }

  async listCpuHistory(range: '24h', options: ReadOptions = {}): Promise<CpuHistory[]> {
    if (range !== '24h') throw new ApiError('invalid_request', 'The CPU history batch requires a 24h range.');
    return decodeCpuHistoryBatch(await this.request('GET', `${API_PREFIX}/hosts/cpu-history?range=${range}`, undefined, undefined, options.signal));
  }

  async listActivity(query: ActivityQuery = {}): Promise<ActivityPage> {
    return decodeActivityPage(await this.request('GET', `${API_PREFIX}/activity?${activitySearch(query)}`));
  }

  async listVms(): Promise<Vm[]> {
    return decodeVms(await this.request('GET', `${API_PREFIX}/vms`));
  }

  async getVm(id: string): Promise<Vm> {
    return decodeVm(await this.request('GET', `${API_PREFIX}/vms/${pathId(id)}`));
  }

  private async ticket(path: string): Promise<{ url: string; ticket: string }> {
    const body = await this.request('POST', path);
    if (typeof body !== 'object' || body === null) throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response);
    const { streamPath, ticket } = body as { streamPath?: unknown; ticket?: unknown };
    if (typeof streamPath !== 'string' || !/^\/api\/v1\/streams\/[A-Za-z0-9_-]{1,128}$/.test(streamPath)) throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response);
    if (typeof ticket !== 'string' || ticket.length < 16 || ticket.length > 256) throw new ApiError('bad_response', DEFAULT_MESSAGES.bad_response);
    return { url: this.wsUrl(streamPath), ticket };
  }

  wsUrl(path: string): string {
    const url = new URL(`${this.baseUrl}${path}`, this.pageUrl);
    if (url.protocol === 'https:') url.protocol = 'wss:';
    else if (url.protocol === 'http:') url.protocol = 'ws:';
    else throw new ApiError('network', 'The dashboard origin cannot open a stream.');
    return url.toString();
  }

  private socket(url: string): WebSocket {
    if (!this.WebSocketImpl) throw new ApiError('network', 'WebSocket is not available.');
    return new this.WebSocketImpl(url);
  }

  // The session is captured before the ticket request, which carries its token.
  async openTerminal(hostId: string): Promise<TerminalSession> {
    const ended = this.sessionEnder();
    const { url, ticket } = await this.ticket(`${API_PREFIX}/hosts/${pathId(hostId)}/terminal`);
    return new WsTerminalSession(this.socket(url), ticket, ended);
  }

  async openConsole(vmId: string): Promise<ConsoleStream> {
    const ended = this.sessionEnder();
    const { url, ticket } = await this.ticket(`${API_PREFIX}/vms/${pathId(vmId)}/console`);
    return new WsConsoleStream(this.socket(url), ticket, ended);
  }
}

function parseText(data: unknown): Record<string, unknown> | null {
  if (typeof data !== 'string' || data.length > MAX_WS_TEXT) return null;
  try {
    const v: unknown = JSON.parse(data);
    return typeof v === 'object' && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : null;
  } catch {
    return null;
  }
}

/** Decodes one server→client terminal message; null for anything malformed. */
export function decodeTerminalMessage(data: unknown): TerminalEvent | null {
  const m = parseText(data);
  if (!m) return null;
  const cid = typeof m.commandId === 'string' && m.commandId.length <= 64 ? m.commandId : null;
  switch (m.type) {
    case 'ready':
      return { type: 'status', status: 'ready' };
    case 'started':
      if (!cid || typeof m.tool !== 'string' || !Array.isArray(m.args) || !m.args.every(a => typeof a === 'string')) return null;
      return { type: 'started', commandId: cid, tool: m.tool, args: m.args as string[] };
    case 'output':
      if (!cid || (m.stream !== 'stdout' && m.stream !== 'stderr') || typeof m.data !== 'string') return null;
      return { type: 'output', commandId: cid, stream: m.stream, data: m.data };
    case 'exit':
      if (!cid || !(m.exitCode === null || Number.isInteger(m.exitCode)) || !ACTIVITY_OUTCOMES.includes(m.outcome as ActivityOutcome)) return null;
      return { type: 'exit', commandId: cid, exitCode: m.exitCode as number | null, outcome: m.outcome as ActivityOutcome, truncated: m.truncated === true };
    case 'error':
      if (typeof m.code !== 'string' || typeof m.message !== 'string') return null;
      return { type: 'error', code: m.code.slice(0, 64), message: m.message.slice(0, 200) };
    default:
      return null;
  }
}

const MAX_CLOSE_REASON = 123;

/** A 4401 for the session a stream was opened under, after a newer sign-in. */
function earlierSessionClosed(what: string): { reason: StreamCloseReason; message: string } {
  return { reason: 'server', message: `The ${what} closed because the session it was opened under ended.` };
}

function closeMessage(what: string, event: CloseEvent): { reason: StreamCloseReason; message: string } {
  if (event.code === STREAM_CLOSE_CODES.sessionEnded) return { reason: 'session_ended', message: `Your session ended, so the server closed the ${what}.` };
  const why = typeof event.reason === 'string' ? event.reason.slice(0, MAX_CLOSE_REASON).trim() : '';
  if (event.code === STREAM_CLOSE_CODES.ticketRejected) return { reason: 'server', message: `The ${what} could not be authorized (expired ticket or slow handshake). Reconnect to try again.` };
  if (event.code === STREAM_CLOSE_CODES.forbidden) return { reason: 'server', message: `The server revoked access to this ${what}${why ? `: ${why}` : '.'}` };
  return { reason: 'server', message: `The server closed the ${what}${why ? `: ${why}` : '.'}` };
}

class WsTerminalSession implements TerminalSession {
  private current: TerminalStatus = 'connecting';
  private readonly events = new Emitter<[TerminalEvent]>();
  private closedByClient = false;

  constructor(private readonly ws: WebSocket, ticket: string, private readonly onSessionEnded: () => boolean = () => true) {
    ws.addEventListener('open', () => ws.send(JSON.stringify({ type: 'auth', ticket })));
    ws.addEventListener('message', (e: MessageEvent) => {
      if (this.current === 'closed' || this.current === 'error') return;
      const event = decodeTerminalMessage(e.data);
      if (!event) return this.fail('bad_message', 'The terminal stream sent an unreadable message.');
      if (event.type === 'status') this.setStatus(event.status);
      else {
        if (event.type === 'started') this.setStatus('running');
        this.events.emit(event);
        if (event.type === 'exit') this.setStatus('ready');
      }
    });
    ws.addEventListener('error', () => this.fail('network', 'The terminal connection failed.'));
    ws.addEventListener('close', (e: Event) => {
      // Server-initiated close: say why, and let the view end a dead session.
      // A 4401 for the current session always ends it, even after a
      // client-side failure already closed the view. A 4401 for an earlier
      // session only closes this stream.
      let closed = closeMessage('terminal', e as CloseEvent);
      if (closed.reason === 'session_ended' && !this.onSessionEnded()) closed = earlierSessionClosed('terminal');
      if (closed.reason !== 'session_ended' && (this.closedByClient || this.current === 'error' || this.current === 'closed')) return;
      this.events.emit({ type: 'closed', ...closed });
      this.setStatus('closed');
    });
  }

  get status(): TerminalStatus {
    return this.current;
  }

  private setStatus(status: TerminalStatus): void {
    if (this.current === status) return;
    this.current = status;
    this.events.emit({ type: 'status', status });
  }

  private fail(code: string, message: string): void {
    if (this.current === 'error' || this.current === 'closed') return;
    this.events.emit({ type: 'error', code, message });
    this.setStatus('error');
    this.closedByClient = true;
    this.ws.close();
  }

  onEvent(listener: (event: TerminalEvent) => void): () => void {
    return this.events.on(listener);
  }

  run(command: TerminalCommand): void {
    if (this.current !== 'ready') throw new ApiError('invalid_request', 'The terminal is not ready for a command.');
    this.ws.send(JSON.stringify({ type: 'run', tool: command.tool, args: command.args, timeoutS: command.timeoutS }));
  }

  cancel(): void {
    if (this.current === 'running') this.ws.send(JSON.stringify({ type: 'cancel' }));
  }

  close(): void {
    this.closedByClient = true;
    this.ws.close();
    this.setStatus('closed');
    this.events.clear();
  }
}

const MAX_FRAME_SIDE = 8192;

/** Validates console frame metadata; sequence numbers must strictly increase. */
export function decodeFrameMeta(m: Record<string, unknown>, lastSeq: number): Omit<ConsoleFrame, 'data'> | null {
  const side = (v: unknown) => Number.isInteger(v) && (v as number) >= 1 && (v as number) <= MAX_FRAME_SIDE;
  if (!Number.isSafeInteger(m.seq) || (m.seq as number) <= lastSeq) return null;
  if (!side(m.width) || !side(m.height)) return null;
  if (m.mime !== 'image/png' && m.mime !== 'image/jpeg') return null;
  if (!isIsoInstant(m.capturedAt)) return null;
  return { seq: m.seq as number, width: m.width as number, height: m.height as number, mime: m.mime, capturedAt: m.capturedAt };
}

class WsConsoleStream implements ConsoleStream {
  private current: ConsoleStatus = 'connecting';
  private readonly frames = new Emitter<[ConsoleFrame]>();
  private readonly statuses = new Emitter<[ConsoleStatus, string | undefined, boolean]>();
  private pending: Omit<ConsoleFrame, 'data'> | null = null;
  private lastSeq = 0;
  private closedByClient = false;

  constructor(private readonly ws: WebSocket, ticket: string, private readonly onSessionEnded: () => boolean = () => true) {
    ws.binaryType = 'blob';
    ws.addEventListener('open', () => ws.send(JSON.stringify({ type: 'auth', ticket })));
    ws.addEventListener('message', (e: MessageEvent) => {
      if (this.current === 'closed' || this.current === 'error') return;
      if (typeof e.data === 'string') {
        const m = parseText(e.data);
        if (m?.type === 'frame') {
          const meta = decodeFrameMeta(m, this.lastSeq);
          if (!meta || this.pending) return this.fail('The console stream sent an invalid frame.');
          this.pending = meta;
          this.lastSeq = meta.seq;
          this.setStatus('streaming');
        } else if (m?.type === 'closed') {
          this.setStatus('closed', typeof m.reason === 'string' ? m.reason.slice(0, 200) : undefined);
        } else {
          this.fail('The console stream sent an unreadable message.');
        }
        return;
      }
      const blob = e.data instanceof Blob ? e.data : null;
      if (!blob || !this.pending || blob.size > MAX_FRAME_BYTES) return this.fail('The console stream sent an invalid frame.');
      this.frames.emit({ ...this.pending, data: new Blob([blob], { type: this.pending.mime }) });
      this.pending = null;
    });
    ws.addEventListener('error', () => this.fail('The console connection failed.'));
    ws.addEventListener('close', (e: Event) => {
      let { reason, message } = closeMessage('console', e as CloseEvent);
      if (reason === 'session_ended' && !this.onSessionEnded()) ({ reason, message } = earlierSessionClosed('console'));
      if (reason === 'session_ended') {
        // A 4401 for the current session always ends it, even after a server
        // notice (such as "VM stopped") or a client-side failure already
        // closed the view, so report it even when the status is unchanged.
        this.current = 'closed';
        this.statuses.emit('closed', message, true);
        return;
      }
      if (this.closedByClient || this.current === 'error' || this.current === 'closed') return;
      this.setStatus('closed', message);
    });
  }

  get status(): ConsoleStatus {
    return this.current;
  }

  private fail(reason: string): void {
    if (this.current === 'error' || this.current === 'closed') return;
    this.setStatus('error', reason);
    this.closedByClient = true;
    this.ws.close();
  }

  private setStatus(status: ConsoleStatus, reason?: string, sessionEnded = false): void {
    if (this.current === status) return;
    this.current = status;
    this.statuses.emit(status, reason, sessionEnded);
  }

  onFrame(listener: (frame: ConsoleFrame) => void): () => void {
    return this.frames.on(listener);
  }

  onStatus(listener: (status: ConsoleStatus, reason?: string, sessionEnded?: boolean) => void): () => void {
    return this.statuses.on(listener);
  }

  close(): void {
    this.closedByClient = true;
    this.ws.close();
    this.setStatus('closed');
    this.frames.clear();
    this.statuses.clear();
  }
}
