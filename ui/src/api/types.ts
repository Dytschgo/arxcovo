// Dashboard API contract (proposed). docs/dashboard-api.md is the prose version
// of this file; keep the two in step. Every type here is what the UI consumes
// after decoding, never a raw server payload.

export type Arch = 'x86_64' | 'arm64';
/** v1 platforms. macOS hosts and guests are roadmap items and join the enum when they ship. */
export type OsFamily = 'windows' | 'linux';

export interface User {
  /** Display name of the single cluster owner. */
  name: string;
}

export interface Session {
  user: User;
  /** Synchronizer token for unsafe requests. Kept in memory only. */
  csrfToken: string;
  /** ISO 8601 instant at which the server will reject the session. */
  expiresAt: string;
}

export interface Credentials {
  username: string;
  password: string;
}

export type Reachability = 'online' | 'degraded' | 'offline';
export type HostMode = 'always_on' | 'opportunistic';
export type AgentRights = 'standard' | 'admin';

export interface HostTool {
  name: string;
  version: string | null;
}

export interface Host {
  id: string;
  name: string;
  os: OsFamily;
  osVersion: string;
  arch: Arch;
  cpu: { model: string; cores: number; usedPercent: number | null };
  memory: { totalBytes: number; usedBytes: number | null };
  disk: { totalBytes: number; freeBytes: number; capBytes: number | null };
  tools: HostTool[];
  reachability: Reachability;
  /** Present when reachability is not online. */
  reachabilityReason: string | null;
  lastSeenAt: string;
  mode: HostMode;
  agentRights: AgentRights;
}

export type ActivityOutcome = 'started' | 'succeeded' | 'failed' | 'rejected' | 'refused' | 'interrupted';
export type ActorKind = 'agent' | 'owner' | 'system';

export interface ActivityEntry {
  id: number;
  /** MCP tool or dashboard action name, for example `run_command`. */
  tool: string;
  outcome: ActivityOutcome;
  actor: { kind: ActorKind; name: string };
  hostId: string | null;
  vmId: string | null;
  /** Short, already-redacted human summary. Never contains secrets. */
  summary: string;
  exitCode: number | null;
  startedAt: string | null;
  finishedAt: string | null;
}

export interface ActivityQuery {
  /** 1..50, default 20. */
  limit?: number;
  /** Cursor: return entries with id strictly below this. */
  beforeId?: number;
  outcome?: ActivityOutcome;
  tool?: string;
  hostId?: string;
  includeReads?: boolean;
}

export interface ActivityPage {
  entries: ActivityEntry[];
  nextBeforeId: number | null;
}

export type VmKind = 'test' | 'static';
export type VmState = 'creating' | 'running' | 'stopped' | 'snapshotting' | 'migrating' | 'failed' | 'destroying';

export interface Snapshot {
  id: string;
  name: string;
  createdAt: string | null;
  sizeBytes: number | null;
}

export interface Vm {
  id: string;
  name: string;
  kind: VmKind;
  template: { id: string; version: number };
  guestOs: OsFamily;
  guestOsVersion: string;
  arch: Arch;
  hostId: string;
  state: VmState;
  /** Guest agent answered its readiness check. */
  ready: boolean;
  /** Null for static VMs, which never expire. */
  expiresAt: string | null;
  cpus: number | null;
  memoryBytes: number | null;
  diskBytes: number;
  snapshots: Snapshot[];
}

// --- Terminal --------------------------------------------------------------

export type TerminalStatus = 'connecting' | 'ready' | 'running' | 'closed' | 'error';

export interface TerminalCommand {
  /** One of the target host's discovered tools. */
  tool: string;
  args: string[];
  timeoutS?: number;
}

export type TerminalEvent =
  | { type: 'status'; status: TerminalStatus }
  | { type: 'started'; commandId: string; tool: string; args: string[] }
  | { type: 'output'; commandId: string; stream: 'stdout' | 'stderr'; data: string }
  | { type: 'exit'; commandId: string; exitCode: number | null; outcome: ActivityOutcome; truncated: boolean }
  | { type: 'error'; code: string; message: string }
  /** The server closed the stream; `session_ended` means sign in again. */
  | { type: 'closed'; reason: StreamCloseReason; message: string };

export type StreamCloseReason = 'session_ended' | 'server';

/** WebSocket close codes the server uses (docs/dashboard-api.md). */
export const STREAM_CLOSE_CODES = { sessionEnded: 4401, forbidden: 4403, ticketRejected: 4408 } as const;

export interface TerminalSession {
  readonly status: TerminalStatus;
  onEvent(listener: (event: TerminalEvent) => void): () => void;
  run(command: TerminalCommand): void;
  cancel(): void;
  close(): void;
}

// --- Console ---------------------------------------------------------------

export type ConsoleStatus = 'connecting' | 'streaming' | 'closed' | 'error';

export interface ConsoleFrame {
  seq: number;
  width: number;
  height: number;
  /** image/png or image/jpeg from the server; the stand-in uses image/svg+xml. */
  mime: string;
  data: Blob;
  capturedAt: string;
}

export interface ConsoleStream {
  readonly status: ConsoleStatus;
  onFrame(listener: (frame: ConsoleFrame) => void): () => void;
  /**
   * `sessionEnded` is true when the server closed the stream because the session ended.
   * That report can follow an earlier `closed` status from a server notice.
   */
  onStatus(listener: (status: ConsoleStatus, reason?: string, sessionEnded?: boolean) => void): () => void;
  close(): void;
}

// --- Client ----------------------------------------------------------------

export type AdapterKind = 'http' | 'standin';

export interface ArxApi {
  readonly kind: AdapterKind;
  /** Current session, or null when the cookie is missing or expired. */
  getSession(): Promise<Session | null>;
  login(credentials: Credentials): Promise<Session>;
  logout(): Promise<void>;
  listHosts(): Promise<Host[]>;
  getHost(id: string): Promise<Host>;
  listActivity(query?: ActivityQuery): Promise<ActivityPage>;
  listVms(): Promise<Vm[]>;
  getVm(id: string): Promise<Vm>;
  openTerminal(hostId: string): Promise<TerminalSession>;
  openConsole(vmId: string): Promise<ConsoleStream>;
}

export type ApiErrorCode =
  | 'unauthorized'
  | 'forbidden'
  | 'not_found'
  | 'invalid_request'
  | 'rate_limited'
  | 'unavailable'
  | 'bad_response'
  | 'network';

export class ApiError extends Error {
  readonly code: ApiErrorCode;
  readonly status: number | null;
  constructor(code: ApiErrorCode, message: string, status: number | null = null) {
    super(message);
    this.name = 'ApiError';
    this.code = code;
    this.status = status;
  }
}

export function isApiError(error: unknown, code?: ApiErrorCode): error is ApiError {
  return error instanceof ApiError && (code === undefined || error.code === code);
}

export const ACTIVITY_OUTCOMES: readonly ActivityOutcome[] = ['started', 'succeeded', 'failed', 'rejected', 'refused', 'interrupted'];
export const VM_STATES: readonly VmState[] = ['creating', 'running', 'stopped', 'snapshotting', 'migrating', 'failed', 'destroying'];
